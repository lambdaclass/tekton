use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::broadcast;

use crate::config::Config;
use crate::error::AppError;
use crate::models::Preview;

/// A structured action extracted from Claude's stream-json output.
#[derive(Debug, Clone)]
pub struct RawAction {
    pub action_type: String,  // "tool_use", "text", "result"
    pub tool_name: Option<String>,
    pub tool_input: Option<serde_json::Value>,
    pub summary: Option<String>,
}

/// Token usage extracted from Claude's result event.
#[derive(Debug, Clone, Default)]
pub struct TokenUsage {
    pub input_tokens: i64,
    pub output_tokens: i64,
}

/// Return value from Claude streaming: text output, structured actions, and token usage.
pub struct ClaudeStreamResult {
    pub text: String,
    pub actions: Vec<RawAction>,
    pub usage: TokenUsage,
}

/// Run a command and return its stdout.
async fn run_cmd(cmd: &str, args: &[&str]) -> Result<String, AppError> {
    run_cmd_with_env(cmd, args, &[]).await
}

/// Run a command with extra environment variables and return its stdout.
async fn run_cmd_with_env(cmd: &str, args: &[&str], env: &[(&str, &str)]) -> Result<String, AppError> {
    tracing::info!("Running: {cmd} {}", args.join(" "));
    let output = Command::new(cmd)
        .args(args)
        .envs(env.iter().copied())
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

            previews.push(Preview {
                slug,
                repo,
                branch,
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
    github_token: &str,
) -> Result<String, AppError> {
    let mut args = vec!["create", repo, branch];
    if let Some(s) = slug {
        args.push("--slug");
        args.push(s);
    }
    run_cmd_with_env(&config.preview_bin, &args, &[("GITHUB_TOKEN", github_token)]).await
}

pub async fn destroy_preview(config: &Config, slug: &str) -> Result<String, AppError> {
    run_cmd(&config.preview_bin, &["destroy", slug]).await
}

pub async fn update_preview(config: &Config, slug: &str, github_token: &str) -> Result<String, AppError> {
    run_cmd_with_env(&config.preview_bin, &["update", slug], &[("GITHUB_TOKEN", github_token)]).await
}

// ── Agent operations ──

pub async fn create_agent(config: &Config, name: &str) -> Result<String, AppError> {
    run_cmd(&config.agent_bin, &["create", name]).await
}

pub async fn destroy_agent(config: &Config, name: &str) -> Result<String, AppError> {
    run_cmd(&config.agent_bin, &["destroy", name]).await
}

/// Get the container IP for an agent from its tracking file (public for use in tasks.rs).
pub fn agent_ip_public(name: &str) -> Result<String, AppError> {
    agent_ip(name)
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

/// Copy a local file into an agent container via SCP.
pub async fn scp_to_agent(
    name: &str,
    local_path: &str,
    remote_path: &str,
) -> Result<(), AppError> {
    let ip = agent_ip(name)?;
    run_cmd(
        "scp",
        &[
            "-o", "StrictHostKeyChecking=no",
            "-o", "UserKnownHostsFile=/dev/null",
            local_path,
            &format!("agent@{ip}:{remote_path}"),
        ],
    )
    .await?;
    Ok(())
}

/// Run a command inside an agent container via SSH and capture stdout.
pub async fn agent_exec_capture(name: &str, cmd_line: &str) -> Result<String, AppError> {
    let ip = agent_ip(name)?;
    run_cmd(
        "ssh",
        &[
            "-o", "StrictHostKeyChecking=no",
            "-o", "UserKnownHostsFile=/dev/null",
            &format!("agent@{ip}"),
            cmd_line,
        ],
    )
    .await
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

/// Run Claude in an agent container with stream-json output, parsing events into readable log lines.
/// Returns Claude's accumulated text output, structured actions, and token usage.
pub async fn agent_exec_claude_streaming(
    name: &str,
    cmd_line: &str,
    tx: broadcast::Sender<String>,
) -> Result<ClaudeStreamResult, AppError> {
    let ip = agent_ip(name)?;

    tracing::info!("Streaming Claude JSON: ssh agent@{ip}");
    let mut child = Command::new("ssh")
        .args([
            "-o", "StrictHostKeyChecking=no",
            "-o", "UserKnownHostsFile=/dev/null",
            &format!("agent@{ip}"),
            cmd_line,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| AppError::Internal(format!("Failed to spawn ssh for Claude streaming: {e}")))?;

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let claude_text = Arc::new(tokio::sync::Mutex::new(String::new()));
    let claude_text_clone = claude_text.clone();

    let actions = Arc::new(tokio::sync::Mutex::new(Vec::<RawAction>::new()));
    let actions_clone = actions.clone();

    let usage = Arc::new(tokio::sync::Mutex::new(TokenUsage::default()));
    let usage_clone = usage.clone();

    let tx2 = tx.clone();
    let stdout_handle = tokio::spawn(async move {
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            // Capture Claude's text responses
            if let Some(text) = extract_claude_text(&line) {
                let mut buf = claude_text_clone.lock().await;
                if !buf.is_empty() {
                    buf.push('\n');
                }
                buf.push_str(&text);
            }
            // Extract structured actions
            if let Some(action) = extract_action(&line) {
                actions_clone.lock().await.push(action);
            }
            // Extract token usage from result events
            if let Some(u) = extract_token_usage(&line) {
                let mut usage = usage_clone.lock().await;
                usage.input_tokens += u.input_tokens;
                usage.output_tokens += u.output_tokens;
            }
            if let Some(formatted) = format_claude_event(&line) {
                let _ = tx.send(formatted);
            }
        }
    });

    let stderr_handle = tokio::spawn(async move {
        let reader = BufReader::new(stderr);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            // Skip noisy SSH warnings
            if line.contains("Warning: Permanently added") {
                continue;
            }
            let _ = tx2.send(line);
        }
    });

    let _ = stdout_handle.await;
    let _ = stderr_handle.await;

    let status = child
        .wait()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to wait on ssh: {e}")))?;

    if !status.success() {
        return Err(AppError::Internal(format!(
            "Claude streaming exited with status {status}"
        )));
    }

    let text = claude_text.lock().await.clone();
    let actions = actions.lock().await.clone();
    let usage = usage.lock().await.clone();

    Ok(ClaudeStreamResult { text, actions, usage })
}

/// Extract raw text from a Claude stream-json "assistant" message.
/// Returns the concatenated text blocks (if any) for saving as a chat message.
fn extract_claude_text(line: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    if v.get("type")?.as_str()? != "assistant" {
        return None;
    }
    let content = v.pointer("/message/content")?.as_array()?;
    let mut texts = Vec::new();
    for block in content {
        if block.get("type").and_then(|t| t.as_str()) == Some("text") {
            if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    texts.push(trimmed.to_string());
                }
            }
        }
    }
    if texts.is_empty() {
        None
    } else {
        Some(texts.join("\n"))
    }
}

/// Extract a structured action from a stream-json line.
fn extract_action(line: &str) -> Option<RawAction> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    let msg_type = v.get("type")?.as_str()?;

    match msg_type {
        "assistant" => {
            let content = v.pointer("/message/content")?;
            if let Some(arr) = content.as_array() {
                for block in arr {
                    if let Some(block_type) = block.get("type").and_then(|t| t.as_str()) {
                        match block_type {
                            "tool_use" => {
                                let name = block.get("name").and_then(|n| n.as_str()).map(String::from);
                                let input = block.get("input").cloned();
                                let summary = format_tool_use_summary(block);
                                return Some(RawAction {
                                    action_type: "tool_use".into(),
                                    tool_name: name,
                                    tool_input: input,
                                    summary,
                                });
                            }
                            "text" => {
                                if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                    if !text.trim().is_empty() {
                                        let truncated: String = text.chars().take(200).collect();
                                        return Some(RawAction {
                                            action_type: "text".into(),
                                            tool_name: None,
                                            tool_input: None,
                                            summary: Some(truncated),
                                        });
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            None
        }
        "result" => {
            Some(RawAction {
                action_type: "result".into(),
                tool_name: None,
                tool_input: None,
                summary: Some("Claude finished".into()),
            })
        }
        _ => None,
    }
}

/// Extract token usage from a Claude stream-json "result" event.
fn extract_token_usage(line: &str) -> Option<TokenUsage> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    if v.get("type")?.as_str()? != "result" {
        return None;
    }
    let input = v.pointer("/usage/input_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
    let output = v.pointer("/usage/output_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
    if input == 0 && output == 0 {
        return None;
    }
    Some(TokenUsage {
        input_tokens: input,
        output_tokens: output,
    })
}

/// Generate a human-readable summary for a tool_use block.
fn format_tool_use_summary(block: &serde_json::Value) -> Option<String> {
    let name = block.get("name")?.as_str()?;
    let input = block.get("input").unwrap_or(&serde_json::Value::Null);

    let summary = match name {
        "Read" => {
            let path = input.get("file_path").and_then(|p| p.as_str()).unwrap_or("...");
            format!("Reading {path}")
        }
        "Edit" => {
            let path = input.get("file_path").and_then(|p| p.as_str()).unwrap_or("...");
            format!("Editing {path}")
        }
        "Write" => {
            let path = input.get("file_path").and_then(|p| p.as_str()).unwrap_or("...");
            format!("Writing {path}")
        }
        "Grep" => {
            let pattern = input.get("pattern").and_then(|p| p.as_str()).unwrap_or("...");
            format!("Searching for \"{pattern}\"")
        }
        "Glob" => {
            let pattern = input.get("pattern").and_then(|p| p.as_str()).unwrap_or("...");
            format!("Globbing \"{pattern}\"")
        }
        "Bash" => {
            let cmd = input.get("command").and_then(|c| c.as_str()).unwrap_or("...");
            let truncated: String = cmd.chars().take(100).collect();
            format!("Running: {truncated}")
        }
        "Task" => "Spawning sub-agent".to_string(),
        _ => format!("Using tool: {name}"),
    };
    Some(summary)
}

/// Parse a stream-json line from Claude and format it as a human-readable log line.
fn format_claude_event(line: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;

    let msg_type = v.get("type")?.as_str()?;

    match msg_type {
        "assistant" => {
            // Look inside message.content for tool_use or text blocks
            let content = v.pointer("/message/content")?;
            if let Some(arr) = content.as_array() {
                for block in arr {
                    if let Some(block_type) = block.get("type").and_then(|t| t.as_str()) {
                        match block_type {
                            "tool_use" => return format_tool_use(block),
                            "text" => {
                                if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                    if text.trim().is_empty() {
                                        return None;
                                    }
                                    let truncated: String = text.chars().take(200).collect();
                                    return Some(format!("💬 {truncated}"));
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            None
        }
        "content_block_start" => {
            let block = v.get("content_block")?;
            if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                return format_tool_use(block);
            }
            None
        }
        "result" => Some("✅ Claude finished".to_string()),
        _ => None,
    }
}

fn format_tool_use(block: &serde_json::Value) -> Option<String> {
    let name = block.get("name")?.as_str()?;
    let input = block.get("input").unwrap_or(&serde_json::Value::Null);

    match name {
        "Read" => {
            let path = input.get("file_path").and_then(|p| p.as_str()).unwrap_or("...");
            Some(format!("⚡ Reading {path}"))
        }
        "Edit" => {
            let path = input.get("file_path").and_then(|p| p.as_str()).unwrap_or("...");
            Some(format!("✏️ Editing {path}"))
        }
        "Write" => {
            let path = input.get("file_path").and_then(|p| p.as_str()).unwrap_or("...");
            Some(format!("✏️ Writing {path}"))
        }
        "Grep" => {
            let pattern = input.get("pattern").and_then(|p| p.as_str()).unwrap_or("...");
            Some(format!("🔍 Searching for \"{pattern}\""))
        }
        "Glob" => {
            let pattern = input.get("pattern").and_then(|p| p.as_str()).unwrap_or("...");
            Some(format!("🔍 Globbing \"{pattern}\""))
        }
        "Bash" => {
            let cmd = input.get("command").and_then(|c| c.as_str()).unwrap_or("...");
            let truncated: String = cmd.chars().take(100).collect();
            Some(format!("🖥️ Running: {truncated}"))
        }
        "Task" => Some("🚀 Spawning sub-agent...".to_string()),
        _ => Some(format!("🔧 Using tool: {name}")),
    }
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
123                 running         feat/foo                        https://123.example.com
456                 stopped         main                            https://456.example.com
";
        let result = parse_preview_list(output, "example.com").unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].slug, "123");
        assert_eq!(result[0].branch, "feat/foo");
        assert_eq!(result[0].url, "https://123.example.com");
        assert_eq!(result[1].slug, "456");
    }

    #[test]
    fn test_parse_empty_preview_list() {
        let output = "No active previews.\n";
        let result = parse_preview_list(output, "example.com").unwrap();
        assert_eq!(result.len(), 0);
    }
}
