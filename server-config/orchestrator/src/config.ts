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
  slack: {
    botToken: requireEnv("SLACK_BOT_TOKEN"),
    appToken: requireEnv("SLACK_APP_TOKEN"),
  },
  github: {
    token: requireEnv("GITHUB_TOKEN"),
  },
  git: {
    userName: optionalEnv("GIT_USER_NAME", "Claude Agent"),
    userEmail: optionalEnv("GIT_USER_EMAIL", "agent@example.com"),
  },
  defaultRepo: optionalEnv("DEFAULT_REPO", ""),
  defaultCloneUrl: optionalEnv("DEFAULT_CLONE_URL", ""),
  defaultBranch: optionalEnv("DEFAULT_BRANCH", "main"),
  dbPath: optionalEnv("DB_PATH", "/var/lib/orchestrator/orchestrator.db"),
  sessionTimeoutMinutes: parseInt(optionalEnv("SESSION_TIMEOUT_MINUTES", "60"), 10),
  port: parseInt(optionalEnv("PORT", "3000"), 10),
} as const;
