use axum::extract::{Path, State};
use axum::Json;

use crate::auth::{self, AuthUser, MemberUser};
use crate::error::AppError;
use crate::models::{CreatePreviewRequest, Preview};
use crate::shell;
use crate::AppState;

async fn get_github_token(state: &AppState, github_login: &str) -> Result<String, AppError> {
    sqlx::query_scalar("SELECT github_token FROM users WHERE github_login = $1")
        .bind(github_login)
        .fetch_one(&state.db)
        .await
        .map_err(|_| AppError::Auth("User not found".to_string()))
}

pub async fn list_previews(
    _user: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<Preview>>, AppError> {
    let previews = shell::list_previews(&state.config).await?;
    Ok(Json(previews))
}

pub async fn create_preview(
    user: MemberUser,
    State(state): State<AppState>,
    Json(req): Json<CreatePreviewRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !state.config.allowed_repos.is_empty() && !state.config.allowed_repos.contains(&req.repo) {
        return Err(AppError::BadRequest(format!(
            "Repo '{}' is not in the allowed list",
            req.repo
        )));
    }

    // Check per-user repo permission
    auth::check_repo_permission(
        &state.db,
        &user.0.sub,
        &req.repo,
        &user.0.role,
        &state.config.github_org,
    )
    .await?;

    let github_token = get_github_token(&state, &user.0.sub).await?;
    let output = shell::create_preview(
        &state.config,
        &req.repo,
        &req.branch,
        req.slug.as_deref(),
        &github_token,
    )
    .await
    .map_err(|e| classify_create_error(e, &req.repo, req.slug.as_deref()))?;

    // Audit: preview.created
    crate::audit::log_event(
        &state.db,
        "preview.created",
        &user.0.sub,
        req.slug.as_deref(),
        serde_json::json!({ "repo": &req.repo, "branch": &req.branch }),
        None,
    )
    .await;

    Ok(Json(serde_json::json!({
        "message": "Preview creation started",
        "output": output.trim(),
    })))
}

pub async fn destroy_preview(
    _user: MemberUser,
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let output = shell::destroy_preview(&state.config, &slug).await?;

    // Audit: preview.destroyed
    crate::audit::log_event(
        &state.db,
        "preview.destroyed",
        &_user.0.sub,
        Some(&slug),
        serde_json::json!({}),
        None,
    )
    .await;

    Ok(Json(serde_json::json!({
        "message": "Preview destroyed",
        "output": output.trim(),
    })))
}

pub async fn update_preview(
    user: MemberUser,
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let github_token = get_github_token(&state, &user.0.sub).await?;
    let output = shell::update_preview(&state.config, &slug, &github_token).await?;

    // Audit: preview.updated
    crate::audit::log_event(
        &state.db,
        "preview.updated",
        &user.0.sub,
        Some(&slug),
        serde_json::json!({}),
        None,
    )
    .await;

    Ok(Json(serde_json::json!({
        "message": "Preview update triggered",
        "output": output.trim(),
    })))
}

/// Inspect the raw shell error from `create_preview` and return a user-friendly
/// `AppError::UserError` when the failure matches a known pattern.
fn classify_create_error(err: AppError, repo: &str, slug: Option<&str>) -> AppError {
    let msg = match &err {
        AppError::Internal(msg) => msg.as_str(),
        _ => return err,
    };

    let lower = msg.to_lowercase();

    // Repository not accessible
    if lower.contains("repository") && lower.contains("not found")
        || lower.contains("could not read from remote repository")
        || lower.contains("fatal: remote error")
        || lower.contains("authentication failed")
        || lower.contains("could not resolve host")
    {
        tracing::warn!("Preview creation failed (repo_not_accessible): {msg}");
        return AppError::UserError {
            code: "repo_not_accessible",
            message: format!(
                "Could not access repository '{repo}'. Check that the name is correct and that the deploy token has access."
            ),
        };
    }

    // Slug / container conflict
    if lower.contains("already exists")
        || lower.contains("name conflict")
        || lower.contains("is already in use")
    {
        let slug_display = slug.unwrap_or("(auto)");
        tracing::warn!("Preview creation failed (slug_conflict): {msg}");
        return AppError::UserError {
            code: "slug_conflict",
            message: format!(
                "A preview with slug '{slug_display}' already exists. Choose a different slug or destroy the existing preview first."
            ),
        };
    }

    // Invalid slug characters
    if lower.contains("invalid slug")
        || lower.contains("invalid container name")
        || lower.contains("invalid character")
    {
        tracing::warn!("Preview creation failed (invalid_slug): {msg}");
        return AppError::UserError {
            code: "invalid_slug",
            message: "The slug contains invalid characters. Use only lowercase letters, numbers, and hyphens.".to_string(),
        };
    }

    // Slug too long
    if lower.contains("is too long") {
        tracing::warn!("Preview creation failed (slug_too_long): {msg}");
        return AppError::UserError {
            code: "slug_too_long",
            message: "The slug is too long. It must be 11 characters or fewer.".to_string(),
        };
    }

    // Branch not found
    if lower.contains("did not match any")
        || (lower.contains("pathspec") && lower.contains("did not match"))
        || lower.contains("remote branch") && lower.contains("not found")
    {
        tracing::warn!("Preview creation failed (branch_not_found): {msg}");
        return AppError::UserError {
            code: "branch_not_found",
            message: format!(
                "Could not find the specified branch in '{repo}'. Check that the branch name is correct."
            ),
        };
    }

    // Unrecognised — keep original Internal error
    err
}
