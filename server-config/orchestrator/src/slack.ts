import { App, LogLevel } from "@slack/bolt";
import { config } from "./config.js";
import * as session from "./session.js";
import * as db from "./db.js";
import type { SessionCallbacks, StreamEvent } from "./types.js";

let app: App;

export function createSlackApp(): App {
  app = new App({
    token: config.slack.botToken,
    appToken: config.slack.appToken,
    socketMode: true,
    logLevel: LogLevel.INFO,
  });

  registerHandlers();
  return app;
}

function registerHandlers(): void {
  // Handle @mentions — start a new session
  app.event("app_mention", async ({ event, say }) => {
    // Strip the bot mention from the text
    const task = event.text.replace(/<@[A-Z0-9]+>/g, "").trim();

    if (!task) {
      await say({
        text: "Please provide a task description. Example: `@agent Add input validation to the signup form`",
        thread_ts: event.ts,
      });
      return;
    }

    // Determine repo (MVP: use default repo, or channel mapping)
    const repoName = config.defaultRepo;
    const cloneUrl = config.defaultCloneUrl;

    if (!repoName || !cloneUrl) {
      await say({
        text: "No default repo configured. Set DEFAULT_REPO and DEFAULT_CLONE_URL environment variables.",
        thread_ts: event.ts,
      });
      return;
    }

    // React with eyes to acknowledge
    try {
      await app.client.reactions.add({
        channel: event.channel,
        name: "eyes",
        timestamp: event.ts,
      });
    } catch {
      // Reaction may fail if already added
    }

    // Post initial status message
    const statusMsg = await say({
      text: ":hourglass_flowing_sand: Starting session...",
      thread_ts: event.ts,
    });

    const threadTs = event.ts;
    const statusTs = statusMsg.ts!;

    // Create callbacks that update Slack
    const callbacks = createSlackCallbacks(event.channel, threadTs, statusTs);

    // Start session
    try {
      await session.startSession(task, repoName, cloneUrl, callbacks, {
        slackChannel: event.channel,
        slackThreadTs: threadTs,
        baseBranch: config.defaultBranch,
      });
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      await updateMessage(event.channel, statusTs, `:x: Failed to start session: ${message}`);
    }
  });

  // Handle thread replies — follow-up prompts
  app.event("message", async ({ event }) => {
    // Only handle threaded messages (replies)
    if (!("thread_ts" in event) || !event.thread_ts) return;
    // Skip bot messages
    if ("bot_id" in event && event.bot_id) return;
    // Skip subtypes (like message_changed)
    if ("subtype" in event && event.subtype) return;

    const text = ("text" in event ? event.text : "") ?? "";
    const channel = event.channel;
    const threadTs = event.thread_ts;

    // Look up session by thread
    const existingSession = db.getSessionByThread(channel, threadTs);
    if (!existingSession) return; // Not a session thread

    // Strip any bot mention
    const message = text.replace(/<@[A-Z0-9]+>/g, "").trim();
    if (!message) return;

    // Post acknowledgment
    const statusMsg = await app.client.chat.postMessage({
      channel,
      thread_ts: threadTs,
      text: ":hourglass_flowing_sand: Processing follow-up...",
    });

    const callbacks = createSlackCallbacks(channel, threadTs, statusMsg.ts!);

    try {
      await session.followUp(existingSession.id, message, callbacks);
    } catch (err) {
      const errorMsg = err instanceof Error ? err.message : String(err);
      await updateMessage(channel, statusMsg.ts!, `:x: Follow-up failed: ${errorMsg}`);
    }
  });
}

function createSlackCallbacks(
  channel: string,
  threadTs: string,
  statusMessageTs: string,
): SessionCallbacks {
  return {
    onStatusChange: async (status, detail) => {
      const emoji = statusEmoji(status);
      const text = detail ?? status;
      await updateMessage(channel, statusMessageTs, `${emoji} ${text}`);
    },

    onStreamEvent: async (event: StreamEvent) => {
      if (event.type === "tool_use") {
        await updateMessage(
          channel,
          statusMessageTs,
          `:hammer_and_wrench: ${event.content}`,
        );
      }
    },

    onComplete: async (prUrl: string) => {
      // Replace the status message
      await updateMessage(
        channel,
        statusMessageTs,
        `:white_check_mark: Done!`,
      );
      // Post the PR link as a separate message in the thread
      await app.client.chat.postMessage({
        channel,
        thread_ts: threadTs,
        text: `:rocket: PR created: ${prUrl}`,
        unfurl_links: true,
      });

      // Add checkmark reaction to original message
      try {
        await app.client.reactions.add({
          channel,
          name: "white_check_mark",
          timestamp: threadTs,
        });
      } catch {
        // Reaction may fail
      }
    },

    onError: async (error: string) => {
      await updateMessage(channel, statusMessageTs, `:x: Failed: ${error}`);
    },
  };
}

async function updateMessage(channel: string, ts: string, text: string): Promise<void> {
  try {
    await app.client.chat.update({
      channel,
      ts,
      text,
    });
  } catch (err) {
    console.error("Failed to update Slack message:", err);
  }
}

function statusEmoji(status: string): string {
  switch (status) {
    case "creating_container":
      return ":package:";
    case "cloning":
      return ":arrow_down:";
    case "running":
      return ":brain:";
    case "pushing":
      return ":arrow_up:";
    case "done":
      return ":white_check_mark:";
    case "failed":
      return ":x:";
    default:
      return ":hourglass_flowing_sand:";
  }
}
