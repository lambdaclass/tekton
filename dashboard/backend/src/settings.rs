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
    pub model: Option<String>,
}

// ── CRUD helpers ──

pub async fn get_user_ai_config(
    pool: &PgPool,
    encryption_key: &str,
    login: &str,
) -> Result<Option<UserAiConfig>, AppError> {
    let row = sqlx::query_as::<_, (Option<String>, Option<String>, Option<String>)>(
        "SELECT ai_provider, ai_api_key_encrypted, ai_model FROM users WHERE github_login = $1",
    )
    .bind(login)
    .fetch_optional(pool)
    .await?;

    match row {
        Some((Some(provider), Some(encrypted_key), model)) => {
            let api_key = decrypt_secret(encryption_key, &encrypted_key)?;
            Ok(Some(UserAiConfig { provider, api_key, model }))
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
    model: Option<&str>,
) -> Result<(), AppError> {
    let encrypted = encrypt_secret(encryption_key, api_key)?;
    sqlx::query(
        "UPDATE users SET ai_provider = $1, ai_api_key_encrypted = $2, ai_model = $3 WHERE github_login = $4",
    )
    .bind(provider)
    .bind(&encrypted)
    .bind(model)
    .bind(login)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn clear_user_ai_config(pool: &PgPool, login: &str) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE users SET ai_provider = NULL, ai_api_key_encrypted = NULL, ai_model = NULL WHERE github_login = $1",
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
    pub model: Option<String>,
}

#[derive(Deserialize)]
pub struct SetAiSettingsRequest {
    pub provider: String,
    pub api_key: Option<String>,
    pub model: Option<String>,
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
            model: None,
        }));
    }

    match get_user_ai_config(&state.db, &state.config.secrets_encryption_key, &user.0.sub).await? {
        Some(cfg) => Ok(Json(AiSettingsResponse {
            provider: Some(cfg.provider),
            has_api_key: true,
            model: cfg.model,
        })),
        None => Ok(Json(AiSettingsResponse {
            provider: None,
            has_api_key: false,
            model: None,
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

    let api_key = req.api_key.as_deref().unwrap_or("").trim().to_string();

    if api_key.is_empty() {
        // No new key supplied — only update provider and model, keep existing key.
        sqlx::query(
            "UPDATE users SET ai_provider = $1, ai_model = $2 WHERE github_login = $3",
        )
        .bind(&req.provider)
        .bind(req.model.as_deref())
        .bind(&user.0.sub)
        .execute(&state.db)
        .await?;
    } else {
        set_user_ai_config(
            &state.db,
            &state.config.secrets_encryption_key,
            &user.0.sub,
            &req.provider,
            &api_key,
            req.model.as_deref(),
        )
        .await?;
    }

    Ok(Json(AiSettingsResponse {
        provider: Some(req.provider),
        has_api_key: true,
        model: req.model,
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
