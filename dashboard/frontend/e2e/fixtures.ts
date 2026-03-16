import { test as base, Page } from "@playwright/test";
import * as path from "path";
import { fileURLToPath } from "url";
import { collectCoverage } from "./coverage";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

// Test data IDs matching seed.sql
export const TEST_IDS = {
  tasks: {
    pending: "task-pending-1",
    running: "task-running-1",
    completed: "task-completed-1",
    failed: "task-failed-1",
    completed2: "task-completed-2",
    subtask: "task-subtask-1",
    awaiting: "task-awaiting-1",
    completedNoPR: "task-completed-nopr",
    tinyCost: "task-tiny-cost",
    old: "task-old",
  },
  users: {
    admin: "testadmin",
    member: "testmember",
    viewer: "testviewer",
  },
  repos: {
    main: "testorg/testrepo",
    frontend: "testorg/frontend",
  },
  org: "testorg",
  intake: {
    sources: {
      github: { name: 'GitHub Bugs', repo: 'testorg/testrepo' },
      linear: { name: 'Linear Features', repo: 'testorg/frontend' },
    },
    issues: {
      backlogAuth: 'Fix null pointer in auth module',
      backlogDarkMode: 'Add dark mode toggle',
      pendingSafari: 'Login page crashes on Safari',
      pendingCsv: 'Implement CSV export',
      inProgressRateLimit: 'Add rate limiting to API',
      reviewSecurity: 'Review security headers',
      doneDeps: 'Upgrade dependencies to latest',
      failedCi: 'Fix flaky CI test',
    },
  },
} as const;

type TestFixtures = {
  authenticatedPage: Page;
  adminPage: Page;
  memberPage: Page;
  viewerPage: Page;
};

export const test = base.extend<TestFixtures>({
  page: async ({ page }, use) => {
    await use(page);
    await collectCoverage(page);
  },

  authenticatedPage: async ({ browser }, use) => {
    const context = await browser.newContext({
      storageState: path.join(__dirname, ".auth", "admin.json"),
    });
    const page = await context.newPage();
    await use(page);
    await collectCoverage(page);
    await context.close();
  },

  adminPage: async ({ browser }, use) => {
    const context = await browser.newContext({
      storageState: path.join(__dirname, ".auth", "admin.json"),
    });
    const page = await context.newPage();
    await use(page);
    await collectCoverage(page);
    await context.close();
  },

  memberPage: async ({ browser }, use) => {
    const context = await browser.newContext({
      storageState: path.join(__dirname, ".auth", "member.json"),
    });
    const page = await context.newPage();
    await use(page);
    await collectCoverage(page);
    await context.close();
  },

  viewerPage: async ({ browser }, use) => {
    const context = await browser.newContext({
      storageState: path.join(__dirname, ".auth", "viewer.json"),
    });
    const page = await context.newPage();
    await use(page);
    await collectCoverage(page);
    await context.close();
  },
});

export { expect } from "@playwright/test";
