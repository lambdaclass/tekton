use std::env;
use std::sync::Arc;

use crate::error::AppError;
use crate::github_app::GitHubApp;

#[derive(Clone)]
pub struct Config {
    pub listen_addr: String,
    pub database_url: String,
    pub jwt_secret: String,
    pub github_client_id: String,
    pub github_client_secret: String,
    pub github_redirect_uri: String,
    pub github_org: String,
    pub preview_domain: String,
    pub allowed_repos: Vec<String>,
    pub preview_bin: String,
    pub agent_bin: String,
    /// Where the host keeps bare mirrors of task repos (see `mirror.rs`).
    pub mirror_dir: String,
    pub static_dir: String,
    pub claude_bin: String,
    pub claude_config_dir: String,
    pub chromium_bin: String,
    pub secrets_encryption_key: String,
    pub intake_enabled: bool,
    pub intake_max_global_concurrent: i32,
    pub github_webhook_secret: String,
    /// Base URL of the Slack Web API; tests point it at a mock server
    pub slack_api_url: String,
    /// Base URL of the GitHub REST API for checking a user's repo access; tests point it at
    /// a mock server
    pub github_api_url: String,
    /// Mints the tokens for task branches, commits and intake PRs. `None` when unconfigured.
    pub github_app: Option<Arc<GitHubApp>>,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            listen_addr: env::var("LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:3200".into()),
            database_url: env::var("DATABASE_URL")
                .unwrap_or_else(|_| "sqlite:///var/lib/dashboard/dashboard.db".into()),
            jwt_secret: env::var("JWT_SECRET")?,
            github_client_id: env::var("GITHUB_CLIENT_ID")?,
            github_client_secret: env::var("GITHUB_CLIENT_SECRET")?,
            github_redirect_uri: env::var("GITHUB_REDIRECT_URI")?,
            github_org: env::var("GITHUB_ORG")?,
            preview_domain: env::var("PREVIEW_DOMAIN").unwrap_or_else(|_| "example.com".into()),
            allowed_repos: env::var("ALLOWED_REPOS")
                .unwrap_or_default()
                .split(',')
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect(),
            preview_bin: env::var("PREVIEW_BIN")
                .unwrap_or_else(|_| "/run/current-system/sw/bin/preview".into()),
            agent_bin: env::var("AGENT_BIN")
                .unwrap_or_else(|_| "/run/current-system/sw/bin/agent".into()),
            mirror_dir: env::var("MIRROR_DIR")
                .unwrap_or_else(|_| "/var/lib/dashboard/mirrors".into()),
            static_dir: env::var("STATIC_DIR").unwrap_or_else(|_| "./static".into()),
            claude_bin: env::var("CLAUDE_BIN")
                .unwrap_or_else(|_| "/run/current-system/sw/bin/claude".into()),
            claude_config_dir: env::var("CLAUDE_CONFIG_DIR")
                .unwrap_or_else(|_| "/var/secrets/claude".into()),
            chromium_bin: env::var("CHROMIUM_BIN").unwrap_or_else(|_| "chromium".into()),
            secrets_encryption_key: secrets_encryption_key()?,
            intake_enabled: env::var("INTAKE_ENABLED").unwrap_or_else(|_| "false".into()) == "true",
            intake_max_global_concurrent: env::var("INTAKE_MAX_GLOBAL_CONCURRENT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(10),
            github_webhook_secret: env::var("GITHUB_WEBHOOK_SECRET")
                .unwrap_or_else(|_| String::new()),
            slack_api_url: env::var("SLACK_API_URL")
                .unwrap_or_else(|_| "https://slack.com/api".into()),
            github_api_url: env::var("GITHUB_API_URL")
                .map(|u| u.trim_end_matches('/').to_string())
                .unwrap_or_else(|_| "https://api.github.com".into()),
            github_app: GitHubApp::from_env()?,
        })
    }
}

impl Config {
    /// The GitHub App, or an error naming the missing settings.
    pub fn github_app(&self) -> Result<&GitHubApp, AppError> {
        self.github_app.as_deref().ok_or_else(|| {
            AppError::Internal(
                "The GitHub App isn't configured: set GITHUB_APP_ID, GITHUB_APP_INSTALLATION_ID \
                 and GITHUB_APP_PRIVATE_KEY_PATH in dashboard.env"
                    .into(),
            )
        })
    }

    /// The dashboard's public origin, for links in outgoing messages. GitHub
    /// redirects back to the dashboard, so the OAuth callback URL carries it.
    pub fn dashboard_url(&self) -> String {
        origin_of(&self.github_redirect_uri)
    }
}

fn origin_of(url: &str) -> String {
    let after_scheme = url.find("://").map(|i| i + 3).unwrap_or(0);
    match url[after_scheme..].find('/') {
        Some(i) => url[..after_scheme + i].to_string(),
        None => url.trim_end_matches('/').to_string(),
    }
}

/// The key for everything encrypted at rest, GitHub tokens included. Required:
/// without it users couldn't sign in, since their token can't be stored.
fn secrets_encryption_key() -> anyhow::Result<String> {
    let key = env::var("SECRETS_ENCRYPTION_KEY").unwrap_or_default();
    let key = key.trim();
    if key.len() < MIN_SECRETS_KEY_LEN {
        anyhow::bail!(
            "SECRETS_ENCRYPTION_KEY must be set to at least {MIN_SECRETS_KEY_LEN} characters \
             (e.g. `od -An -tx1 -N32 /dev/urandom | tr -d ' \\n'`): it encrypts stored GitHub tokens and secrets"
        );
    }
    Ok(key.to_string())
}

const MIN_SECRETS_KEY_LEN: usize = 32;

#[cfg(test)]
mod tests {
    use super::origin_of;

    #[test]
    fn origin_drops_the_path() {
        assert_eq!(
            origin_of("https://dashboard.example.com/api/auth/callback"),
            "https://dashboard.example.com"
        );
        assert_eq!(
            origin_of("http://localhost:3200/api/auth/callback"),
            "http://localhost:3200"
        );
        assert_eq!(
            origin_of("https://dashboard.example.com"),
            "https://dashboard.example.com"
        );
    }
}
