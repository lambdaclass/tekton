use std::env;

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
    pub static_dir: String,
    pub claude_bin: String,
    pub claude_config_dir: String,
    pub chromium_bin: String,
    pub claude_oauth_redirect_uri: String,
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
            preview_domain: env::var("PREVIEW_DOMAIN")
                .unwrap_or_else(|_| "example.com".into()),
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
            static_dir: env::var("STATIC_DIR").unwrap_or_else(|_| "./static".into()),
            claude_bin: env::var("CLAUDE_BIN")
                .unwrap_or_else(|_| "/run/current-system/sw/bin/claude".into()),
            claude_config_dir: env::var("CLAUDE_CONFIG_DIR")
                .unwrap_or_else(|_| "/var/secrets/claude".into()),
            chromium_bin: env::var("CHROMIUM_BIN").unwrap_or_else(|_| "chromium".into()),
            claude_oauth_redirect_uri: env::var("CLAUDE_OAUTH_REDIRECT_URI")
                .unwrap_or_else(|_| String::new()),
        })
    }
}
