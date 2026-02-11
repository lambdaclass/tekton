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

export const config = {
  githubWebhookSecret: requireEnv("GITHUB_WEBHOOK_SECRET"),
  githubToken: requireEnv("GITHUB_TOKEN"),
  previewDomain: requireEnv("PREVIEW_DOMAIN"),
  port: parseInt(optionalEnv("WEBHOOK_PORT", "3100"), 10),
  allowedRepos: optionalEnv("ALLOWED_REPOS", "")
    .split(",")
    .map((r) => r.trim())
    .filter(Boolean),
} as const;
