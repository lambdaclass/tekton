use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::auth::{AdminUser, MemberUser};
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
            Ok(Some(UserAiConfig {
                provider,
                api_key,
                model,
            }))
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

// ── Global AI config CRUD helpers ──

pub async fn get_global_ai_config(
    pool: &PgPool,
    encryption_key: &str,
) -> Result<Option<UserAiConfig>, AppError> {
    let row = sqlx::query_as::<_, (String, String, Option<String>)>(
        "SELECT ai_provider, ai_api_key_encrypted, ai_model FROM global_ai_config WHERE id = 1",
    )
    .fetch_optional(pool)
    .await?;

    match row {
        Some((provider, encrypted_key, model)) => {
            let api_key = decrypt_secret(encryption_key, &encrypted_key)?;
            Ok(Some(UserAiConfig {
                provider,
                api_key,
                model,
            }))
        }
        None => Ok(None),
    }
}

pub async fn set_global_ai_config(
    pool: &PgPool,
    encryption_key: &str,
    provider: &str,
    api_key: &str,
    model: Option<&str>,
    updated_by: &str,
) -> Result<(), AppError> {
    let encrypted = encrypt_secret(encryption_key, api_key)?;
    sqlx::query(
        "INSERT INTO global_ai_config (id, ai_provider, ai_api_key_encrypted, ai_model, updated_by, updated_at)
         VALUES (1, $1, $2, $3, $4, NOW())
         ON CONFLICT (id) DO UPDATE SET ai_provider = $1, ai_api_key_encrypted = $2, ai_model = $3, updated_by = $4, updated_at = NOW()",
    )
    .bind(provider)
    .bind(&encrypted)
    .bind(model)
    .bind(updated_by)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn clear_global_ai_config(pool: &PgPool) -> Result<(), AppError> {
    sqlx::query("DELETE FROM global_ai_config WHERE id = 1")
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_global_fallback: Option<bool>,
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
            has_global_fallback: None,
        }));
    }

    match get_user_ai_config(&state.db, &state.config.secrets_encryption_key, &user.0.sub).await? {
        Some(cfg) => Ok(Json(AiSettingsResponse {
            provider: Some(cfg.provider),
            has_api_key: true,
            model: cfg.model,
            has_global_fallback: None,
        })),
        None => {
            // Check if a global fallback key exists
            let has_global = get_global_ai_config(&state.db, &state.config.secrets_encryption_key)
                .await?
                .is_some();
            Ok(Json(AiSettingsResponse {
                provider: None,
                has_api_key: false,
                model: None,
                has_global_fallback: Some(has_global),
            }))
        }
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
        sqlx::query("UPDATE users SET ai_provider = $1, ai_model = $2 WHERE github_login = $3")
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

    // Audit: admin.ai_settings_update
    crate::audit::log_event(
        &state.db,
        "admin.ai_settings_update",
        &user.0.sub,
        None,
        serde_json::json!({ "provider": &req.provider }),
        None,
    )
    .await;

    Ok(Json(AiSettingsResponse {
        provider: Some(req.provider),
        has_api_key: true,
        model: req.model,
        has_global_fallback: None,
    }))
}

/// DELETE /api/settings/ai
pub async fn delete_ai_settings(
    user: MemberUser,
    State(state): State<crate::AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    clear_user_ai_config(&state.db, &user.0.sub).await?;

    // Audit: admin.ai_settings_delete
    crate::audit::log_event(
        &state.db,
        "admin.ai_settings_delete",
        &user.0.sub,
        None,
        serde_json::json!({}),
        None,
    )
    .await;

    Ok(Json(serde_json::json!({ "deleted": true })))
}

// ── Admin: Global AI settings endpoints ──

/// GET /api/admin/settings/ai
pub async fn get_global_ai_settings(
    AdminUser(_claims): AdminUser,
    State(state): State<crate::AppState>,
) -> Result<Json<AiSettingsResponse>, AppError> {
    if state.config.secrets_encryption_key.is_empty() {
        return Ok(Json(AiSettingsResponse {
            provider: None,
            has_api_key: false,
            model: None,
            has_global_fallback: None,
        }));
    }

    match get_global_ai_config(&state.db, &state.config.secrets_encryption_key).await? {
        Some(cfg) => Ok(Json(AiSettingsResponse {
            provider: Some(cfg.provider),
            has_api_key: true,
            model: cfg.model,
            has_global_fallback: None,
        })),
        None => Ok(Json(AiSettingsResponse {
            provider: None,
            has_api_key: false,
            model: None,
            has_global_fallback: None,
        })),
    }
}

/// PUT /api/admin/settings/ai
pub async fn put_global_ai_settings(
    admin: AdminUser,
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
            "UPDATE global_ai_config SET ai_provider = $1, ai_model = $2, updated_by = $3, updated_at = NOW() WHERE id = 1",
        )
        .bind(&req.provider)
        .bind(req.model.as_deref())
        .bind(&admin.0.sub)
        .execute(&state.db)
        .await?;
    } else {
        set_global_ai_config(
            &state.db,
            &state.config.secrets_encryption_key,
            &req.provider,
            &api_key,
            req.model.as_deref(),
            &admin.0.sub,
        )
        .await?;
    }

    crate::audit::log_event(
        &state.db,
        "admin.global_ai_settings_update",
        &admin.0.sub,
        None,
        serde_json::json!({ "provider": &req.provider }),
        None,
    )
    .await;

    Ok(Json(AiSettingsResponse {
        provider: Some(req.provider),
        has_api_key: true,
        model: req.model,
        has_global_fallback: None,
    }))
}

/// DELETE /api/admin/settings/ai
pub async fn delete_global_ai_settings(
    admin: AdminUser,
    State(state): State<crate::AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    clear_global_ai_config(&state.db).await?;

    crate::audit::log_event(
        &state.db,
        "admin.global_ai_settings_delete",
        &admin.0.sub,
        None,
        serde_json::json!({}),
        None,
    )
    .await;

    Ok(Json(serde_json::json!({ "deleted": true })))
}
