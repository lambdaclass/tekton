use std::env;
use std::fs;
use std::io::{ErrorKind, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use anyhow::{bail, Context};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use rand::RngCore;

/// Where the key lives when `SECRETS_ENCRYPTION_KEY` is unset. The systemd
/// unit creates the directory; `SECRETS_ENCRYPTION_KEY_FILE` overrides the path.
const DEFAULT_SECRETS_KEY_FILE: &str = "/var/lib/dashboard/secrets-key";

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
        })
    }
}

/// The key for everything encrypted at rest. `SECRETS_ENCRYPTION_KEY` wins when
/// set; otherwise the key comes from a file, generated on first start. It never
/// goes in the DB, since protecting DB dumps is the point.
fn secrets_encryption_key() -> anyhow::Result<String> {
    if let Some(key) = env::var("SECRETS_ENCRYPTION_KEY")
        .ok()
        .filter(|k| !k.is_empty())
    {
        return Ok(key);
    }
    let path = env::var("SECRETS_ENCRYPTION_KEY_FILE")
        .ok()
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| DEFAULT_SECRETS_KEY_FILE.into());
    load_or_create_key_file(Path::new(&path))
        .with_context(|| format!("Could not load the secrets encryption key from {path}"))
}

/// Reads the key file, creating it (mode 0600, 32 random bytes in base64) when
/// it doesn't exist. The new key is written to a temp file and hard-linked into
/// place, so a concurrent starter never reads a half-written key: whoever links
/// first wins and the other reads that key.
fn load_or_create_key_file(path: &Path) -> anyhow::Result<String> {
    match fs::read_to_string(path) {
        Ok(contents) => return parse_key_file(path, &contents),
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(e) => return Err(e).with_context(|| format!("cannot read {}", path.display())),
    }

    let mut raw = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut raw);
    let key = BASE64.encode(raw);

    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let hint = "set SECRETS_ENCRYPTION_KEY_FILE to a writable path, or set SECRETS_ENCRYPTION_KEY";
    fs::create_dir_all(dir).with_context(|| format!("cannot create {}; {hint}", dir.display()))?;
    let tmp = dir.join(format!(
        ".secrets-key.{}.{}.tmp",
        std::process::id(),
        rand::thread_rng().next_u64()
    ));
    let written = (|| -> std::io::Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)?;
        file.write_all(format!("{key}\n").as_bytes())?;
        file.sync_all()?;
        fs::hard_link(&tmp, path)?;
        // Persist the new directory entry: the key must survive a power loss
        // once tokens have been encrypted with it.
        fs::File::open(dir)?.sync_all()
    })();
    let _ = fs::remove_file(&tmp);

    match written {
        Ok(()) => {
            tracing::info!(
                "Generated a new secrets encryption key at {}; back it up",
                path.display()
            );
            Ok(key)
        }
        Err(e) if e.kind() == ErrorKind::AlreadyExists => {
            let contents = fs::read_to_string(path)
                .with_context(|| format!("cannot read {}", path.display()))?;
            parse_key_file(path, &contents)
        }
        Err(e) => Err(e).with_context(|| format!("cannot create {}; {hint}", path.display())),
    }
}

/// The file holds 32 bytes in base64. Anything else means the file was
/// truncated or edited, and using it would make every stored secret unreadable.
fn parse_key_file(path: &Path, contents: &str) -> anyhow::Result<String> {
    let key = contents.trim();
    match BASE64.decode(key) {
        Ok(bytes) if bytes.len() == 32 => Ok(key.to_string()),
        _ => bail!(
            "{} is not a valid key (expected 32 bytes in base64)",
            path.display()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn creates_key_file_with_0600_and_reuses_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/secrets-key");

        let key = load_or_create_key_file(&path).unwrap();
        assert_eq!(BASE64.decode(&key).unwrap().len(), 32);
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);

        assert_eq!(load_or_create_key_file(&path).unwrap(), key);
        // No temp files left behind.
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
    }

    #[test]
    fn reads_existing_key_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets-key");
        let key = BASE64.encode([7u8; 32]);
        fs::write(&path, format!("  {key}\n")).unwrap();
        assert_eq!(load_or_create_key_file(&path).unwrap(), key);
    }

    #[test]
    fn rejects_invalid_key_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets-key");
        for bad in ["", "not base64!", &BASE64.encode([1u8; 16])] {
            fs::write(&path, bad).unwrap();
            assert!(load_or_create_key_file(&path).is_err(), "accepted {bad:?}");
            // Never overwritten.
            assert_eq!(fs::read_to_string(&path).unwrap(), bad);
        }
    }

    #[test]
    fn rejects_unreadable_key_file() {
        let dir = tempfile::tempdir().unwrap();
        // A directory where the file should be: exists but can't be read as one.
        let path = dir.path().join("secrets-key");
        fs::create_dir(&path).unwrap();
        assert!(load_or_create_key_file(&path).is_err());
    }

    #[test]
    fn concurrent_creation_agrees_on_one_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets-key");
        let keys: Vec<String> = std::thread::scope(|s| {
            let handles: Vec<_> = (0..8)
                .map(|_| s.spawn(|| load_or_create_key_file(&path).unwrap()))
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        assert!(keys.iter().all(|k| k == &keys[0]));
        assert_eq!(fs::read_to_string(&path).unwrap().trim(), keys[0]);
    }

    #[test]
    fn env_key_takes_precedence_over_file() {
        // Env-var tests share process state; keep them in this one test.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets-key");
        env::set_var("SECRETS_ENCRYPTION_KEY_FILE", &path);

        env::set_var("SECRETS_ENCRYPTION_KEY", "from-env");
        assert_eq!(secrets_encryption_key().unwrap(), "from-env");
        assert!(!path.exists(), "env key must not create the file");

        env::set_var("SECRETS_ENCRYPTION_KEY", "");
        let key = secrets_encryption_key().unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap().trim(), key);

        env::remove_var("SECRETS_ENCRYPTION_KEY");
        env::remove_var("SECRETS_ENCRYPTION_KEY_FILE");
    }
}
