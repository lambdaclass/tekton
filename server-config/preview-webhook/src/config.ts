import { type TokenProvider, PatTokenProvider, GitHubAppTokenProvider } from "./auth.js";

function requireEnv(name: string): string {
  const value = process.env[name];
  if (!value) {
    throw new Error(`Missing required environment variable: ${name}`);
  }
  return value;
}

function optionalEnv(name: string, defaultValue: string): string {
  return process.env[name] || defaultValue;
}

function createTokenProvider(): TokenProvider {
  const appId = process.env["GITHUB_APP_ID"];
  const installationId = process.env["GITHUB_APP_INSTALLATION_ID"];
  const privateKeyPath = process.env["GITHUB_APP_PRIVATE_KEY_PATH"];

  if (appId && installationId && privateKeyPath) {
    console.log("[config] Auth mode: GitHub App");
    return new GitHubAppTokenProvider(appId, installationId, privateKeyPath);
  }

  const token = process.env["GITHUB_TOKEN"];
  if (token) {
    console.log("[config] Auth mode: PAT");
    return new PatTokenProvider(token);
  }

  throw new Error(
    "Missing GitHub auth config. Set GITHUB_TOKEN, or set GITHUB_APP_ID + GITHUB_APP_INSTALLATION_ID + GITHUB_APP_PRIVATE_KEY_PATH."
  );
}

export const tokenProvider = createTokenProvider();

export const config = {
  githubWebhookSecret: requireEnv("GITHUB_WEBHOOK_SECRET"),
  previewDomain: requireEnv("PREVIEW_DOMAIN"),
  port: parseInt(optionalEnv("WEBHOOK_PORT", "3100"), 10),
  allowedRepos: optionalEnv("ALLOWED_REPOS", "")
    .split(",")
    .map((r) => r.trim())
    .filter(Boolean),
} as const;
