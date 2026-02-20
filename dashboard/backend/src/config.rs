use std::env;

#[derive(Clone)]
pub struct Config {
    pub listen_addr: String,
    pub database_url: String,
    pub jwt_secret: String,
    pub google_client_id: String,
    pub google_client_secret: String,
    pub google_redirect_uri: String,
    pub allowed_domain: String,
    pub preview_domain: String,
    pub allowed_repos: Vec<String>,
    pub vertex_repos: Vec<String>,
    pub preview_bin: String,
    pub agent_bin: String,
    pub static_dir: String,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            listen_addr: env::var("LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:3200".into()),
            database_url: env::var("DATABASE_URL")
                .unwrap_or_else(|_| "sqlite:///var/lib/dashboard/dashboard.db".into()),
            jwt_secret: env::var("JWT_SECRET")?,
            google_client_id: env::var("GOOGLE_CLIENT_ID")?,
            google_client_secret: env::var("GOOGLE_CLIENT_SECRET")?,
            google_redirect_uri: env::var("GOOGLE_REDIRECT_URI")?,
            allowed_domain: env::var("ALLOWED_DOMAIN")
                .unwrap_or_else(|_| "lambdaclass.com".into()),
            preview_domain: env::var("PREVIEW_DOMAIN")
                .unwrap_or_else(|_| "hipermegared.link".into()),
            allowed_repos: env::var("ALLOWED_REPOS")
                .unwrap_or_default()
                .split(',')
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect(),
            vertex_repos: env::var("VERTEX_REPOS")
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
        })
    }
}
