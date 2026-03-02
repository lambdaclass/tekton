use axum::extract::{Path, State};
use axum::Json;

use crate::auth::AuthUser;
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
    user: AuthUser,
    State(state): State<AppState>,
    Json(req): Json<CreatePreviewRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !state.config.allowed_repos.is_empty()
        && !state.config.allowed_repos.contains(&req.repo)
    {
        return Err(AppError::BadRequest(format!(
            "Repo '{}' is not in the allowed list",
            req.repo
        )));
    }

    let github_token = get_github_token(&state, &user.0.sub).await?;
    let output = shell::create_preview(
        &state.config,
        &req.repo,
        &req.branch,
        req.slug.as_deref(),
        &github_token,
    )
    .await?;

    Ok(Json(serde_json::json!({
        "message": "Preview creation started",
        "output": output.trim(),
    })))
}

pub async fn destroy_preview(
    _user: AuthUser,
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let output = shell::destroy_preview(&state.config, &slug).await?;
    Ok(Json(serde_json::json!({
        "message": "Preview destroyed",
        "output": output.trim(),
    })))
}

pub async fn update_preview(
    user: AuthUser,
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let github_token = get_github_token(&state, &user.0.sub).await?;
    let output = shell::update_preview(&state.config, &slug, &github_token).await?;
    Ok(Json(serde_json::json!({
        "message": "Preview update triggered",
        "output": output.trim(),
    })))
}
