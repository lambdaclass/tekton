use std::net::SocketAddr;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use axum::extract::connect_info::ConnectInfo;
use axum::extract::{Path, Query, State};
use axum::Json;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use rand::RngCore;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use sqlx::PgPool;

use crate::auth::AdminUser;
use crate::error::AppError;
use crate::models::{CreateSecretRequest, SecretEntry};

// ── Encryption helpers ──

/// Derive a 32-byte key from the config key using SHA-256.
fn derive_key(raw: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(raw.as_bytes());
    let result = hasher.finalize();
    let mut key = [0u8; 32];
    key.copy_from_slice(&result);
    key
}

pub fn encrypt_secret(key_str: &str, plaintext: &str) -> Result<String, AppError> {
    let key_bytes = derive_key(key_str);
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| AppError::Internal(format!("Encryption failed: {e}")))?;
    // Prepend nonce to ciphertext, then base64 encode
    let mut combined = nonce_bytes.to_vec();
    combined.extend_from_slice(&ciphertext);
    Ok(BASE64.encode(&combined))
}

pub fn decrypt_secret(key_str: &str, encrypted: &str) -> Result<String, AppError> {
    let key_bytes = derive_key(key_str);
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);
    let combined = BASE64
        .decode(encrypted)
        .map_err(|e| AppError::Internal(format!("Base64 decode failed: {e}")))?;
    if combined.len() < 12 {
        return Err(AppError::Internal("Invalid encrypted data".into()));
    }
    let (nonce_bytes, ciphertext) = combined.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| AppError::Internal(format!("Decryption failed: {e}")))?;
    String::from_utf8(plaintext)
        .map_err(|e| AppError::Internal(format!("UTF-8 decode failed: {e}")))
}

// ── Admin API handlers ──

#[derive(Debug, Deserialize)]
pub struct ListSecretsQuery {
    pub repo: Option<String>,
}

/// GET /api/admin/secrets?repo=optional
pub async fn list_secrets(
    _admin: AdminUser,
    State(state): State<crate::AppState>,
    Query(params): Query<ListSecretsQuery>,
) -> Result<Json<Vec<SecretEntry>>, AppError> {
    let secrets = if let Some(ref repo) = params.repo {
        sqlx::query_as::<_, SecretEntry>(
            "SELECT id, repo, name, created_by, \
             TO_CHAR(created_at, 'YYYY-MM-DD HH24:MI:SS') as created_at \
             FROM secrets WHERE repo = $1 ORDER BY repo, name",
        )
        .bind(repo)
        .fetch_all(&state.db)
        .await?
    } else {
        sqlx::query_as::<_, SecretEntry>(
            "SELECT id, repo, name, created_by, \
             TO_CHAR(created_at, 'YYYY-MM-DD HH24:MI:SS') as created_at \
             FROM secrets ORDER BY repo, name",
        )
        .fetch_all(&state.db)
        .await?
    };
    Ok(Json(secrets))
}

/// POST /api/admin/secrets
pub async fn create_secret(
    admin: AdminUser,
    State(state): State<crate::AppState>,
    Json(req): Json<CreateSecretRequest>,
) -> Result<Json<SecretEntry>, AppError> {
    if state.config.secrets_encryption_key.is_empty() {
        return Err(AppError::BadRequest(
            "SECRETS_ENCRYPTION_KEY is not configured".into(),
        ));
    }

    let encrypted = encrypt_secret(&state.config.secrets_encryption_key, &req.value)?;

    sqlx::query(
        "INSERT INTO secrets (repo, name, encrypted_value, created_by) \
         VALUES ($1, $2, $3, $4) \
         ON CONFLICT (repo, name) DO UPDATE SET \
           encrypted_value = EXCLUDED.encrypted_value, \
           created_by = EXCLUDED.created_by, \
           created_at = NOW()",
    )
    .bind(&req.repo)
    .bind(&req.name)
    .bind(&encrypted)
    .bind(&admin.0.sub)
    .execute(&state.db)
    .await?;

    let entry = sqlx::query_as::<_, SecretEntry>(
        "SELECT id, repo, name, created_by, \
         TO_CHAR(created_at, 'YYYY-MM-DD HH24:MI:SS') as created_at \
         FROM secrets WHERE repo = $1 AND name = $2",
    )
    .bind(&req.repo)
    .bind(&req.name)
    .fetch_one(&state.db)
    .await?;

    // Audit: admin.secret_create
    crate::audit::log_event(
        &state.db,
        "admin.secret_create",
        &admin.0.sub,
        Some(&req.repo),
        serde_json::json!({ "name": &req.name }),
        None,
    )
    .await;

    Ok(Json(entry))
}

/// DELETE /api/admin/secrets/{id}
pub async fn delete_secret(
    _admin: AdminUser,
    State(state): State<crate::AppState>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Fetch secret metadata before deleting so we can log it
    let secret_info = sqlx::query_as::<_, (String, String)>(
        "SELECT repo, name FROM secrets WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?;

    let result = sqlx::query("DELETE FROM secrets WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Secret not found".into()));
    }

    // Audit: admin.secret_delete
    if let Some((repo, name)) = secret_info {
        crate::audit::log_event(
            &state.db,
            "admin.secret_delete",
            &_admin.0.sub,
            Some(&repo),
            serde_json::json!({ "name": name, "secret_id": id }),
            None,
        )
        .await;
    }

    Ok(Json(serde_json::json!({ "deleted": true })))
}

// ── Internal endpoint for preview.sh ──

/// GET /internal/secrets/{owner}/{repo}
/// Localhost-only endpoint for preview.sh to fetch decrypted secrets.
/// Returns plain text with one KEY=VALUE per line.
pub async fn internal_list_secrets(
    State(state): State<crate::AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Path((owner, repo)): Path<(String, String)>,
) -> Result<String, AppError> {
    if !addr.ip().is_loopback() {
        return Err(AppError::Forbidden(
            "This endpoint is only accessible from localhost".into(),
        ));
    }

    let full_repo = format!("{owner}/{repo}");
    let secrets =
        load_secrets_for_repo(&state.db, &state.config.secrets_encryption_key, &full_repo).await?;

    let body = secrets
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("\n");

    Ok(body)
}

// ── Helper for pipeline use ──

/// Load and decrypt all secrets for a repo (repo-specific + global '*').
/// Returns Vec of (name, decrypted_value) pairs.
pub async fn load_secrets_for_repo(
    db: &PgPool,
    encryption_key: &str,
    repo: &str,
) -> Result<Vec<(String, String)>, AppError> {
    if encryption_key.is_empty() {
        return Ok(vec![]);
    }

    let rows = sqlx::query_as::<_, (String, String)>(
        "SELECT name, encrypted_value FROM secrets WHERE repo = $1 OR repo = '*' ORDER BY name",
    )
    .bind(repo)
    .fetch_all(db)
    .await?;

    let mut secrets = Vec::with_capacity(rows.len());
    for (name, encrypted_value) in rows {
        match decrypt_secret(encryption_key, &encrypted_value) {
            Ok(value) => secrets.push((name, value)),
            Err(e) => {
                tracing::warn!("Failed to decrypt secret '{name}': {e}");
            }
        }
    }
    Ok(secrets)
}
