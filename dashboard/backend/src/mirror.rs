//! Host-side bare mirrors of task repos.
//!
//! Agent containers run Claude with passwordless sudo, so nothing placed inside one can be
//! kept from it. The GitHub token therefore never enters a container: the backend fetches
//! from GitHub into a root-owned bare mirror on the host, then pushes the commits into the
//! agent's repo over the SSH connection it already uses. Changes go back to GitHub through
//! `createCommitOnBranch`, also from the backend.

use std::path::PathBuf;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

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

/// Upper bound on one git command. A fetch runs while holding the mirror's lock, so a stalled
/// one would otherwise block every task on that repo until the backend restarts.
const GIT_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// Runs git with `args`, failing with its stderr. `env` is how the token is passed, so it
/// never shows up in argv, logs or on disk.
async fn git(args: &[&str], env: &[(&str, String)]) -> Result<String, AppError> {
    let output = git_output(args, env).await?;
    if !output.status.success() {
        return Err(git_failed(args, &output));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Runs git with `args` and returns its output whatever the exit status, killing it after
/// [`GIT_TIMEOUT`].
async fn git_output(
    args: &[&str],
    env: &[(&str, String)],
) -> Result<std::process::Output, AppError> {
    tracing::info!("Running: git {}", args.join(" "));
    let child = Command::new("git")
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        // Stable English messages, which `fetch_if_exists` matches on.
        .env("LC_ALL", "C")
        // Abort a transfer that stays under 1 KB/s for a minute, so a stalled HTTPS helper
        // exits by itself instead of outliving the timeout below.
        .env("GIT_HTTP_LOW_SPEED_LIMIT", "1000")
        .env("GIT_HTTP_LOW_SPEED_TIME", "60")
        .envs(env.iter().map(|(k, v)| (*k, v.as_str())))
        .kill_on_drop(true)
        .output();
    match tokio::time::timeout(GIT_TIMEOUT, child).await {
        Ok(output) => output.map_err(|e| AppError::Internal(format!("Failed to execute git: {e}"))),
        Err(_) => Err(AppError::Internal(format!(
            "git {} timed out after {}s",
            args.first().unwrap_or(&""),
            GIT_TIMEOUT.as_secs()
        ))),
    }
}

fn git_failed(args: &[&str], output: &std::process::Output) -> AppError {
    let stderr = String::from_utf8_lossy(&output.stderr);
    AppError::Internal(format!(
        "git {} failed: {}",
        args.first().unwrap_or(&""),
        stderr.trim()
    ))
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
    fetch_if_exists(config, repo, token, branches)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "Branch {} not found in {repo}",
                branches.join(", ")
            ))
        })
}

/// Like [`fetch`], but returns `None` when one of `branches` doesn't exist on GitHub. Any
/// other failure (network, token, timeout) is still an error.
async fn fetch_if_exists(
    config: &Config,
    repo: &str,
    token: &str,
    branches: &[&str],
) -> Result<Option<Vec<String>>, AppError> {
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
    let output = git_output(&args, &auth_env(token)).await?;
    if !output.status.success() {
        if is_missing_ref(&String::from_utf8_lossy(&output.stderr)) {
            return Ok(None);
        }
        return Err(git_failed(&args, &output));
    }

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
    Ok(Some(oids))
}

/// Whether a failed fetch's stderr says a requested branch isn't on the remote.
fn is_missing_ref(stderr: &str) -> bool {
    stderr.contains("couldn't find remote ref")
}

/// Pushes each `(branch, commit)` from the mirror into the agent's repo as
/// `refs/remotes/origin/<branch>`, the refs a normal fetch from `origin` would have written.
/// Commits rather than branch names, so a later fetch into the shared mirror can't change
/// what this agent receives. The agent's repo must exist.
async fn push_to_agent(
    config: &Config,
    repo: &str,
    agent_name: &str,
    heads: &[(&str, &str)],
) -> Result<(), AppError> {
    for (b, oid) in heads {
        validate_branch(b)?;
        if oid.len() < 40 || !oid.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(AppError::Internal(format!("Invalid commit id '{oid}'")));
        }
    }
    let path = mirror_path(config, repo)?;
    let path_str = path.to_string_lossy().to_string();
    let ip = shell::agent_ip_public(agent_name)?;
    let target = format!("agent@{ip}:/home/agent/repo");
    let refspecs: Vec<String> = heads
        .iter()
        .map(|(b, oid)| format!("+{oid}:refs/remotes/origin/{b}"))
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
            "ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
             -o ConnectTimeout=30 -o ServerAliveInterval=15 -o ServerAliveCountMax=4"
                .into(),
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
    push_heads(config, repo, agent_name, branches, &oids).await?;
    Ok(oids)
}

/// [`sync_to_agent`] for one branch that may not exist on GitHub yet: `None` if it doesn't.
pub async fn sync_branch_if_exists(
    config: &Config,
    repo: &str,
    token: &str,
    agent_name: &str,
    branch: &str,
) -> Result<Option<String>, AppError> {
    let Some(mut oids) = fetch_if_exists(config, repo, token, &[branch]).await? else {
        return Ok(None);
    };
    push_heads(config, repo, agent_name, &[branch], &oids).await?;
    Ok(oids.pop())
}

async fn push_heads(
    config: &Config,
    repo: &str,
    agent_name: &str,
    branches: &[&str],
    oids: &[String],
) -> Result<(), AppError> {
    let heads: Vec<(&str, &str)> = branches
        .iter()
        .copied()
        .zip(oids.iter().map(String::as_str))
        .collect();
    push_to_agent(config, repo, agent_name, &heads).await
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

    #[test]
    fn missing_ref_detection() {
        assert!(is_missing_ref(
            "fatal: couldn't find remote ref refs/heads/tekton/task-1\n"
        ));
        assert!(!is_missing_ref(
            "fatal: unable to access 'https://github.com/o/r.git/': Could not resolve host\n"
        ));
        assert!(!is_missing_ref(
            "remote: Invalid username or token.\nfatal: Authentication failed\n"
        ));
    }
}
