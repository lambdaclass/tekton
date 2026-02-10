import { v4 as uuidv4 } from "uuid";
import { config } from "./config.js";
import * as db from "./db.js";
import * as container from "./container.js";
import * as git from "./git.js";
import { processStreamChunk, summarizeToolUse } from "./stream.js";
import type { Session, SessionCallbacks, StreamEvent } from "./types.js";

// Track active Claude processes per session for follow-ups
const activeSessions = new Map<string, { running: boolean }>();

export async function startSession(
  task: string,
  repoName: string,
  cloneUrl: string,
  callbacks: SessionCallbacks,
  options?: { slackChannel?: string; slackThreadTs?: string; baseBranch?: string },
): Promise<Session> {
  const sessionId = uuidv4();
  const sessionPrefix = sessionId.slice(0, 8);

  // Ensure repo exists in DB
  const repo = db.upsertRepo(repoName, cloneUrl, undefined, options?.baseBranch ?? config.defaultBranch);

  // Create session record
  const session = db.createSession(
    sessionId,
    repo.id,
    task,
    options?.slackChannel,
    options?.slackThreadTs,
  );
  db.addMessage(sessionId, "user", task);

  const containerName = `agent-${sessionPrefix}`;
  const branchName = git.generateBranchName(task, sessionPrefix);
  const baseBranch = options?.baseBranch ?? repo.default_branch;

  activeSessions.set(sessionId, { running: true });

  // Run the full lifecycle asynchronously
  runSessionLifecycle(
    sessionId,
    containerName,
    branchName,
    baseBranch,
    cloneUrl,
    task,
    callbacks,
  ).catch(async (err) => {
    const message = err instanceof Error ? err.message : String(err);
    db.updateSession(sessionId, { status: "failed", error_message: message });
    await callbacks.onError(message);
    activeSessions.delete(sessionId);
  });

  return session;
}

async function runSessionLifecycle(
  sessionId: string,
  containerName: string,
  branchName: string,
  baseBranch: string,
  cloneUrl: string,
  task: string,
  callbacks: SessionCallbacks,
): Promise<void> {
  try {
    // Step 1: Create container
    await callbacks.onStatusChange("creating_container", "Creating sandbox container...");
    db.updateSession(sessionId, { status: "creating_container", container_name: containerName });

    const info = await container.createContainer(containerName);
    db.updateSession(sessionId, { container_ip: info.ip });

    // Wait for SSH to be available
    await container.waitForSsh(info.ip);

    // Step 2: Set up git and clone repo
    await callbacks.onStatusChange("cloning", `Cloning ${cloneUrl}...`);
    db.updateSession(sessionId, { status: "cloning" });

    await git.setupGit(info.ip);
    await git.cloneRepo(info.ip, cloneUrl);
    await git.createBranch(info.ip, branchName, baseBranch);
    db.updateSession(sessionId, { branch_name: branchName });

    // Step 3: Run Claude
    await callbacks.onStatusChange("running", "Running Claude on task...");
    db.updateSession(sessionId, { status: "running" });

    await runClaude(sessionId, info.ip, task, callbacks);

    // Step 4: Commit, push, create PR
    await callbacks.onStatusChange("pushing", "Pushing changes and creating PR...");
    db.updateSession(sessionId, { status: "pushing" });

    // Generate commit message from task
    const commitMessage = `agent: ${task.slice(0, 72)}`;
    await git.commitAndPush(info.ip, branchName, commitMessage);

    // Create PR
    const prTitle = task.length > 70 ? task.slice(0, 67) + "..." : task;
    const prBody = [
      "## Summary",
      "",
      `Task: ${task}`,
      "",
      `Session: \`${sessionId}\``,
      "",
      "---",
      "*Created by background agent*",
    ].join("\n");

    const prUrl = await git.createPr(info.ip, prTitle, prBody, baseBranch);
    db.updateSession(sessionId, { status: "done", pr_url: prUrl });

    await callbacks.onComplete(prUrl);
  } finally {
    activeSessions.delete(sessionId);
  }
}

async function runClaude(
  sessionId: string,
  containerIp: string,
  task: string,
  callbacks: SessionCallbacks,
): Promise<void> {
  const escapedTask = task.replace(/'/g, "'\\''");
  const command = `cd /home/agent/workspace && claude -p '${escapedTask}' --dangerously-skip-permissions --output-format stream-json`;

  let buffer = "";
  const fileEdits = new Set<string>();
  let lastUpdateTime = 0;
  const MIN_UPDATE_INTERVAL_MS = 2_000;

  const exitCode = await container.sshExecStreaming(
    containerIp,
    command,
    (chunk: string) => {
      buffer = processStreamChunk(buffer, chunk, (event: StreamEvent) => {
        // Track file edits
        if (event.type === "tool_use" && event.filePath) {
          fileEdits.add(event.filePath);
        }

        // Rate-limit Slack updates
        const now = Date.now();
        if (now - lastUpdateTime >= MIN_UPDATE_INTERVAL_MS) {
          lastUpdateTime = now;
          if (event.type === "tool_use") {
            const summary = summarizeToolUse(event);
            callbacks.onStreamEvent(event).catch(() => {});
          } else if (event.type === "error") {
            callbacks.onStreamEvent(event).catch(() => {});
          }
        }
      });
    },
    { timeout: 600_000 }, // 10 min timeout for Claude
  );

  // Store Claude's completion in messages
  db.addMessage(sessionId, "assistant", `Completed with exit code ${exitCode}`);

  if (exitCode !== 0) {
    throw new Error(`Claude exited with code ${exitCode}`);
  }
}

export async function followUp(
  sessionId: string,
  message: string,
  callbacks: SessionCallbacks,
): Promise<void> {
  const session = db.getSession(sessionId);
  if (!session) throw new Error(`Session ${sessionId} not found`);
  if (!session.container_ip) throw new Error("Session has no container");

  const state = activeSessions.get(sessionId);
  if (state?.running) {
    throw new Error("Claude is still running on this session");
  }

  activeSessions.set(sessionId, { running: true });
  db.addMessage(sessionId, "user", message);
  db.updateSession(sessionId, { status: "running" });

  try {
    await callbacks.onStatusChange("running", "Running follow-up...");
    await runClaude(sessionId, session.container_ip, message, callbacks);

    // Push updated changes
    await callbacks.onStatusChange("pushing", "Pushing updates...");
    db.updateSession(sessionId, { status: "pushing" });

    const commitMessage = `agent: ${message.slice(0, 72)}`;
    await git.commitAndPush(session.container_ip, session.branch_name!, commitMessage);

    db.updateSession(sessionId, { status: "done" });
    await callbacks.onStatusChange("done", "Follow-up changes pushed!");
  } catch (err) {
    const errorMsg = err instanceof Error ? err.message : String(err);
    db.updateSession(sessionId, { status: "failed", error_message: errorMsg });
    await callbacks.onError(errorMsg);
  } finally {
    activeSessions.delete(sessionId);
  }
}

export async function destroySession(sessionId: string): Promise<void> {
  const session = db.getSession(sessionId);
  if (!session) return;

  if (session.container_name) {
    try {
      await container.destroyContainer(session.container_name);
    } catch {
      // Container may already be gone
    }
  }

  db.updateSession(sessionId, { status: "done" });
  activeSessions.delete(sessionId);
}

export function cleanupStaleSessions(): void {
  const sessions = db.getActiveSessions();
  const cutoff = Date.now() - config.sessionTimeoutMinutes * 60 * 1000;

  for (const session of sessions) {
    const updatedAt = new Date(session.updated_at).getTime();
    if (updatedAt < cutoff) {
      console.log(`Cleaning up stale session ${session.id}`);
      destroySession(session.id).catch((err) =>
        console.error(`Failed to cleanup session ${session.id}:`, err),
      );
    }
  }
}
