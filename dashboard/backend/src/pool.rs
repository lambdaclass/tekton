use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::auth::AdminUser;
use crate::error::AppError;
use crate::shell;

#[derive(Debug, Serialize)]
pub struct PoolContainer {
    pub name: String,
    pub ip: String,
}

#[derive(Debug, Serialize)]
pub struct PoolStatus {
    pub target: u32,
    pub available: u32,
    pub containers: Vec<PoolContainer>,
}

#[derive(Debug, Deserialize)]
pub struct ResizeRequest {
    pub target: u32,
}

/// GET /api/admin/pool/status
pub async fn get_pool_status(
    _user: AdminUser,
    State(state): State<crate::AppState>,
) -> Result<Json<PoolStatus>, AppError> {
    let output = shell::pool_status(&state.config).await?;
    let status = parse_pool_status(&output, state.config.agent_pool_size)?;
    Ok(Json(status))
}

/// POST /api/admin/pool/resize
pub async fn resize_pool(
    _user: AdminUser,
    State(state): State<crate::AppState>,
    Json(req): Json<ResizeRequest>,
) -> Result<Json<PoolStatus>, AppError> {
    if req.target > 20 {
        return Err(AppError::BadRequest("Pool size cannot exceed 20".into()));
    }

    // Write the new pool-size file via the agent script's pool-init
    // (pool-init also sets the .pool-size file)
    shell::run_agent_cmd(&state.config, &["pool-init", &req.target.to_string()]).await?;

    // Read back the status
    let output = shell::pool_status(&state.config).await?;
    let status = parse_pool_status(&output, req.target)?;
    Ok(Json(status))
}

/// POST /api/admin/pool/refill
pub async fn refill_pool(
    _user: AdminUser,
    State(state): State<crate::AppState>,
) -> Result<Json<PoolStatus>, AppError> {
    shell::pool_refill(&state.config).await?;

    let output = shell::pool_status(&state.config).await?;
    let status = parse_pool_status(&output, state.config.agent_pool_size)?;
    Ok(Json(status))
}

/// Parse the output of `agent pool-status` into a structured PoolStatus.
fn parse_pool_status(output: &str, fallback_target: u32) -> Result<PoolStatus, AppError> {
    let clean = strip_ansi(output);
    let mut target: u32 = fallback_target;
    let mut available: u32 = 0;
    let mut containers = Vec::new();
    let mut in_containers = false;

    for line in clean.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("Target size:") {
            if let Some(v) = trimmed.split(':').nth(1) {
                target = v.trim().parse().unwrap_or(fallback_target);
            }
        } else if trimmed.starts_with("Available:") {
            if let Some(v) = trimmed.split(':').nth(1) {
                available = v.trim().parse().unwrap_or(0);
            }
        } else if trimmed.starts_with("Available containers:") {
            in_containers = true;
        } else if in_containers && !trimmed.is_empty() {
            // Format: "pool-1  (10.100.0.3)"
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if !parts.is_empty() {
                let name = parts[0].to_string();
                let ip = if parts.len() > 1 {
                    parts[1].trim_matches(|c| c == '(' || c == ')').to_string()
                } else {
                    String::new()
                };
                containers.push(PoolContainer { name, ip });
            }
        }
    }

    Ok(PoolStatus {
        target,
        available,
        containers,
    })
}

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
