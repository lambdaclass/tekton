use std::env;

use anyhow::bail;

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
    pub secrets_encryption_key: String,
    pub intake_enabled: bool,
    pub intake_max_global_concurrent: i32,
    pub github_webhook_secret: String,
}

/// Read from the environment, then removed from it so child processes
/// (agent, preview, git, ssh, chromium) never inherit them.
const SECRET_VARS: [&str; 3] = [
    "SECRETS_ENCRYPTION_KEY",
    "JWT_SECRET",
    "GITHUB_CLIENT_SECRET",
];

impl Config {
    /// Must run before any other thread starts: it removes `SECRET_VARS` from
    /// the process environment.
    pub fn from_env() -> anyhow::Result<Self> {
        let config = Self::read_env()?;
        scrub_secret_env();
        Ok(config)
    }

    fn read_env() -> anyhow::Result<Self> {
        Ok(Self {
            listen_addr: env::var("LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:3200".into()),
            database_url: env::var("DATABASE_URL")
                .unwrap_or_else(|_| "sqlite:///var/lib/dashboard/dashboard.db".into()),
            jwt_secret: required_secret("JWT_SECRET")?,
            github_client_id: env::var("GITHUB_CLIENT_ID")?,
            github_client_secret: required_secret("GITHUB_CLIENT_SECRET")?,
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
            static_dir: env::var("STATIC_DIR").unwrap_or_else(|_| "./static".into()),
            claude_bin: env::var("CLAUDE_BIN")
                .unwrap_or_else(|_| "/run/current-system/sw/bin/claude".into()),
            claude_config_dir: env::var("CLAUDE_CONFIG_DIR")
                .unwrap_or_else(|_| "/var/secrets/claude".into()),
            chromium_bin: env::var("CHROMIUM_BIN").unwrap_or_else(|_| "chromium".into()),
            secrets_encryption_key: required_secret("SECRETS_ENCRYPTION_KEY")?,
            intake_enabled: env::var("INTAKE_ENABLED").unwrap_or_else(|_| "false".into()) == "true",
            intake_max_global_concurrent: env::var("INTAKE_MAX_GLOBAL_CONCURRENT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(10),
            github_webhook_secret: env::var("GITHUB_WEBHOOK_SECRET")
                .unwrap_or_else(|_| String::new()),
        })
    }
}

/// The secrets that must stay off persistent disk come from
/// `/run/dashboard-secrets/secrets.env`, pushed from 1Password; see
/// docs/deployment-guide.md.
fn required_secret(name: &str) -> anyhow::Result<String> {
    match env::var(name) {
        Ok(v) if !v.trim().is_empty() => Ok(v),
        _ => bail!(
            "{name} is not set. On the server it comes from \
             /run/dashboard-secrets/secrets.env (pushed from 1Password; see the \
             deployment guide); locally, add it to dashboard/backend/.env"
        ),
    }
}

fn scrub_secret_env() {
    for name in SECRET_VARS {
        env::remove_var(name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_secret_rejects_missing_and_empty() {
        let name = "TEKTON_TEST_REQUIRED_SECRET";
        env::remove_var(name);
        assert!(required_secret(name).is_err());
        env::set_var(name, "  ");
        assert!(required_secret(name).is_err());
        env::set_var(name, "value");
        assert_eq!(required_secret(name).unwrap(), "value");
        env::remove_var(name);
    }

    #[test]
    fn children_do_not_inherit_secrets() {
        for name in SECRET_VARS {
            env::set_var(name, "leak-check");
        }
        scrub_secret_env();
        let out = std::process::Command::new("env").output().unwrap();
        let child_env = String::from_utf8_lossy(&out.stdout);
        for name in SECRET_VARS {
            assert!(!child_env.contains(&format!("{name}=")), "{name} leaked");
        }
    }
}
