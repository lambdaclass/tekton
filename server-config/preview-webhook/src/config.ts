import { type TokenProvider, GitHubAppTokenProvider } from "./auth.js";

type Env = Record<string, string | undefined>;

function requireEnv(env: Env, name: string): string {
  const value = env[name];
  if (!value) {
    throw new Error(`Missing required environment variable: ${name}`);
  }
  return value;
}

function optionalEnv(env: Env, name: string, defaultValue: string): string {
  return env[name] || defaultValue;
}

export function createTokenProvider(env: Env = process.env): TokenProvider {
  if (env["GITHUB_TOKEN"] !== undefined) {
    throw new Error(
      "GITHUB_TOKEN is set, but PAT auth has been removed. Remove GITHUB_TOKEN from " +
        "/var/secrets/preview.env, revoke the PAT on GitHub, and configure the GitHub App " +
        "(GITHUB_APP_ID, GITHUB_APP_INSTALLATION_ID, GITHUB_APP_PRIVATE_KEY_PATH)."
    );
  }

  const names = ["GITHUB_APP_ID", "GITHUB_APP_INSTALLATION_ID", "GITHUB_APP_PRIVATE_KEY_PATH"] as const;
  const missing = names.filter((n) => !env[n]);
  if (missing.length > 0) {
    throw new Error(`Missing GitHub App config: ${missing.join(", ")}`);
  }
  const appId = env["GITHUB_APP_ID"]!;
  const installationId = env["GITHUB_APP_INSTALLATION_ID"]!;
  for (const [name, value] of [["GITHUB_APP_ID", appId], ["GITHUB_APP_INSTALLATION_ID", installationId]]) {
    if (!/^\d+$/.test(value)) {
      throw new Error(`${name} must be numeric`);
    }
  }

  console.log("[config] Auth mode: GitHub App");
  return GitHubAppTokenProvider.fromKeyFile(appId, installationId, env["GITHUB_APP_PRIVATE_KEY_PATH"]!);
}

export interface Config {
  githubWebhookSecret: string;
  previewDomain: string;
  port: number;
  allowedRepos: string[];
}

export function loadConfig(env: Env = process.env): Config {
  return {
    githubWebhookSecret: requireEnv(env, "GITHUB_WEBHOOK_SECRET"),
    previewDomain: requireEnv(env, "PREVIEW_DOMAIN"),
    port: parseInt(optionalEnv(env, "WEBHOOK_PORT", "3100"), 10),
    allowedRepos: optionalEnv(env, "ALLOWED_REPOS", "")
      .split(",")
      .map((r) => r.trim())
      .filter(Boolean),
  };
}
