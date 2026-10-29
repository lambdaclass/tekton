use axum::extract::{Path, State};
use axum::Json;

use crate::auth::AdminUser;
use crate::error::AppError;
use crate::models::{
    CreateSentrySourceRequest, SentryIssue, SentrySource, UpdateSentrySourceRequest,
};
use crate::sentry::{SentrySourceWithSecret, SECRET_COLUMNS, SOURCE_COLUMNS};

const ISSUE_COLUMNS: &str = "id, source_id, sentry_issue_id, sentry_short_id, sentry_url, title, \
     github_issue_number, github_issue_url, \
     TO_CHAR(created_at, 'YYYY-MM-DD HH24:MI:SS') as created_at";

fn encrypt_token(state: &crate::AppState, token: &str) -> Result<String, AppError> {
    if state.config.secrets_encryption_key.is_empty() {
        return Err(AppError::BadRequest(
            "SECRETS_ENCRYPTION_KEY is not configured".into(),
        ));
    }
    crate::secrets::encrypt_secret(&state.config.secrets_encryption_key, token)
}

async fn ensure_github_user(state: &crate::AppState, login: &str) -> Result<(), AppError> {
    let _: String = sqlx::query_scalar("SELECT github_token FROM users WHERE github_login = $1")
        .bind(login)
        .fetch_one(&state.db)
        .await
        .map_err(|_| {
            AppError::BadRequest(format!("User '{login}' not found or has no GitHub token"))
        })?;
    Ok(())
}

/// GET /api/admin/sentry/sources
pub async fn list_sources(
    _admin: AdminUser,
    State(state): State<crate::AppState>,
) -> Result<Json<Vec<SentrySource>>, AppError> {
    let sources = sqlx::query_as::<_, SentrySource>(&format!(
        "SELECT {SOURCE_COLUMNS} FROM sentry_sources ORDER BY created_at DESC"
    ))
    .fetch_all(&state.db)
    .await?;
    Ok(Json(sources))
}

/// POST /api/admin/sentry/sources
pub async fn create_source(
    admin: AdminUser,
    State(state): State<crate::AppState>,
    Json(req): Json<CreateSentrySourceRequest>,
) -> Result<Json<SentrySource>, AppError> {
    if req.auth_token.trim().is_empty() {
        return Err(AppError::BadRequest("Sentry auth token is required".into()));
    }
    crate::intake::parse_owner_repo(&req.target_repo)?;
    ensure_github_user(&state, &req.run_as_user).await?;
    let encrypted = encrypt_token(&state, req.auth_token.trim())?;

    let source = sqlx::query_as::<_, SentrySource>(&format!(
        "INSERT INTO sentry_sources \
         (name, sentry_org, sentry_projects, environments, auth_token_encrypted, \
          target_repo, labels, run_as_user, created_by) \
         VALUES ($1, $2, $3, COALESCE($4, '{{production}}'), $5, $6, \
                 COALESCE($7, '{{Sentry Alert}}'), $8, $9) \
         RETURNING {SOURCE_COLUMNS}"
    ))
    .bind(&req.name)
    .bind(&req.sentry_org)
    .bind(&req.sentry_projects)
    .bind(&req.environments)
    .bind(&encrypted)
    .bind(&req.target_repo)
    .bind(&req.labels)
    .bind(&req.run_as_user)
    .bind(&admin.0.sub)
    .fetch_one(&state.db)
    .await?;

    crate::audit::log_event(
        &state.db,
        "admin.sentry_source_create",
        &admin.0.sub,
        Some(&req.target_repo),
        serde_json::json!({ "source_id": source.id, "name": &req.name }),
        None,
    )
    .await;

    Ok(Json(source))
}

/// PUT /api/admin/sentry/sources/{id}
pub async fn update_source(
    admin: AdminUser,
    State(state): State<crate::AppState>,
    Path(id): Path<i64>,
    Json(req): Json<UpdateSentrySourceRequest>,
) -> Result<Json<SentrySource>, AppError> {
    if let Some(ref repo) = req.target_repo {
        crate::intake::parse_owner_repo(repo)?;
    }
    if let Some(ref login) = req.run_as_user {
        ensure_github_user(&state, login).await?;
    }
    let encrypted = match req.auth_token.as_deref().map(str::trim) {
        Some(token) if !token.is_empty() => Some(encrypt_token(&state, token)?),
        _ => None,
    };

    let source = sqlx::query_as::<_, SentrySource>(&format!(
        "UPDATE sentry_sources SET \
         name = COALESCE($2, name), \
         sentry_org = COALESCE($3, sentry_org), \
         sentry_projects = COALESCE($4, sentry_projects), \
         environments = COALESCE($5, environments), \
         auth_token_encrypted = COALESCE($6, auth_token_encrypted), \
         target_repo = COALESCE($7, target_repo), \
         labels = COALESCE($8, labels), \
         run_as_user = COALESCE($9, run_as_user), \
         updated_at = NOW() \
         WHERE id = $1 RETURNING {SOURCE_COLUMNS}"
    ))
    .bind(id)
    .bind(&req.name)
    .bind(&req.sentry_org)
    .bind(&req.sentry_projects)
    .bind(&req.environments)
    .bind(&encrypted)
    .bind(&req.target_repo)
    .bind(&req.labels)
    .bind(&req.run_as_user)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("Sentry source not found".into()))?;

    crate::audit::log_event(
        &state.db,
        "admin.sentry_source_update",
        &admin.0.sub,
        Some(&source.target_repo),
        serde_json::json!({ "source_id": source.id, "token_changed": encrypted.is_some() }),
        None,
    )
    .await;

    Ok(Json(source))
}

/// DELETE /api/admin/sentry/sources/{id}
pub async fn delete_source(
    admin: AdminUser,
    State(state): State<crate::AppState>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    let name: String =
        sqlx::query_scalar("DELETE FROM sentry_sources WHERE id = $1 RETURNING name")
            .bind(id)
            .fetch_optional(&state.db)
            .await?
            .ok_or_else(|| AppError::NotFound("Sentry source not found".into()))?;

    crate::audit::log_event(
        &state.db,
        "admin.sentry_source_delete",
        &admin.0.sub,
        None,
        serde_json::json!({ "source_id": id, "name": name }),
        None,
    )
    .await;

    Ok(Json(serde_json::json!({ "deleted": true })))
}

/// POST /api/admin/sentry/sources/{id}/toggle
pub async fn toggle_source(
    admin: AdminUser,
    State(state): State<crate::AppState>,
    Path(id): Path<i64>,
) -> Result<Json<SentrySource>, AppError> {
    let source = sqlx::query_as::<_, SentrySource>(&format!(
        "UPDATE sentry_sources SET enabled = NOT enabled, updated_at = NOW() \
         WHERE id = $1 RETURNING {SOURCE_COLUMNS}"
    ))
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("Sentry source not found".into()))?;

    crate::audit::log_event(
        &state.db,
        "admin.sentry_source_toggle",
        &admin.0.sub,
        Some(&source.target_repo),
        serde_json::json!({ "source_id": source.id, "enabled": source.enabled }),
        None,
    )
    .await;

    Ok(Json(source))
}

/// GET /api/admin/sentry/sources/{id}/issues — GitHub issues already opened
pub async fn list_source_issues(
    _admin: AdminUser,
    State(state): State<crate::AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Vec<SentryIssue>>, AppError> {
    let issues = sqlx::query_as::<_, SentryIssue>(&format!(
        "SELECT {ISSUE_COLUMNS} FROM sentry_issues WHERE source_id = $1 \
         ORDER BY created_at DESC LIMIT 200"
    ))
    .bind(id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(issues))
}

/// POST /api/admin/sentry/sources/{id}/test — dry run: what the next polls would file
pub async fn test_poll_source(
    _admin: AdminUser,
    State(state): State<crate::AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let due = sqlx::query_as::<_, SentrySourceWithSecret>(&format!(
        "SELECT {SOURCE_COLUMNS}, {SECRET_COLUMNS} FROM sentry_sources WHERE id = $1"
    ))
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("Sentry source not found".into()))?;

    let token = crate::secrets::decrypt_secret(
        &state.config.secrets_encryption_key,
        &due.auth_token_encrypted,
    )?;
    let issues =
        crate::sentry::unfiled_sentry_issues(&state.db, &due.source, &token, &due.cutoff).await?;

    Ok(Json(
        issues
            .iter()
            .take(20)
            .map(|issue| {
                serde_json::json!({
                    "title": crate::sentry::build_issue_title(issue),
                    "url": &issue.permalink,
                    "project": &issue.project.slug,
                })
            })
            .collect(),
    ))
}
