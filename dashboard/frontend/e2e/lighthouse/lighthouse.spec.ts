import { test, expect } from "@playwright/test";
import * as fs from "fs";
import * as path from "path";
import { fileURLToPath } from "url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
import {
  launchChrome,
  runLighthouseAudit,
  THRESHOLDS,
  type AuditScores,
  type CategoryId,
} from "./lighthouse-config";
import { TEST_IDS } from "../fixtures";

const BASE_URL = process.env.BASE_URL || "http://localhost:3200";
const STORAGE_STATE_PATH = path.join(__dirname, "..", ".auth", "admin.json");

/** Read the dashboard_session cookie value from the Playwright storage state. */
function getAuthCookie(): string {
  const state = JSON.parse(fs.readFileSync(STORAGE_STATE_PATH, "utf-8"));
  const cookie = state.cookies?.find(
    (c: { name: string }) => c.name === "dashboard_session",
  );
  if (!cookie) {
    throw new Error(
      `dashboard_session cookie not found in ${STORAGE_STATE_PATH}`,
    );
  }
  return cookie.value;
}

function assertScores(scores: AuditScores, page: string): void {
  for (const [category, threshold] of Object.entries(THRESHOLDS)) {
    const actual = scores[category as CategoryId];
    console.log(
      `  ${category}: ${actual} (threshold: ${threshold})`,
    );
    expect
      .soft(actual, `${page} — ${category} score ${actual} < ${threshold}`)
      .toBeGreaterThanOrEqual(threshold);
  }
}

const PAGES: { name: string; path: string }[] = [
  { name: "Home page", path: "/" },
  { name: "Tasks list", path: "/tasks" },
  { name: "Task detail", path: `/tasks/${TEST_IDS.tasks.completed}` },
  { name: "Admin panel", path: "/admin" },
  { name: "Cost dashboard", path: "/cost" },
];

test.describe.serial("Lighthouse audits", () => {
  let chrome: Awaited<ReturnType<typeof launchChrome>>;
  let authCookie: string;

  test.beforeAll(async () => {
    authCookie = getAuthCookie();
    chrome = await launchChrome();
    console.log(`Chrome launched on port ${chrome.port}`);
  });

  test.afterAll(async () => {
    await chrome?.kill();
  });

  for (const page of PAGES) {
    test(`${page.name} meets audit thresholds`, async () => {
      test.setTimeout(60_000);

      const url = `${BASE_URL}${page.path}`;
      console.log(`Auditing ${url} ...`);

      const scores = await runLighthouseAudit(url, chrome.port, {
        Cookie: `dashboard_session=${authCookie}`,
      });

      console.log(`Scores for ${page.name}:`);
      assertScores(scores, page.name);
    });
  }
});
