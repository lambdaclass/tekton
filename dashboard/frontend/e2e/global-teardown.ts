import { FullConfig } from "@playwright/test";
import { execSync } from "child_process";
import * as fs from "fs";
import * as path from "path";
import { fileURLToPath } from "url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const DATABASE_URL =
  process.env.DATABASE_URL ||
  "postgres://tekton:tekton_test_password@localhost:5432/tekton_test";

async function globalTeardown(_config: FullConfig): Promise<void> { // eslint-disable-line @typescript-eslint/no-unused-vars
  // Drop all seeded tables to leave the database clean
  console.log("Cleaning up test database...");
  const dropSql = `
    DROP TABLE IF EXISTS intake_poll_log CASCADE;
    DROP TABLE IF EXISTS intake_issues CASCADE;
    DROP TABLE IF EXISTS intake_sources CASCADE;
    DROP TABLE IF EXISTS task_state_transitions CASCADE;
    DROP TABLE IF EXISTS task_actions CASCADE;
    DROP TABLE IF EXISTS task_messages CASCADE;
    DROP TABLE IF EXISTS task_logs CASCADE;
    DROP TABLE IF EXISTS user_repo_permissions CASCADE;
    DROP TABLE IF EXISTS secrets CASCADE;
    DROP TABLE IF EXISTS repo_policies CASCADE;
    DROP TABLE IF EXISTS org_policies CASCADE;
    DROP TABLE IF EXISTS audit_log CASCADE;
    DROP TABLE IF EXISTS budgets CASCADE;
    DROP TABLE IF EXISTS tasks CASCADE;
    DROP TABLE IF EXISTS users CASCADE;
  `;
  try {
    execSync(`psql "${DATABASE_URL}" -c "${dropSql.replace(/\n/g, " ")}"`, {
      stdio: "inherit",
    });
    console.log("Database cleaned.");
  } catch (e) {
    console.warn("Database cleanup warning:", e);
  }

  // Remove auth storage state files
  const authDir = path.join(__dirname, ".auth");
  if (fs.existsSync(authDir)) {
    fs.rmSync(authDir, { recursive: true, force: true });
  }
}

export default globalTeardown;
