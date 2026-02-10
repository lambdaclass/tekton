import Database from "better-sqlite3";
import { mkdirSync } from "node:fs";
import { dirname } from "node:path";
import type { Repo, Session, Message, SessionStatus } from "./types.js";

let db: Database.Database;

export function initDb(dbPath: string): Database.Database {
  mkdirSync(dirname(dbPath), { recursive: true });
  db = new Database(dbPath);
  db.pragma("journal_mode = WAL");
  db.pragma("foreign_keys = ON");
  migrate();
  return db;
}

function migrate(): void {
  db.exec(`
    CREATE TABLE IF NOT EXISTS repos (
      id INTEGER PRIMARY KEY,
      name TEXT UNIQUE NOT NULL,
      clone_url TEXT NOT NULL,
      description TEXT,
      default_branch TEXT DEFAULT 'main'
    );

    CREATE TABLE IF NOT EXISTS sessions (
      id TEXT PRIMARY KEY,
      repo_id INTEGER REFERENCES repos(id),
      container_name TEXT,
      container_ip TEXT,
      branch_name TEXT,
      pr_url TEXT,
      status TEXT DEFAULT 'pending',
      error_message TEXT,
      slack_channel TEXT,
      slack_thread_ts TEXT,
      task TEXT NOT NULL,
      created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
      updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
    );

    CREATE TABLE IF NOT EXISTS messages (
      id INTEGER PRIMARY KEY,
      session_id TEXT REFERENCES sessions(id),
      role TEXT NOT NULL,
      content TEXT NOT NULL,
      created_at DATETIME DEFAULT CURRENT_TIMESTAMP
    );
  `);
}

// Repo queries
export function upsertRepo(
  name: string,
  cloneUrl: string,
  description?: string,
  defaultBranch?: string,
): Repo {
  const stmt = db.prepare(`
    INSERT INTO repos (name, clone_url, description, default_branch)
    VALUES (?, ?, ?, ?)
    ON CONFLICT(name) DO UPDATE SET
      clone_url = excluded.clone_url,
      description = COALESCE(excluded.description, repos.description),
      default_branch = COALESCE(excluded.default_branch, repos.default_branch)
    RETURNING *
  `);
  return stmt.get(name, cloneUrl, description ?? null, defaultBranch ?? "main") as Repo;
}

export function getRepo(name: string): Repo | undefined {
  return db.prepare("SELECT * FROM repos WHERE name = ?").get(name) as Repo | undefined;
}

export function getRepoById(id: number): Repo | undefined {
  return db.prepare("SELECT * FROM repos WHERE id = ?").get(id) as Repo | undefined;
}

// Session queries
export function createSession(
  id: string,
  repoId: number,
  task: string,
  slackChannel?: string,
  slackThreadTs?: string,
): Session {
  const stmt = db.prepare(`
    INSERT INTO sessions (id, repo_id, task, slack_channel, slack_thread_ts)
    VALUES (?, ?, ?, ?, ?)
    RETURNING *
  `);
  return stmt.get(id, repoId, task, slackChannel ?? null, slackThreadTs ?? null) as Session;
}

export function getSession(id: string): Session | undefined {
  return db.prepare("SELECT * FROM sessions WHERE id = ?").get(id) as Session | undefined;
}

export function updateSession(
  id: string,
  updates: Partial<
    Pick<
      Session,
      "container_name" | "container_ip" | "branch_name" | "pr_url" | "status" | "error_message"
    >
  >,
): void {
  const fields = Object.entries(updates).filter(([, v]) => v !== undefined);
  if (fields.length === 0) return;

  const sets = fields.map(([k]) => `${k} = ?`).join(", ");
  const values = fields.map(([, v]) => v);
  db.prepare(`UPDATE sessions SET ${sets}, updated_at = CURRENT_TIMESTAMP WHERE id = ?`).run(
    ...values,
    id,
  );
}

export function getSessionByThread(
  slackChannel: string,
  slackThreadTs: string,
): Session | undefined {
  return db
    .prepare("SELECT * FROM sessions WHERE slack_channel = ? AND slack_thread_ts = ?")
    .get(slackChannel, slackThreadTs) as Session | undefined;
}

export function getActiveSessions(): Session[] {
  return db
    .prepare("SELECT * FROM sessions WHERE status NOT IN ('done', 'failed') ORDER BY created_at DESC")
    .all() as Session[];
}

// Message queries
export function addMessage(sessionId: string, role: Message["role"], content: string): Message {
  const stmt = db.prepare(`
    INSERT INTO messages (session_id, role, content)
    VALUES (?, ?, ?)
    RETURNING *
  `);
  return stmt.get(sessionId, role, content) as Message;
}

export function getMessages(sessionId: string): Message[] {
  return db
    .prepare("SELECT * FROM messages WHERE session_id = ? ORDER BY created_at ASC")
    .all(sessionId) as Message[];
}
