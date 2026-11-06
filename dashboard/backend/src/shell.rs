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
    pub action_type: String, // "tool_use", "text", "result"
    pub tool_name: Option<String>,
    pub tool_input: Option<serde_json::Value>,
    pub summary: Option<String>,
}

/// Token usage extracted from Claude's result event.
#[derive(Debug, Clone, Default)]
pub struct TokenUsage {
    pub input_tokens: i64,
    pub output_tokens: i64,
    /// Cost reported by the Claude CLI (from the `result` event's `total_cost_usd` field).
    pub cost_usd: f64,
}

/// Return value from Claude streaming: text output, structured actions, and token usage.
#[derive(Debug, Default)]
pub struct ClaudeStreamResult {
    pub text: String,
    pub actions: Vec<RawAction>,
    pub usage: TokenUsage,
}

/// A failed Claude invocation, with whatever output was streamed before it failed.
#[derive(Debug)]
pub struct ClaudeStreamError {
    pub error: AppError,
    pub partial: ClaudeStreamResult,
}

pub type ClaudeRun = Result<ClaudeStreamResult, ClaudeStreamError>;

impl ClaudeStreamError {
    /// An invocation that failed before Claude produced any output.
    pub fn before_start(error: AppError) -> Self {
        Self {
            error,
            partial: ClaudeStreamResult::default(),
        }
    }
}

/// Accumulates token usage from a Claude stream-json run.
///
/// The `result` event carries the final usage and cost. If the run dies before emitting one,
/// fall back to summing per-message usage from `assistant` events. Those carry no cost, and we
/// only record cost the CLI reports, so partial usage has a cost of 0.
#[derive(Debug, Default)]
pub struct UsageAccumulator {
    result: Option<TokenUsage>,
    /// (input, output) per assistant message id. stream-json repeats a message's usage on
    /// every content block, so keep the max per id rather than summing every event.
    messages: std::collections::HashMap<String, (i64, i64)>,
}

impl UsageAccumulator {
    pub fn feed(&mut self, line: &str) {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            return;
        };
        match v.get("type").and_then(|t| t.as_str()) {
            Some("result") => {
                if let Some(u) = extract_token_usage(line) {
                    let total = self.result.get_or_insert_with(TokenUsage::default);
                    total.input_tokens += u.input_tokens;
                    total.output_tokens += u.output_tokens;
                    total.cost_usd += u.cost_usd;
                }
            }
            Some("assistant") => {
                let Some(id) = v.pointer("/message/id").and_then(|i| i.as_str()) else {
                    return;
                };
                let input = v
                    .pointer("/message/usage/input_tokens")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                let output = v
                    .pointer("/message/usage/output_tokens")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                let entry = self.messages.entry(id.to_string()).or_default();
                entry.0 = entry.0.max(input);
                entry.1 = entry.1.max(output);
            }
            _ => {}
        }
    }

    pub fn finish(&self) -> TokenUsage {
        if let Some(result) = &self.result {
            return result.clone();
        }
        let (input_tokens, output_tokens) = self
            .messages
            .values()
            .fold((0, 0), |(i, o), (mi, mo)| (i + mi, o + mo));
        TokenUsage {
            input_tokens,
            output_tokens,
            cost_usd: 0.0,
        }
    }
}

/// Run a command and return its stdout.
async fn run_cmd(cmd: &str, args: &[&str]) -> Result<String, AppError> {
    run_cmd_with_env(cmd, args, &[]).await
}

/// Run a command with extra environment variables and return its stdout.
async fn run_cmd_with_env(
    cmd: &str,
    args: &[&str],
    env: &[(&str, &str)],
) -> Result<String, AppError> {
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
    run_cmd_with_env(
        &config.preview_bin,
        &args,
        &[("GITHUB_TOKEN", github_token)],
    )
    .await
}

pub async fn destroy_preview(config: &Config, slug: &str) -> Result<String, AppError> {
    run_cmd(&config.preview_bin, &["destroy", slug]).await
}

pub async fn update_preview(
    config: &Config,
    slug: &str,
    github_token: &str,
) -> Result<String, AppError> {
    run_cmd_with_env(
        &config.preview_bin,
        &["update", slug],
        &[("GITHUB_TOKEN", github_token)],
    )
    .await
}

// ── Agent operations ──

pub async fn create_agent(config: &Config, name: &str) -> Result<String, AppError> {
    run_cmd(&config.agent_bin, &["create", name]).await
}

pub async fn destroy_agent(config: &Config, name: &str) -> Result<String, AppError> {
    // Clean up any egress rules before destroying the container
    let _ = remove_egress_rules(name).await;
    run_cmd(&config.agent_bin, &["destroy", name]).await
}

/// Apply network egress restrictions to an agent container using iptables FORWARD rules.
///
/// The rules are applied to the FORWARD chain for traffic from the container's IP.
/// We use a dedicated chain named "AGENT-{name}" for easy cleanup.
/// Always allows: DNS (53/udp, 53/tcp), traffic to host veth IP (for API access).
pub async fn apply_egress_rules(
    name: &str,
    egress_policy: &serde_json::Value,
) -> Result<(), AppError> {
    let container_ip = agent_ip(name)?;
    let host_ip = agent_host_ip(name)?;
    let chain_name = format!("AGENT-{name}");

    // Create a dedicated iptables chain for this agent
    let mut cmds = vec![
        format!("iptables -N {chain_name} 2>/dev/null || iptables -F {chain_name}"),
        // Always allow DNS
        format!("iptables -A {chain_name} -p udp --dport 53 -j ACCEPT"),
        format!("iptables -A {chain_name} -p tcp --dport 53 -j ACCEPT"),
        // Always allow traffic to the host veth IP (for dashboard API, postgres, etc.)
        format!("iptables -A {chain_name} -d {host_ip} -j ACCEPT"),
        // Always allow established/related connections
        format!("iptables -A {chain_name} -m state --state ESTABLISHED,RELATED -j ACCEPT"),
    ];

    if let Some(allow_list) = egress_policy.get("allow").and_then(|v| v.as_array()) {
        // Allow-list mode: resolve each domain and add ACCEPT rules, then DROP all else
        let domains: Vec<&str> = allow_list.iter().filter_map(|v| v.as_str()).collect();
        for domain in &domains {
            // Use the domain directly — iptables can resolve hostnames
            cmds.push(format!("iptables -A {chain_name} -d {domain} -j ACCEPT"));
        }
        // Drop everything else
        cmds.push(format!("iptables -A {chain_name} -j DROP"));
    } else if let Some(deny_list) = egress_policy.get("deny").and_then(|v| v.as_array()) {
        // Deny-list mode: block specific domains, allow everything else
        let domains: Vec<&str> = deny_list.iter().filter_map(|v| v.as_str()).collect();
        for domain in &domains {
            cmds.push(format!("iptables -A {chain_name} -d {domain} -j DROP"));
        }
        // Accept everything else (implicit by falling through to FORWARD default)
        cmds.push(format!("iptables -A {chain_name} -j ACCEPT"));
    } else {
        // No restrictions, just accept all
        cmds.push(format!("iptables -A {chain_name} -j ACCEPT"));
    }

    // Jump from FORWARD to our chain for traffic from this container
    cmds.push(format!(
        "iptables -I FORWARD -s {container_ip} -j {chain_name}"
    ));

    let script = cmds.join(" && ");
    run_cmd("bash", &["-c", &script]).await?;
    Ok(())
}

/// Remove network egress rules for an agent container.
pub async fn remove_egress_rules(name: &str) -> Result<(), AppError> {
    let container_ip = match agent_ip(name) {
        Ok(ip) => ip,
        Err(_) => return Ok(()), // Agent tracking file already gone, nothing to clean
    };
    let chain_name = format!("AGENT-{name}");

    // Remove the FORWARD jump rule, flush and delete the chain
    let script = format!(
        "iptables -D FORWARD -s {container_ip} -j {chain_name} 2>/dev/null; \
         iptables -F {chain_name} 2>/dev/null; \
         iptables -X {chain_name} 2>/dev/null; \
         true"
    );
    run_cmd("bash", &["-c", &script]).await?;
    Ok(())
}

/// Get the container IP for an agent from its tracking file (public for use in tasks.rs).
pub fn agent_ip_public(name: &str) -> Result<String, AppError> {
    agent_ip(name)
}

/// Get the host-side veth IP for an agent container from its tracking file.
pub fn agent_host_ip(name: &str) -> Result<String, AppError> {
    let path = format!("/var/lib/claude-agents/{name}");
    let content = std::fs::read_to_string(&path)
        .map_err(|e| AppError::Internal(format!("Cannot read agent tracking file {path}: {e}")))?;
    // Format: "slot host_ip container_ip"
    content
        .split_whitespace()
        .nth(1)
        .map(String::from)
        .ok_or_else(|| AppError::Internal(format!("Bad format in agent tracking file {path}")))
}

/// Fetch the last `max_lines` lines of logs from a preview container.
pub async fn get_preview_logs(
    config: &Config,
    slug: &str,
    max_lines: usize,
) -> Result<String, AppError> {
    let output = tokio::process::Command::new(&config.preview_bin)
        .args(["logs", slug])
        .output()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to run preview logs: {e}")))?;
    let text = String::from_utf8_lossy(&output.stdout).to_string();
    let lines: Vec<&str> = text.lines().collect();
    let tail = if lines.len() > max_lines {
        &lines[lines.len() - max_lines..]
    } else {
        &lines[..]
    };
    Ok(tail.join("\n"))
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
pub async fn scp_to_agent(name: &str, local_path: &str, remote_path: &str) -> Result<(), AppError> {
    let ip = agent_ip(name)?;
    run_cmd(
        "scp",
        &[
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            "UserKnownHostsFile=/dev/null",
            local_path,
            &format!("agent@{ip}:{remote_path}"),
        ],
    )
    .await?;
    Ok(())
}

/// Recursively copy `remote_path` out of an agent container to `local_path` on the host.
pub async fn scp_from_agent(
    name: &str,
    remote_path: &str,
    local_path: &str,
) -> Result<(), AppError> {
    let ip = agent_ip(name)?;
    run_cmd(
        "scp",
        &[
            "-r",
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            "UserKnownHostsFile=/dev/null",
            &format!("agent@{ip}:{remote_path}"),
            local_path,
        ],
    )
    .await?;
    Ok(())
}

/// Names of all agent containers known to the agent manager.
pub async fn list_agents(config: &Config) -> Result<Vec<String>, AppError> {
    let output = run_cmd(&config.agent_bin, &["list"]).await?;
    Ok(parse_agent_list(&output))
}

fn parse_agent_list(output: &str) -> Vec<String> {
    strip_ansi(output)
        .lines()
        .skip(1) // header
        .filter_map(|line| line.split_whitespace().next())
        .filter(|name| !name.starts_with('('))
        .map(String::from)
        .collect()
}

/// Run a command inside an agent container via SSH and capture stdout.
pub async fn agent_exec_capture(name: &str, cmd_line: &str) -> Result<String, AppError> {
    let ip = agent_ip(name)?;
    run_cmd(
        "ssh",
        &[
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            "UserKnownHostsFile=/dev/null",
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
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            "UserKnownHostsFile=/dev/null",
            &format!("agent@{ip}"),
            cmd_line,
        ],
        tx,
    )
    .await
}

/// Run Claude in an agent container with stream-json output, parsing events into readable log lines.
/// Returns Claude's accumulated text output, structured actions, and token usage. On failure the
/// error carries whatever was streamed before it, so callers can still record that usage.
pub async fn agent_exec_claude_streaming(
    name: &str,
    cmd_line: &str,
    tx: broadcast::Sender<String>,
) -> ClaudeRun {
    let ip = agent_ip(name).map_err(ClaudeStreamError::before_start)?;

    tracing::info!("Streaming Claude JSON: ssh agent@{ip}");
    let mut cmd = Command::new("ssh");
    cmd.args([
        "-o",
        "StrictHostKeyChecking=no",
        "-o",
        "UserKnownHostsFile=/dev/null",
        &format!("agent@{ip}"),
        cmd_line,
    ]);
    stream_claude(cmd, tx).await
}

/// Spawn `cmd` and parse its stdout as Claude stream-json.
async fn stream_claude(mut cmd: Command, tx: broadcast::Sender<String>) -> ClaudeRun {
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            ClaudeStreamError::before_start(AppError::Internal(format!(
                "Failed to spawn ssh for Claude streaming: {e}"
            )))
        })?;

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let claude_text = Arc::new(tokio::sync::Mutex::new(String::new()));
    let claude_text_clone = claude_text.clone();

    let actions = Arc::new(tokio::sync::Mutex::new(Vec::<RawAction>::new()));
    let actions_clone = actions.clone();

    let usage = Arc::new(tokio::sync::Mutex::new(UsageAccumulator::default()));
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
            usage_clone.lock().await.feed(&line);
            if let Some(formatted) = format_claude_event(&line) {
                let _ = tx.send(formatted);
            }
        }
    });

    let stderr_lines = Arc::new(tokio::sync::Mutex::new(Vec::<String>::new()));
    let stderr_lines_clone = stderr_lines.clone();
    let stderr_handle = tokio::spawn(async move {
        let reader = BufReader::new(stderr);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            // Skip noisy SSH warnings
            if line.contains("Warning: Permanently added") {
                continue;
            }
            stderr_lines_clone.lock().await.push(line.clone());
            let _ = tx2.send(line);
        }
    });

    let _ = stdout_handle.await;
    let _ = stderr_handle.await;

    let output = ClaudeStreamResult {
        text: claude_text.lock().await.clone(),
        actions: actions.lock().await.clone(),
        usage: usage.lock().await.finish(),
    };

    let status = match child.wait().await {
        Ok(status) => status,
        Err(e) => {
            return Err(ClaudeStreamError {
                error: AppError::Internal(format!("Failed to wait on ssh: {e}")),
                partial: output,
            })
        }
    };

    if !status.success() {
        let stderr_output = stderr_lines.lock().await.join("\n");
        let detail = if stderr_output.is_empty() {
            String::new()
        } else {
            format!(": {stderr_output}")
        };
        return Err(ClaudeStreamError {
            error: AppError::Internal(format!(
                "Claude streaming exited with status {status}{detail}"
            )),
            partial: output,
        });
    }

    Ok(output)
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
                                let name =
                                    block.get("name").and_then(|n| n.as_str()).map(String::from);
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
        "result" => Some(RawAction {
            action_type: "result".into(),
            tool_name: None,
            tool_input: None,
            summary: Some("Claude finished".into()),
        }),
        _ => None,
    }
}

/// Extract token usage and cost from a Claude stream-json "result" event.
fn extract_token_usage(line: &str) -> Option<TokenUsage> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    if v.get("type")?.as_str()? != "result" {
        return None;
    }
    let input = v
        .pointer("/usage/input_tokens")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let output = v
        .pointer("/usage/output_tokens")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let cost = v
        .get("total_cost_usd")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    if input == 0 && output == 0 && cost == 0.0 {
        return None;
    }
    Some(TokenUsage {
        input_tokens: input,
        output_tokens: output,
        cost_usd: cost,
    })
}

/// Generate a human-readable summary for a tool_use block.
fn format_tool_use_summary(block: &serde_json::Value) -> Option<String> {
    let name = block.get("name")?.as_str()?;
    let input = block.get("input").unwrap_or(&serde_json::Value::Null);

    let summary = match name {
        "Read" => {
            let path = input
                .get("file_path")
                .and_then(|p| p.as_str())
                .unwrap_or("...");
            format!("Reading {path}")
        }
        "Edit" => {
            let path = input
                .get("file_path")
                .and_then(|p| p.as_str())
                .unwrap_or("...");
            format!("Editing {path}")
        }
        "Write" => {
            let path = input
                .get("file_path")
                .and_then(|p| p.as_str())
                .unwrap_or("...");
            format!("Writing {path}")
        }
        "Grep" => {
            let pattern = input
                .get("pattern")
                .and_then(|p| p.as_str())
                .unwrap_or("...");
            format!("Searching for \"{pattern}\"")
        }
        "Glob" => {
            let pattern = input
                .get("pattern")
                .and_then(|p| p.as_str())
                .unwrap_or("...");
            format!("Globbing \"{pattern}\"")
        }
        "Bash" => {
            let cmd = input
                .get("command")
                .and_then(|c| c.as_str())
                .unwrap_or("...");
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
            let path = input
                .get("file_path")
                .and_then(|p| p.as_str())
                .unwrap_or("...");
            Some(format!("⚡ Reading {path}"))
        }
        "Edit" => {
            let path = input
                .get("file_path")
                .and_then(|p| p.as_str())
                .unwrap_or("...");
            Some(format!("✏️ Editing {path}"))
        }
        "Write" => {
            let path = input
                .get("file_path")
                .and_then(|p| p.as_str())
                .unwrap_or("...");
            Some(format!("✏️ Writing {path}"))
        }
        "Grep" => {
            let pattern = input
                .get("pattern")
                .and_then(|p| p.as_str())
                .unwrap_or("...");
            Some(format!("🔍 Searching for \"{pattern}\""))
        }
        "Glob" => {
            let pattern = input
                .get("pattern")
                .and_then(|p| p.as_str())
                .unwrap_or("...");
            Some(format!("🔍 Globbing \"{pattern}\""))
        }
        "Bash" => {
            let cmd = input
                .get("command")
                .and_then(|c| c.as_str())
                .unwrap_or("...");
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

    #[test]
    fn test_parse_agent_list() {
        let output = "\x1b[1mNAME            STATUS          HOST IP         CONTAINER IP\x1b[0m\n\
task-abc123     \x1b[0;32mrunning\x1b[0m         10.100.0.2      10.100.0.3\n\
task-def456     \x1b[1;33mstopped\x1b[0m         10.100.0.4      10.100.0.5\n";
        assert_eq!(parse_agent_list(output), vec!["task-abc123", "task-def456"]);
        let empty = "NAME            STATUS          HOST IP         CONTAINER IP\n  \x1b[0;36m(no agents)\x1b[0m\n";
        assert!(parse_agent_list(empty).is_empty());
    }

    fn assistant_event(id: &str, input: i64, output: i64) -> String {
        format!(
            r#"{{"type":"assistant","message":{{"id":"{id}","content":[{{"type":"text","text":"hi"}}],"usage":{{"input_tokens":{input},"output_tokens":{output}}}}}}}"#
        )
    }

    fn result_event(input: i64, output: i64, cost: f64) -> String {
        format!(
            r#"{{"type":"result","total_cost_usd":{cost},"usage":{{"input_tokens":{input},"output_tokens":{output}}}}}"#
        )
    }

    fn usage_of(lines: &[String]) -> TokenUsage {
        let mut acc = UsageAccumulator::default();
        for line in lines {
            acc.feed(line);
        }
        acc.finish()
    }

    #[test]
    fn usage_prefers_result_event_over_partial() {
        let usage = usage_of(&[
            assistant_event("m1", 10, 5),
            assistant_event("m2", 20, 7),
            result_event(100, 50, 0.25),
        ]);
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.cost_usd, 0.25);
    }

    #[test]
    fn partial_usage_dedupes_repeated_message_ids() {
        // stream-json repeats a message's usage on each of its content blocks
        let usage = usage_of(&[
            assistant_event("m1", 10, 3),
            assistant_event("m1", 10, 5),
            assistant_event("m2", 20, 7),
        ]);
        assert_eq!(usage.input_tokens, 30);
        assert_eq!(usage.output_tokens, 12);
        assert_eq!(usage.cost_usd, 0.0);
    }

    #[test]
    fn no_usage_information_is_zero() {
        let usage = usage_of(&[
            "not json".to_string(),
            r#"{"type":"system","subtype":"init"}"#.to_string(),
        ]);
        assert_eq!(usage.input_tokens, 0);
        assert_eq!(usage.output_tokens, 0);
        assert_eq!(usage.cost_usd, 0.0);
    }

    /// Run `stream_claude` on a shell script that prints `lines` and exits with `code`.
    async fn run_fake_claude(lines: &[String], code: i32) -> ClaudeRun {
        let body: String = lines.iter().map(|l| format!("{l}\n")).collect();
        let mut cmd = Command::new("sh");
        cmd.args(["-c", &format!("cat <<'EOF'\n{body}EOF\nexit {code}")]);
        let (tx, _rx) = broadcast::channel(64);
        stream_claude(cmd, tx).await
    }

    #[tokio::test]
    async fn successful_run_reports_result_usage() {
        let run = run_fake_claude(
            &[assistant_event("m1", 10, 5), result_event(100, 50, 0.25)],
            0,
        )
        .await;
        let output = run.expect("run should succeed");
        assert_eq!(output.usage.input_tokens, 100);
        assert_eq!(output.usage.output_tokens, 50);
        assert_eq!(output.usage.cost_usd, 0.25);
        assert_eq!(output.text, "hi");
    }

    #[tokio::test]
    async fn failed_run_keeps_final_result_usage() {
        let run = run_fake_claude(
            &[assistant_event("m1", 10, 5), result_event(100, 50, 0.25)],
            1,
        )
        .await;
        let err = run.expect_err("run should fail");
        assert_eq!(err.partial.usage.input_tokens, 100);
        assert_eq!(err.partial.usage.output_tokens, 50);
        assert_eq!(err.partial.usage.cost_usd, 0.25);
    }

    #[tokio::test]
    async fn failed_run_keeps_partial_usage() {
        let run = run_fake_claude(
            &[assistant_event("m1", 10, 5), assistant_event("m2", 20, 7)],
            1,
        )
        .await;
        let err = run.expect_err("run should fail");
        assert_eq!(err.partial.usage.input_tokens, 30);
        assert_eq!(err.partial.usage.output_tokens, 12);
        assert_eq!(err.partial.usage.cost_usd, 0.0);
        assert_eq!(err.partial.actions.len(), 2);
    }

    #[tokio::test]
    async fn failed_run_without_usage_is_zero() {
        let err = run_fake_claude(&[], 1).await.expect_err("run should fail");
        assert_eq!(err.partial.usage.input_tokens, 0);
        assert_eq!(err.partial.usage.output_tokens, 0);
        assert_eq!(err.partial.usage.cost_usd, 0.0);
    }

    #[tokio::test]
    async fn fallback_invocations_are_accounted_independently() {
        // A failed --continue followed by a fresh-session fallback: each run reports only its
        // own usage, so recording both counts every invocation exactly once.
        let failed = run_fake_claude(&[assistant_event("m1", 10, 5)], 1)
            .await
            .expect_err("--continue should fail");
        let fallback =
            run_fake_claude(&[assistant_event("m1", 40, 9), result_event(40, 9, 0.1)], 0)
                .await
                .expect("fallback should succeed");
        assert_eq!(failed.partial.usage.input_tokens, 10);
        assert_eq!(fallback.usage.input_tokens, 40);
        assert_eq!(
            failed.partial.usage.output_tokens + fallback.usage.output_tokens,
            14
        );
    }
}
