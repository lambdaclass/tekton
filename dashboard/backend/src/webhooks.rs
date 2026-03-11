use axum::extract::{Path, State};
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::auth::MemberUser;
use crate::error::AppError;
use crate::AppState;

// GraphQL response types for organization repositories query
#[derive(Debug, Deserialize)]
struct GraphQLResponse {
    data: Option<GraphQLData>,
    errors: Option<Vec<GraphQLError>>,
}

#[derive(Debug, Deserialize)]
struct GraphQLData {
    organization: GraphQLOrg,
}

#[derive(Debug, Deserialize)]
struct GraphQLOrg {
    repositories: GraphQLRepoConnection,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphQLRepoConnection {
    page_info: GraphQLPageInfo,
    nodes: Vec<GraphQLRepoNode>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphQLPageInfo {
    has_next_page: bool,
    end_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphQLRepoNode {
    name_with_owner: String,
    viewer_can_administer: bool,
}

#[derive(Debug, Deserialize)]
struct GraphQLError {
    message: String,
}

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
    let t_start = std::time::Instant::now();
    let token = get_github_token(&state, &user.0.sub).await?;
    tracing::info!(elapsed_ms = t_start.elapsed().as_millis() as u64, "webhooks/repos: fetched github token");
    let client = reqwest::Client::new();
    let org = &state.config.github_org;
    let webhook_url = format!(
        "https://webhook.{}/webhook/github",
        state.config.preview_domain
    );

    // Fetch admin repos via GraphQL (single query with cursor pagination)
    let query = r#"
        query($org: String!, $cursor: String) {
            organization(login: $org) {
                repositories(first: 100, after: $cursor) {
                    pageInfo { hasNextPage endCursor }
                    nodes { nameWithOwner viewerCanAdminister }
                }
            }
        }
    "#;

    let mut admin_repos: Vec<String> = Vec::new();
    let mut cursor: Option<String> = None;
    let mut graphql_page = 0u32;

    loop {
        graphql_page += 1;
        let t_page = std::time::Instant::now();
        let body = serde_json::json!({
            "query": query,
            "variables": { "org": org, "cursor": cursor },
        });

        let resp: GraphQLResponse = client
            .post("https://api.github.com/graphql")
            .header("Authorization", format!("bearer {token}"))
            .header("User-Agent", "tekton-dashboard")
            .json(&body)
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("GitHub GraphQL request failed: {e}")))?
            .json()
            .await
            .map_err(|e| AppError::Internal(format!("Failed to parse GraphQL response: {e}")))?;

        if let Some(errors) = resp.errors {
            let msgs: Vec<String> = errors.into_iter().map(|e| e.message).collect();
            return Err(AppError::Internal(format!(
                "GitHub GraphQL errors: {}",
                msgs.join("; ")
            )));
        }

        let data = resp
            .data
            .ok_or_else(|| AppError::Internal("GitHub GraphQL returned no data".into()))?;

        let conn = data.organization.repositories;

        let admin_in_page = conn.nodes.iter().filter(|n| n.viewer_can_administer).count();
        tracing::info!(
            page = graphql_page,
            repos_in_page = conn.nodes.len(),
            admin_in_page,
            elapsed_ms = t_page.elapsed().as_millis() as u64,
            "webhooks/repos: GraphQL page fetched"
        );

        for node in &conn.nodes {
            if node.viewer_can_administer {
                admin_repos.push(node.name_with_owner.clone());
            }
        }

        if conn.page_info.has_next_page {
            cursor = conn.page_info.end_cursor;
        } else {
            break;
        }
    }

    tracing::info!(
        total_admin_repos = admin_repos.len(),
        graphql_pages = graphql_page,
        elapsed_ms = t_start.elapsed().as_millis() as u64,
        "webhooks/repos: finished GraphQL pagination"
    );

    // If ALLOWED_REPOS is non-empty, further filter
    if !state.config.allowed_repos.is_empty() {
        admin_repos.retain(|r| state.config.allowed_repos.contains(r));
        tracing::info!(filtered_repos = admin_repos.len(), "webhooks/repos: filtered by ALLOWED_REPOS");
    }

    // Check webhook status for each repo concurrently using JoinSet
    let t_hooks = std::time::Instant::now();
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

    tracing::info!(
        repos_checked = results.len(),
        elapsed_ms = t_hooks.elapsed().as_millis() as u64,
        "webhooks/repos: finished webhook status checks"
    );

    // Sort by repo name for consistent ordering
    results.sort_by(|a, b| a.full_name.cmp(&b.full_name));

    tracing::info!(
        total_elapsed_ms = t_start.elapsed().as_millis() as u64,
        "webhooks/repos: request complete"
    );

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
        "https://webhook.{}/webhook/github",
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
        .post(format!("https://api.github.com/repos/{owner}/{repo}/hooks"))
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
