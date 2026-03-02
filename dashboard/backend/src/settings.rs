use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::auth::MemberUser;
use crate::error::AppError;
use crate::secrets::{decrypt_secret, encrypt_secret};

// ── Data model ──

pub struct UserAiConfig {
    pub provider: String,
    pub api_key: String,
}

// ── CRUD helpers ──

pub async fn get_user_ai_config(
    pool: &PgPool,
    encryption_key: &str,
    login: &str,
) -> Result<Option<UserAiConfig>, AppError> {
    let row = sqlx::query_as::<_, (Option<String>, Option<String>)>(
        "SELECT ai_provider, ai_api_key_encrypted FROM users WHERE github_login = $1",
    )
    .bind(login)
    .fetch_optional(pool)
    .await?;

    match row {
        Some((Some(provider), Some(encrypted_key))) => {
            let api_key = decrypt_secret(encryption_key, &encrypted_key)?;
            Ok(Some(UserAiConfig { provider, api_key }))
        }
        _ => Ok(None),
    }
}

pub async fn set_user_ai_config(
    pool: &PgPool,
    encryption_key: &str,
    login: &str,
    provider: &str,
    api_key: &str,
) -> Result<(), AppError> {
    let encrypted = encrypt_secret(encryption_key, api_key)?;
    sqlx::query(
        "UPDATE users SET ai_provider = $1, ai_api_key_encrypted = $2 WHERE github_login = $3",
    )
    .bind(provider)
    .bind(&encrypted)
    .bind(login)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn clear_user_ai_config(pool: &PgPool, login: &str) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE users SET ai_provider = NULL, ai_api_key_encrypted = NULL WHERE github_login = $1",
    )
    .bind(login)
    .execute(pool)
    .await?;
    Ok(())
}

// ── HTTP handlers ──

#[derive(Serialize)]
pub struct AiSettingsResponse {
    pub provider: Option<String>,
    pub has_api_key: bool,
}

#[derive(Deserialize)]
pub struct SetAiSettingsRequest {
    pub provider: String,
    pub api_key: String,
}

/// GET /api/settings/ai
pub async fn get_ai_settings(
    user: MemberUser,
    State(state): State<crate::AppState>,
) -> Result<Json<AiSettingsResponse>, AppError> {
    if state.config.secrets_encryption_key.is_empty() {
        return Ok(Json(AiSettingsResponse {
            provider: None,
            has_api_key: false,
        }));
    }

    match get_user_ai_config(&state.db, &state.config.secrets_encryption_key, &user.0.sub).await? {
        Some(cfg) => Ok(Json(AiSettingsResponse {
            provider: Some(cfg.provider),
            has_api_key: true,
        })),
        None => Ok(Json(AiSettingsResponse {
            provider: None,
            has_api_key: false,
        })),
    }
}

/// PUT /api/settings/ai
pub async fn put_ai_settings(
    user: MemberUser,
    State(state): State<crate::AppState>,
    Json(req): Json<SetAiSettingsRequest>,
) -> Result<Json<AiSettingsResponse>, AppError> {
    if state.config.secrets_encryption_key.is_empty() {
        return Err(AppError::BadRequest(
            "SECRETS_ENCRYPTION_KEY is not configured".into(),
        ));
    }

    if req.provider != "anthropic" && req.provider != "openrouter" {
        return Err(AppError::BadRequest(format!(
            "Invalid provider '{}'. Use 'anthropic' or 'openrouter'",
            req.provider
        )));
    }

    if req.api_key.is_empty() {
        return Err(AppError::BadRequest("API key cannot be empty".into()));
    }

    set_user_ai_config(
        &state.db,
        &state.config.secrets_encryption_key,
        &user.0.sub,
        &req.provider,
        &req.api_key,
    )
    .await?;

    Ok(Json(AiSettingsResponse {
        provider: Some(req.provider),
        has_api_key: true,
    }))
}

/// DELETE /api/settings/ai
pub async fn delete_ai_settings(
    user: MemberUser,
    State(state): State<crate::AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    clear_user_ai_config(&state.db, &user.0.sub).await?;
    Ok(Json(serde_json::json!({ "deleted": true })))
}
