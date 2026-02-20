use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::broadcast;

use crate::config::Config;
use crate::error::AppError;
use crate::models::Preview;

/// Run a command and return its stdout.
async fn run_cmd(cmd: &str, args: &[&str]) -> Result<String, AppError> {
    tracing::info!("Running: {cmd} {}", args.join(" "));
    let output = Command::new(cmd)
        .args(args)
        .output()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to execute {cmd}: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::error!("{cmd} failed: {stderr}");
        return Err(AppError::Internal(format!("{cmd} failed: {stderr}")));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Run a command and stream stdout lines to a broadcast channel.
/// Returns when the command exits.
pub async fn run_cmd_streaming(
    cmd: &str,
    args: &[&str],
    tx: broadcast::Sender<String>,
) -> Result<(), AppError> {
    tracing::info!("Streaming: {cmd} {}", args.join(" "));
    let mut child = Command::new(cmd)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| AppError::Internal(format!("Failed to spawn {cmd}: {e}")))?;

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let tx2 = tx.clone();
    let stdout_handle = tokio::spawn(async move {
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let _ = tx.send(line);
        }
    });

    let stderr_handle = tokio::spawn(async move {
        let reader = BufReader::new(stderr);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let _ = tx2.send(line);
        }
    });

    let _ = stdout_handle.await;
    let _ = stderr_handle.await;

    let status = child
        .wait()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to wait on {cmd}: {e}")))?;

    if !status.success() {
        return Err(AppError::Internal(format!(
            "{cmd} exited with status {status}"
        )));
    }
    Ok(())
}

// ── Preview operations ──

/// Strip ANSI escape sequences from a string.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_escape = false;
    for c in s.chars() {
        if in_escape {
            if c.is_ascii_alphabetic() {
                in_escape = false;
            }
            continue;
        }
        if c == '\x1b' {
            in_escape = true;
            continue;
        }
        out.push(c);
    }
    out
}

pub async fn list_previews(config: &Config) -> Result<Vec<Preview>, AppError> {
    let output = run_cmd(&config.preview_bin, &["list"]).await?;
    let clean = strip_ansi(&output);
    parse_preview_list(&clean, &config.preview_domain)
}

fn parse_preview_list(output: &str, _domain: &str) -> Result<Vec<Preview>, AppError> {
    let mut previews = Vec::new();
    let preview_dir = "/var/lib/preview-deploys";

    for line in output.lines() {
        let line = line.trim();
        if line.is_empty()
            || line.starts_with("SLUG")
            || line.starts_with("No active")
            || line.starts_with("---")
        {
            continue;
        }

        // Format: "SLUG                STATUS          BRANCH                          URL"
        // Columns are fixed-width, but split_whitespace works since values have no spaces
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 4 {
            let slug = parts[0].to_string();
            // parts[1] = status (running/stopped), parts[2] = branch, parts[3] = url
            let branch = parts[2].to_string();
            let url = parts[3].to_string();

            // Read repo and type from tracking files
            let repo = std::fs::read_to_string(format!("{preview_dir}/{slug}"))
                .ok()
                .and_then(|content| {
                    // Format: "slot host_ip local_ip repo branch"
                    content.split_whitespace().nth(3).map(String::from)
                })
                .unwrap_or_default();

            let preview_type = std::fs::read_to_string(format!("{preview_dir}/{slug}.type"))
                .unwrap_or_else(|_| "node".into())
                .trim()
                .to_string();

            previews.push(Preview {
                slug,
                repo,
                branch,
                preview_type,
                url,
            });
        }
    }

    Ok(previews)
}

pub async fn create_preview(
    config: &Config,
    repo: &str,
    branch: &str,
    slug: Option<&str>,
    preview_type: &str,
) -> Result<String, AppError> {
    let mut args = vec!["create", repo, branch, "--type", preview_type];
    if let Some(s) = slug {
        args.push("--slug");
        args.push(s);
    }
    run_cmd(&config.preview_bin, &args).await
}

pub async fn destroy_preview(config: &Config, slug: &str) -> Result<String, AppError> {
    run_cmd(&config.preview_bin, &["destroy", slug]).await
}

pub async fn update_preview(config: &Config, slug: &str) -> Result<String, AppError> {
    run_cmd(&config.preview_bin, &["update", slug]).await
}

// ── Agent operations ──

pub async fn create_agent(config: &Config, name: &str) -> Result<String, AppError> {
    run_cmd(&config.agent_bin, &["create", name]).await
}

pub async fn destroy_agent(config: &Config, name: &str) -> Result<String, AppError> {
    run_cmd(&config.agent_bin, &["destroy", name]).await
}

/// Get the container IP for an agent from its tracking file.
fn agent_ip(name: &str) -> Result<String, AppError> {
    let path = format!("/var/lib/claude-agents/{name}");
    let content = std::fs::read_to_string(&path)
        .map_err(|e| AppError::Internal(format!("Cannot read agent tracking file {path}: {e}")))?;
    // Format: "slot host_ip container_ip"
    content
        .split_whitespace()
        .nth(2)
        .map(String::from)
        .ok_or_else(|| AppError::Internal(format!("Bad format in agent tracking file {path}")))
}

/// Run a command inside an agent container via SSH.
pub async fn agent_exec(
    name: &str,
    cmd_line: &str,
    tx: broadcast::Sender<String>,
) -> Result<(), AppError> {
    let ip = agent_ip(name)?;
    run_cmd_streaming(
        "ssh",
        &[
            "-o", "StrictHostKeyChecking=no",
            "-o", "UserKnownHostsFile=/dev/null",
            &format!("agent@{ip}"),
            cmd_line,
        ],
        tx,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_ansi() {
        let input = "\x1b[1mSLUG\x1b[0m  \x1b[0;32mrunning\x1b[0m";
        assert_eq!(strip_ansi(input), "SLUG  running");
    }

    #[test]
    fn test_parse_preview_list() {
        let output = "\
SLUG                STATUS          BRANCH                          URL
123                 running         feat/foo                        https://123.hipermegared.link
456                 stopped         main                            https://456.hipermegared.link
";
        let result = parse_preview_list(output, "hipermegared.link").unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].slug, "123");
        assert_eq!(result[0].branch, "feat/foo");
        assert_eq!(result[0].url, "https://123.hipermegared.link");
        assert_eq!(result[1].slug, "456");
    }

    #[test]
    fn test_parse_empty_preview_list() {
        let output = "No active previews.\n";
        let result = parse_preview_list(output, "hipermegared.link").unwrap();
        assert_eq!(result.len(), 0);
    }
}
