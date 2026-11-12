//! Host-side bare mirrors of task repos.
//!
//! Agent containers run Claude with passwordless sudo, so nothing placed inside one can be
//! kept from it. The GitHub token therefore never enters a container: the backend fetches
//! from GitHub into a root-owned bare mirror on the host, then pushes the commits into the
//! agent's repo over the SSH connection it already uses. Changes go back to GitHub through
//! `createCommitOnBranch`, also from the backend.

use std::path::PathBuf;
use std::sync::{Arc, LazyLock};

use base64::Engine;
use dashmap::DashMap;
use tokio::process::Command;
use tokio::sync::Mutex;

use crate::config::Config;
use crate::error::AppError;
use crate::shell;

/// One lock per mirror: concurrent fetches into the same bare repo fail on ref locks.
static LOCKS: LazyLock<DashMap<String, Arc<Mutex<()>>>> = LazyLock::new(DashMap::new);

/// The agent's `origin`: GitHub without credentials, so tools see the right remote but
/// can't push (or fetch a private repo).
pub fn public_url(repo: &str) -> String {
    format!("https://github.com/{repo}.git")
}

fn mirror_path(config: &Config, repo: &str) -> Result<PathBuf, AppError> {
    let valid_part = |s: &str| {
        !s.is_empty()
            && !s.starts_with('.')
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    match repo.split_once('/') {
        Some((owner, name)) if valid_part(owner) && valid_part(name) => {
            Ok(PathBuf::from(&config.mirror_dir)
                .join(owner)
                .join(format!("{name}.git")))
        }
        _ => Err(AppError::BadRequest(format!(
            "Invalid repository name '{repo}'"
        ))),
    }
}

fn validate_branch(branch: &str) -> Result<(), AppError> {
    let ok = !branch.is_empty()
        && !branch.starts_with('-')
        && !branch.contains("..")
        && !branch.chars().any(|c| {
            c.is_whitespace()
                || c.is_control()
                || matches!(c, ':' | '~' | '^' | '?' | '*' | '[' | '\\')
        });
    if ok {
        Ok(())
    } else {
        Err(AppError::BadRequest(format!(
            "Invalid branch name '{branch}'"
        )))
    }
}

/// Runs git with `args`, failing with its stderr. `env` is how the token is passed, so it
/// never shows up in argv, logs or on disk.
async fn git(args: &[&str], env: &[(&str, String)]) -> Result<String, AppError> {
    tracing::info!("Running: git {}", args.join(" "));
    let output = Command::new("git")
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .envs(env.iter().map(|(k, v)| (*k, v.as_str())))
        .output()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to execute git: {e}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::Internal(format!(
            "git {} failed: {}",
            args.first().unwrap_or(&""),
            stderr.trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn auth_env(token: &str) -> Vec<(&'static str, String)> {
    let basic = base64::engine::general_purpose::STANDARD.encode(format!("x-access-token:{token}"));
    vec![
        ("GIT_CONFIG_COUNT", "1".into()),
        (
            "GIT_CONFIG_KEY_0",
            "http.https://github.com/.extraHeader".into(),
        ),
        (
            "GIT_CONFIG_VALUE_0",
            format!("Authorization: Basic {basic}"),
        ),
    ]
}

/// Fetches `branches` from GitHub into the repo's mirror (creating it if needed) and returns
/// their head commits, in order.
pub async fn fetch(
    config: &Config,
    repo: &str,
    token: &str,
    branches: &[&str],
) -> Result<Vec<String>, AppError> {
    for b in branches {
        validate_branch(b)?;
    }
    let path = mirror_path(config, repo)?;
    let path_str = path.to_string_lossy().to_string();
    let lock = LOCKS.entry(path_str.clone()).or_default().clone();
    let _guard = lock.lock().await;

    if !path.join("HEAD").exists() {
        tokio::fs::create_dir_all(&path)
            .await
            .map_err(|e| AppError::Internal(format!("Cannot create mirror {path_str}: {e}")))?;
        git(&["init", "--quiet", "--bare", &path_str], &[]).await?;
    }

    let url = public_url(repo);
    let refspecs: Vec<String> = branches
        .iter()
        .map(|b| format!("+refs/heads/{b}:refs/heads/{b}"))
        .collect();
    let mut args = vec!["-C", &path_str, "fetch", "--quiet", "--no-tags", "--", &url];
    args.extend(refspecs.iter().map(String::as_str));
    git(&args, &auth_env(token)).await?;

    let mut oids = Vec::with_capacity(branches.len());
    for b in branches {
        let rev = format!("refs/heads/{b}");
        let oid = git(
            &["-C", &path_str, "rev-parse", "--verify", "--quiet", &rev],
            &[],
        )
        .await?;
        oids.push(oid.trim().to_string());
    }
    Ok(oids)
}

/// Pushes `branches` from the mirror into the agent's repo as `refs/remotes/origin/<b>`,
/// the refs a normal fetch from `origin` would have written. The agent's repo must exist.
pub async fn push_to_agent(
    config: &Config,
    repo: &str,
    agent_name: &str,
    branches: &[&str],
) -> Result<(), AppError> {
    for b in branches {
        validate_branch(b)?;
    }
    let path = mirror_path(config, repo)?;
    let path_str = path.to_string_lossy().to_string();
    let ip = shell::agent_ip_public(agent_name)?;
    let target = format!("agent@{ip}:/home/agent/repo");
    let refspecs: Vec<String> = branches
        .iter()
        .map(|b| format!("+refs/heads/{b}:refs/remotes/origin/{b}"))
        .collect();
    let mut args = vec![
        "-C",
        &path_str,
        "push",
        "--quiet",
        "--no-verify",
        "--",
        &target,
    ];
    args.extend(refspecs.iter().map(String::as_str));
    git(
        &args,
        &[(
            "GIT_SSH_COMMAND",
            "ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null".into(),
        )],
    )
    .await?;
    Ok(())
}

/// [`fetch`] then [`push_to_agent`]: brings the agent's `origin/<b>` refs up to date with
/// GitHub and returns their head commits.
pub async fn sync_to_agent(
    config: &Config,
    repo: &str,
    token: &str,
    agent_name: &str,
    branches: &[&str],
) -> Result<Vec<String>, AppError> {
    let oids = fetch(config, repo, token, branches).await?;
    push_to_agent(config, repo, agent_name, branches).await?;
    Ok(oids)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_validation() {
        for ok in ["main", "feat/x-1", "release/1.2", "a_b"] {
            assert!(validate_branch(ok).is_ok(), "{ok}");
        }
        for bad in [
            "", "-x", "a..b", "a b", "a:b", "a~1", "x^", "*", "a\\b", "a\nb",
        ] {
            assert!(validate_branch(bad).is_err(), "{bad:?}");
        }
    }
}
