import { sshExec } from "./container.js";
import { config } from "./config.js";

const WORKSPACE = "/home/agent/workspace";

export async function setupGit(containerIp: string): Promise<void> {
  // Configure git user
  await sshExec(containerIp, `git config --global user.name "${config.git.userName}"`);
  await sshExec(containerIp, `git config --global user.email "${config.git.userEmail}"`);

  // Configure GitHub token for HTTPS authentication
  await sshExec(
    containerIp,
    `git config --global url."https://x-access-token:${config.github.token}@github.com/".insteadOf "https://github.com/"`,
  );

  // Configure gh CLI auth
  await sshExec(containerIp, `echo "${config.github.token}" | gh auth login --with-token`, {
    timeout: 15_000,
  });
}

export async function cloneRepo(
  containerIp: string,
  cloneUrl: string,
): Promise<void> {
  // Convert SSH URLs to HTTPS so the token-based auth works
  const httpsUrl = cloneUrl
    .replace(/^git@github\.com:/, "https://github.com/")
    .replace(/\.git$/, "");

  const result = await sshExec(
    containerIp,
    `git clone "${httpsUrl}" "${WORKSPACE}"`,
    { timeout: 120_000 },
  );
  if (result.exitCode !== 0) {
    throw new Error(`git clone failed: ${result.stderr}`);
  }
}

export async function createBranch(
  containerIp: string,
  branchName: string,
  baseBranch: string,
): Promise<void> {
  const result = await sshExec(
    containerIp,
    `cd "${WORKSPACE}" && git checkout -b "${branchName}" "origin/${baseBranch}"`,
  );
  if (result.exitCode !== 0) {
    throw new Error(`git checkout failed: ${result.stderr}`);
  }
}

export async function commitAndPush(
  containerIp: string,
  branchName: string,
  commitMessage: string,
): Promise<void> {
  // Stage all changes
  const addResult = await sshExec(containerIp, `cd "${WORKSPACE}" && git add -A`);
  if (addResult.exitCode !== 0) {
    throw new Error(`git add failed: ${addResult.stderr}`);
  }

  // Check if there are changes to commit
  const diffResult = await sshExec(containerIp, `cd "${WORKSPACE}" && git diff --cached --quiet`);
  if (diffResult.exitCode === 0) {
    throw new Error("No changes to commit");
  }

  // Commit
  const escapedMessage = commitMessage.replace(/"/g, '\\"');
  const commitResult = await sshExec(
    containerIp,
    `cd "${WORKSPACE}" && git commit -m "${escapedMessage}"`,
  );
  if (commitResult.exitCode !== 0) {
    throw new Error(`git commit failed: ${commitResult.stderr}`);
  }

  // Push
  const pushResult = await sshExec(
    containerIp,
    `cd "${WORKSPACE}" && git push origin "${branchName}"`,
    { timeout: 60_000 },
  );
  if (pushResult.exitCode !== 0) {
    throw new Error(`git push failed: ${pushResult.stderr}`);
  }
}

export async function createPr(
  containerIp: string,
  title: string,
  body: string,
  baseBranch: string,
): Promise<string> {
  const escapedTitle = title.replace(/"/g, '\\"');
  const escapedBody = body.replace(/"/g, '\\"');

  const result = await sshExec(
    containerIp,
    `cd "${WORKSPACE}" && gh pr create --title "${escapedTitle}" --body "${escapedBody}" --base "${baseBranch}"`,
    { timeout: 30_000 },
  );
  if (result.exitCode !== 0) {
    throw new Error(`gh pr create failed: ${result.stderr}`);
  }
  // gh pr create outputs the PR URL
  return result.stdout.trim();
}

export function generateBranchName(task: string, sessionIdPrefix: string): string {
  const slug = task
    .toLowerCase()
    .replace(/[^a-z0-9\s-]/g, "")
    .replace(/\s+/g, "-")
    .slice(0, 40);
  return `agent/${slug}-${sessionIdPrefix}`;
}
