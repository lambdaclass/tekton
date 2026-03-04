import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: "./e2e",
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  workers: process.env.CI ? 1 : undefined,
  reporter: "html",
  timeout: 30_000,

  use: {
    baseURL: "http://localhost:3200",
    trace: "on-first-retry",
    screenshot: "only-on-failure",
  },

  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
    {
      name: "lighthouse",
      testMatch: /lighthouse\.spec\.ts/,
      use: { ...devices["Desktop Chrome"] },
    },
  ],

  globalSetup: "./e2e/global-setup.ts",
  globalTeardown: "./e2e/global-teardown.ts",

  webServer: {
    command: "cd ../backend && cargo run --release",
    port: 3200,
    reuseExistingServer: !process.env.CI,
    timeout: 120_000,
    env: {
      DATABASE_URL:
        process.env.DATABASE_URL ||
        "postgres://localhost:5432/tekton_test",
      JWT_SECRET: process.env.JWT_SECRET || "test-secret-key-for-ci",
      GITHUB_CLIENT_ID: process.env.GITHUB_CLIENT_ID || "test-client-id",
      GITHUB_CLIENT_SECRET:
        process.env.GITHUB_CLIENT_SECRET || "test-client-secret",
      GITHUB_REDIRECT_URI:
        process.env.GITHUB_REDIRECT_URI ||
        "http://localhost:3200/api/auth/callback",
      GITHUB_ORG: process.env.GITHUB_ORG || "testorg",
      STATIC_DIR: process.env.STATIC_DIR || "../frontend/dist",
      SECRETS_ENCRYPTION_KEY: "test-encryption-key-32chars-ok!",
      PREVIEW_DOMAIN: "preview.test.dev",
      TEST_MODE: "true",
    },
  },
});
