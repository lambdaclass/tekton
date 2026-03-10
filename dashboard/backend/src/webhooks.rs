use axum::extract::{Path, State};
use axum::Json;
use serde::Serialize;

use crate::auth::MemberUser;
use crate::error::AppError;
use crate::AppState;

async fn get_github_token(state: &AppState, github_login: &str) -> Result<String, AppError> {
    sqlx::query_scalar("SELECT github_token FROM users WHERE github_login = $1")
        .bind(github_login)
        .fetch_one(&state.db)
        .await
        .map_err(|_| AppError::Auth("User not found".to_string()))
}

#[derive(Debug, Serialize, Clone)]
pub struct RepoWebhookInfo {
    pub full_name: String,
    pub hook_id: Option<i64>,
    pub active: bool,
}

/// GET /api/webhooks/repos — List org repos with webhook status
pub async fn list_repos_with_webhook_status(
    user: MemberUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<RepoWebhookInfo>>, AppError> {
    let token = get_github_token(&state, &user.0.sub).await?;
    let client = reqwest::Client::new();
    let org = &state.config.github_org;
    let webhook_url = format!(
        "https://webhook.preview.{}/webhook/github",
        state.config.preview_domain
    );

    // Fetch all org repos (paginated)
    let mut all_repos: Vec<serde_json::Value> = Vec::new();
    let mut page = 1u32;

    loop {
        let resp = client
            .get(format!(
                "https://api.github.com/orgs/{org}/repos?per_page=100&page={page}"
            ))
            .header("Authorization", format!("token {token}"))
            .header("User-Agent", "tekton-dashboard")
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("GitHub API request failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(AppError::Internal(format!(
                "GitHub API returned {status}: {text}"
            )));
        }

        let items: Vec<serde_json::Value> = resp
            .json()
            .await
            .map_err(|e| AppError::Internal(format!("Failed to parse repos response: {e}")))?;

        if items.is_empty() {
            break;
        }

        all_repos.extend(items.iter().cloned());

        if items.len() < 100 {
            break;
        }
        page += 1;
    }

    // Filter to repos where user has admin permission
    let mut admin_repos: Vec<String> = Vec::new();
    for repo in &all_repos {
        let has_admin = repo
            .get("permissions")
            .and_then(|p| p.get("admin"))
            .and_then(|a| a.as_bool())
            .unwrap_or(false);

        if has_admin {
            if let Some(full_name) = repo["full_name"].as_str() {
                admin_repos.push(full_name.to_string());
            }
        }
    }

    // If ALLOWED_REPOS is non-empty, further filter
    if !state.config.allowed_repos.is_empty() {
        admin_repos.retain(|r| state.config.allowed_repos.contains(r));
    }

    // Check webhook status for each repo concurrently using JoinSet
    let mut join_set = tokio::task::JoinSet::new();

    for repo_name in admin_repos {
        let client = client.clone();
        let token = token.clone();
        let webhook_url = webhook_url.clone();

        join_set.spawn(async move {
            let resp = client
                .get(format!(
                    "https://api.github.com/repos/{repo_name}/hooks?per_page=100"
                ))
                .header("Authorization", format!("token {token}"))
                .header("User-Agent", "tekton-dashboard")
                .header("Accept", "application/vnd.github+json")
                .send()
                .await;

            let (hook_id, active) = match resp {
                Ok(r) if r.status().is_success() => {
                    let hooks: Vec<serde_json::Value> = r.json().await.unwrap_or_default();
                    let matching = hooks.iter().find(|h| {
                        h.get("config")
                            .and_then(|c| c.get("url"))
                            .and_then(|u| u.as_str())
                            == Some(&webhook_url)
                    });
                    match matching {
                        Some(hook) => {
                            let id = hook["id"].as_i64();
                            let is_active = hook["active"].as_bool().unwrap_or(false);
                            (id, is_active)
                        }
                        None => (None, false),
                    }
                }
                _ => (None, false),
            };

            RepoWebhookInfo {
                full_name: repo_name,
                hook_id,
                active,
            }
        });
    }

    let mut results = Vec::new();
    while let Some(result) = join_set.join_next().await {
        if let Ok(info) = result {
            results.push(info);
        }
    }

    // Sort by repo name for consistent ordering
    results.sort_by(|a, b| a.full_name.cmp(&b.full_name));

    Ok(Json(results))
}

/// POST /api/webhooks/repos/{owner}/{repo} — Enable webhook
pub async fn create_webhook(
    user: MemberUser,
    State(state): State<AppState>,
    Path((owner, repo)): Path<(String, String)>,
) -> Result<Json<RepoWebhookInfo>, AppError> {
    let token = get_github_token(&state, &user.0.sub).await?;
    let client = reqwest::Client::new();
    let full_name = format!("{owner}/{repo}");
    let webhook_url = format!(
        "https://webhook.preview.{}/webhook/github",
        state.config.preview_domain
    );

    // Check if webhook already exists
    let hooks_resp = client
        .get(format!(
            "https://api.github.com/repos/{owner}/{repo}/hooks?per_page=100"
        ))
        .header("Authorization", format!("token {token}"))
        .header("User-Agent", "tekton-dashboard")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("GitHub API request failed: {e}")))?;

    if hooks_resp.status().is_success() {
        let hooks: Vec<serde_json::Value> = hooks_resp.json().await.unwrap_or_default();
        let existing = hooks.iter().find(|h| {
            h.get("config")
                .and_then(|c| c.get("url"))
                .and_then(|u| u.as_str())
                == Some(&webhook_url)
        });

        if let Some(hook) = existing {
            let hook_id = hook["id"].as_i64();
            return Ok(Json(RepoWebhookInfo {
                full_name,
                hook_id,
                active: true,
            }));
        }
    }

    // Create the webhook
    let body = serde_json::json!({
        "name": "web",
        "active": true,
        "events": ["pull_request"],
        "config": {
            "url": webhook_url,
            "content_type": "json",
            "secret": state.config.github_webhook_secret,
            "insecure_ssl": "0"
        }
    });

    let resp = client
        .post(format!(
            "https://api.github.com/repos/{owner}/{repo}/hooks"
        ))
        .header("Authorization", format!("token {token}"))
        .header("User-Agent", "tekton-dashboard")
        .header("Accept", "application/vnd.github+json")
        .json(&body)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("GitHub API request failed: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(AppError::Internal(format!(
            "Failed to create webhook: GitHub returned {status}: {text}"
        )));
    }

    let created: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to parse webhook response: {e}")))?;

    let hook_id = created["id"].as_i64();

    // Audit log
    crate::audit::log_event(
        &state.db,
        "webhook.created",
        &user.0.sub,
        Some(&full_name),
        serde_json::json!({ "hook_id": hook_id }),
        None,
    )
    .await;

    Ok(Json(RepoWebhookInfo {
        full_name,
        hook_id,
        active: true,
    }))
}

/// DELETE /api/webhooks/repos/{owner}/{repo}/{hook_id} — Disable webhook
pub async fn delete_webhook(
    user: MemberUser,
    State(state): State<AppState>,
    Path((owner, repo, hook_id)): Path<(String, String, i64)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let token = get_github_token(&state, &user.0.sub).await?;
    let client = reqwest::Client::new();
    let full_name = format!("{owner}/{repo}");

    let resp = client
        .delete(format!(
            "https://api.github.com/repos/{owner}/{repo}/hooks/{hook_id}"
        ))
        .header("Authorization", format!("token {token}"))
        .header("User-Agent", "tekton-dashboard")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("GitHub API request failed: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(AppError::Internal(format!(
            "Failed to delete webhook: GitHub returned {status}: {text}"
        )));
    }

    // Audit log
    crate::audit::log_event(
        &state.db,
        "webhook.deleted",
        &user.0.sub,
        Some(&full_name),
        serde_json::json!({ "hook_id": hook_id }),
        None,
    )
    .await;

    Ok(Json(serde_json::json!({ "deleted": true })))
}
