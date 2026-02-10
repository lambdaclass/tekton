import Fastify from "fastify";
import { config } from "./config.js";
import { initDb } from "./db.js";
import * as db from "./db.js";
import { createSlackApp } from "./slack.js";
import { cleanupStaleSessions } from "./session.js";

async function main(): Promise<void> {
  console.log("Starting orchestrator...");

  // Initialize database
  initDb(config.dbPath);
  console.log(`Database initialized at ${config.dbPath}`);

  // Seed default repo if configured
  if (config.defaultRepo && config.defaultCloneUrl) {
    db.upsertRepo(config.defaultRepo, config.defaultCloneUrl, undefined, config.defaultBranch);
    console.log(`Default repo: ${config.defaultRepo}`);
  }

  // Start Slack bot
  const slackApp = createSlackApp();
  await slackApp.start();
  console.log("Slack bot connected (Socket Mode)");

  // Start HTTP server for health checks
  const fastify = Fastify({ logger: false });

  fastify.get("/health", async () => {
    return { status: "ok", uptime: process.uptime() };
  });

  fastify.get("/sessions", async () => {
    return db.getActiveSessions();
  });

  await fastify.listen({ port: config.port, host: "127.0.0.1" });
  console.log(`HTTP server listening on http://127.0.0.1:${config.port}`);

  // Periodic cleanup of stale sessions
  setInterval(cleanupStaleSessions, 5 * 60 * 1000);

  // Graceful shutdown
  const shutdown = async () => {
    console.log("Shutting down...");
    await fastify.close();
    await slackApp.stop();
    process.exit(0);
  };
  process.on("SIGTERM", shutdown);
  process.on("SIGINT", shutdown);

  console.log("Orchestrator ready!");
}

main().catch((err) => {
  console.error("Fatal error:", err);
  process.exit(1);
});
