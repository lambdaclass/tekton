use std::net::SocketAddr;

use axum::extract::connect_info::ConnectInfo;
use axum::extract::{FromRequestParts, Query, State};
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use axum_extra::extract::cookie::{Cookie, SameSite};
use axum_extra::extract::CookieJar;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::Deserialize;
use sqlx::PgPool;

use crate::error::AppError;
use crate::models::{Claims, GitHubTokenResponse, GitHubUserInfo, SetSshKeyRequest, SshKeyResponse, UserInfo};
use crate::AppState;

const COOKIE_NAME: &str = "dashboard_session";
const JWT_EXPIRY_SECS: usize = 86400 * 7; // 7 days

// ── Extractor: authenticated user ──

pub struct AuthUser(pub Claims);

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let jar = CookieJar::from_request_parts(parts, state)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())?;

        let token = jar
            .get(COOKIE_NAME)
            .map(|c| c.value().to_string())
            .ok_or_else(|| {
                (StatusCode::UNAUTHORIZED, "Not authenticated").into_response()
            })?;

        let key = DecodingKey::from_secret(state.config.jwt_secret.as_bytes());
        let data = decode::<Claims>(&token, &key, &Validation::default())
            .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid session").into_response())?;

        Ok(AuthUser(data.claims))
    }
}

// ── Handlers ──

#[derive(Deserialize)]
pub struct CallbackQuery {
    pub code: String,
}

pub async fn login(State(state): State<AppState>) -> Redirect {
    let url = format!(
        "https://github.com/login/oauth/authorize?\
         client_id={}&redirect_uri={}&scope=repo%20read:org&allow_signup=false",
        state.config.github_client_id,
        urlencoding::encode(&state.config.github_redirect_uri),
    );
    Redirect::temporary(&url)
}

pub async fn callback(
    State(state): State<AppState>,
    Query(query): Query<CallbackQuery>,
    jar: CookieJar,
) -> Result<(CookieJar, Redirect), AppError> {
    let client = reqwest::Client::new();

    // Exchange code for access token
    let token_resp: GitHubTokenResponse = client
        .post("https://github.com/login/oauth/access_token")
        .header("Accept", "application/json")
        .form(&[
            ("code", query.code.as_str()),
            ("client_id", state.config.github_client_id.as_str()),
            ("client_secret", state.config.github_client_secret.as_str()),
            ("redirect_uri", state.config.github_redirect_uri.as_str()),
        ])
        .send()
        .await?
        .json()
        .await?;

    // Get user info
    let user_info: GitHubUserInfo = client
        .get("https://api.github.com/user")
        .header("User-Agent", "dashboard")
        .bearer_auth(&token_resp.access_token)
        .send()
        .await?
        .json()
        .await?;

    // Check org membership
    let org_check = client
        .get(format!(
            "https://api.github.com/orgs/{}/members/{}",
            state.config.github_org, user_info.login
        ))
        .header("User-Agent", "dashboard")
        .bearer_auth(&token_resp.access_token)
        .send()
        .await?;

    if org_check.status() != StatusCode::NO_CONTENT {
        return Err(AppError::Auth(format!(
            "User '{}' is not a member of the '{}' organization",
            user_info.login, state.config.github_org
        )));
    }

    // Upsert into users table
    let name = user_info
        .name
        .unwrap_or_else(|| user_info.login.clone());

    // Determine role for new users: first user ever gets 'admin', others get 'member'
    let user_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(&state.db)
        .await?;
    let role_for_new_user = if user_count.0 == 0 {
        "admin"
    } else {
        "member"
    };

    sqlx::query(
        "INSERT INTO users (github_login, name, email, github_token, role)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT(github_login) DO UPDATE SET
           name = EXCLUDED.name,
           email = EXCLUDED.email,
           github_token = EXCLUDED.github_token,
           updated_at = NOW()",
    )
    .bind(&user_info.login)
    .bind(&name)
    .bind(&user_info.email)
    .bind(&token_resp.access_token)
    .bind(role_for_new_user)
    .execute(&state.db)
    .await?;

    // Fetch the user's current role (preserved on conflict, so we read it back)
    let (role,): (String,) =
        sqlx::query_as("SELECT role FROM users WHERE github_login = $1")
            .bind(&user_info.login)
            .fetch_one(&state.db)
            .await?;

    // Issue JWT
    let exp = chrono::Utc::now().timestamp() as usize + JWT_EXPIRY_SECS;
    let claims = Claims {
        sub: user_info.login,
        name,
        role,
        exp,
    };
    let key = EncodingKey::from_secret(state.config.jwt_secret.as_bytes());
    let token = encode(&Header::default(), &claims, &key)
        .map_err(|e| AppError::Internal(format!("JWT encode error: {e}")))?;

    let cookie = Cookie::build((COOKIE_NAME, token))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .secure(true)
        .max_age(time::Duration::days(7));

    Ok((jar.add(cookie), Redirect::temporary("/")))
}

pub async fn logout(jar: CookieJar) -> (CookieJar, Redirect) {
    let cookie = Cookie::build((COOKIE_NAME, ""))
        .path("/")
        .max_age(time::Duration::ZERO);
    (jar.remove(cookie), Redirect::temporary("/"))
}

pub async fn me(AuthUser(claims): AuthUser) -> axum::Json<UserInfo> {
    axum::Json(UserInfo {
        login: claims.sub,
        name: claims.name,
        role: claims.role,
    })
}

// ── SSH key handlers ──

/// GET /api/auth/ssh-key — get the current user's SSH public key.
pub async fn get_ssh_key(
    AuthUser(claims): AuthUser,
    State(state): State<AppState>,
) -> Result<axum::Json<SshKeyResponse>, AppError> {
    let key: Option<String> =
        sqlx::query_scalar("SELECT ssh_public_key FROM users WHERE github_login = $1")
            .bind(&claims.sub)
            .fetch_one(&state.db)
            .await?;

    Ok(axum::Json(SshKeyResponse { ssh_public_key: key }))
}

/// PUT /api/auth/ssh-key — set the current user's SSH public key.
pub async fn set_ssh_key(
    AuthUser(claims): AuthUser,
    State(state): State<AppState>,
    axum::Json(req): axum::Json<SetSshKeyRequest>,
) -> Result<axum::Json<SshKeyResponse>, AppError> {
    let trimmed = req.ssh_public_key.trim().to_string();

    // Basic validation: must start with a known SSH key prefix
    if !trimmed.is_empty() {
        let valid_prefixes = ["ssh-rsa", "ssh-ed25519", "ecdsa-sha2-", "ssh-dss", "sk-ssh-ed25519", "sk-ecdsa-sha2-"];
        if !valid_prefixes.iter().any(|p| trimmed.starts_with(p)) {
            return Err(AppError::BadRequest(
                "Invalid SSH public key format. Must start with ssh-rsa, ssh-ed25519, ecdsa-sha2-*, etc.".into(),
            ));
        }
    }

    let key_to_store: Option<&str> = if trimmed.is_empty() { None } else { Some(&trimmed) };

    sqlx::query("UPDATE users SET ssh_public_key = $1, updated_at = NOW() WHERE github_login = $2")
        .bind(key_to_store)
        .bind(&claims.sub)
        .execute(&state.db)
        .await?;

    Ok(axum::Json(SshKeyResponse {
        ssh_public_key: key_to_store.map(String::from),
    }))
}

/// DELETE /api/auth/ssh-key — remove the current user's SSH public key.
pub async fn delete_ssh_key(
    AuthUser(claims): AuthUser,
    State(state): State<AppState>,
) -> Result<axum::Json<SshKeyResponse>, AppError> {
    sqlx::query("UPDATE users SET ssh_public_key = NULL, updated_at = NOW() WHERE github_login = $1")
        .bind(&claims.sub)
        .execute(&state.db)
        .await?;

    Ok(axum::Json(SshKeyResponse { ssh_public_key: None }))
}

/// GET /internal/ssh-keys — localhost-only endpoint for preview.sh.
/// Returns one SSH public key per line (all users that have set a key).
pub async fn internal_list_ssh_keys(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Result<String, AppError> {
    if !addr.ip().is_loopback() {
        return Err(AppError::Forbidden(
            "This endpoint is only accessible from localhost".into(),
        ));
    }

    let keys: Vec<String> = sqlx::query_scalar(
        "SELECT ssh_public_key FROM users WHERE ssh_public_key IS NOT NULL AND ssh_public_key != ''",
    )
    .fetch_all(&state.db)
    .await?;

    Ok(keys.join("\n"))
}

// ── Extractor: admin user ──

pub struct AdminUser(pub Claims);

impl FromRequestParts<AppState> for AdminUser {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let AuthUser(claims) = AuthUser::from_request_parts(parts, state).await?;

        if claims.role != "admin" {
            return Err((StatusCode::FORBIDDEN, "Admin access required").into_response());
        }

        Ok(AdminUser(claims))
    }
}

// ── Extractor: member user (admin or member) ──

pub struct MemberUser(pub Claims);

impl FromRequestParts<AppState> for MemberUser {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let AuthUser(claims) = AuthUser::from_request_parts(parts, state).await?;

        if claims.role != "admin" && claims.role != "member" {
            return Err((StatusCode::FORBIDDEN, "Member access required").into_response());
        }

        Ok(MemberUser(claims))
    }
}

// ── Permission helpers ──

/// Check if a user has permission to access a repo.
/// Admins always have access. Members automatically have access to repos under
/// the configured GitHub org. Others need an explicit entry in user_repo_permissions.
pub async fn check_repo_permission(
    db: &PgPool,
    github_login: &str,
    repo: &str,
    role: &str,
    github_org: &str,
) -> Result<(), AppError> {
    if role == "admin" {
        return Ok(());
    }

    // Members of the org automatically have access to all repos under that org
    if role == "member" && repo.starts_with(&format!("{github_org}/")) {
        return Ok(());
    }

    let has_perm: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM user_repo_permissions WHERE github_login = $1 AND repo = $2",
    )
    .bind(github_login)
    .bind(repo)
    .fetch_one(db)
    .await?;

    if has_perm.0 > 0 {
        return Ok(());
    }

    Err(AppError::Forbidden(format!(
        "No permission to access repo '{repo}'"
    )))
}

/// Check if a user owns a task (or is an admin).
pub async fn check_task_ownership(
    db: &PgPool,
    task_id: &str,
    github_login: &str,
    role: &str,
) -> Result<(), AppError> {
    if role == "admin" {
        return Ok(());
    }

    let created_by: Option<String> =
        sqlx::query_scalar("SELECT created_by FROM tasks WHERE id = $1")
            .bind(task_id)
            .fetch_optional(db)
            .await?
            .flatten();

    if created_by.as_deref() == Some(github_login) {
        return Ok(());
    }

    Err(AppError::Forbidden(
        "You do not have permission to access this task".into(),
    ))
}

// ── Admin handlers ──

pub async fn list_users(
    AdminUser(_claims): AdminUser,
    State(state): State<AppState>,
) -> Result<axum::Json<Vec<serde_json::Value>>, AppError> {
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT github_login, name, role FROM users ORDER BY created_at ASC",
    )
    .fetch_all(&state.db)
    .await?;

    let users: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|(login, name, role)| {
            serde_json::json!({
                "login": login,
                "name": name,
                "role": role,
            })
        })
        .collect();

    Ok(axum::Json(users))
}

pub async fn set_user_role(
    AdminUser(_claims): AdminUser,
    State(state): State<AppState>,
    axum::extract::Path(login): axum::extract::Path<String>,
    axum::Json(req): axum::Json<crate::models::SetUserRoleRequest>,
) -> Result<axum::Json<serde_json::Value>, AppError> {
    if req.role != "admin" && req.role != "member" && req.role != "viewer" {
        return Err(AppError::BadRequest(format!(
            "Invalid role '{}'. Must be 'admin', 'member', or 'viewer'",
            req.role
        )));
    }

    let result = sqlx::query("UPDATE users SET role = $1, updated_at = NOW() WHERE github_login = $2")
        .bind(&req.role)
        .bind(&login)
        .execute(&state.db)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound(format!("User '{login}' not found")));
    }

    Ok(axum::Json(serde_json::json!({
        "login": login,
        "role": req.role,
    })))
}

pub async fn get_user_repos(
    AdminUser(_claims): AdminUser,
    State(state): State<AppState>,
    axum::extract::Path(login): axum::extract::Path<String>,
) -> Result<axum::Json<Vec<String>>, AppError> {
    let repos: Vec<String> = sqlx::query_scalar(
        "SELECT repo FROM user_repo_permissions WHERE github_login = $1 ORDER BY repo",
    )
    .bind(&login)
    .fetch_all(&state.db)
    .await?;

    Ok(axum::Json(repos))
}

#[derive(Deserialize)]
pub struct SetUserReposRequest {
    pub repos: Vec<String>,
}

pub async fn set_user_repos(
    AdminUser(_claims): AdminUser,
    State(state): State<AppState>,
    axum::extract::Path(login): axum::extract::Path<String>,
    axum::Json(req): axum::Json<SetUserReposRequest>,
) -> Result<axum::Json<Vec<String>>, AppError> {
    // Verify user exists
    let exists: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM users WHERE github_login = $1")
            .bind(&login)
            .fetch_one(&state.db)
            .await?;
    if exists.0 == 0 {
        return Err(AppError::NotFound(format!("User '{login}' not found")));
    }

    // Delete existing permissions and insert new ones
    sqlx::query("DELETE FROM user_repo_permissions WHERE github_login = $1")
        .bind(&login)
        .execute(&state.db)
        .await?;

    for repo in &req.repos {
        sqlx::query(
            "INSERT INTO user_repo_permissions (github_login, repo) VALUES ($1, $2)",
        )
        .bind(&login)
        .bind(repo)
        .execute(&state.db)
        .await?;
    }

    Ok(axum::Json(req.repos))
}
