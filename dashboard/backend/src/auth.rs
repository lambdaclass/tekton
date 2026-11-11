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
use crate::models::{Claims, GitHubTokenResponse, GitHubUserInfo, UserInfo};
use crate::AppState;

const COOKIE_NAME: &str = "dashboard_session";
const JWT_EXPIRY_SECS: usize = 86400 * 7; // 7 days

/// Try to decode the JWT from the session cookie. Returns None on any failure.
fn extract_user_from_jar(jar: &CookieJar, state: &AppState) -> Option<Claims> {
    let token = jar.get(COOKIE_NAME)?.value();
    let key = DecodingKey::from_secret(state.config.jwt_secret.as_bytes());
    decode::<Claims>(token, &key, &Validation::default())
        .ok()
        .map(|data| data.claims)
}

// ── Stored GitHub tokens ──

/// The stored GitHub token for `login`, decrypted. The only way the backend
/// reads a user's GitHub token. A user who doesn't exist, has no token, or has
/// one that won't decrypt (the key changed) is an `AppError::Auth`: signing in
/// again stores a fresh one.
pub async fn load_github_token(db: &PgPool, key: &str, login: &str) -> Result<String, AppError> {
    let stored: Option<Option<String>> =
        sqlx::query_scalar("SELECT github_token_encrypted FROM users WHERE github_login = $1")
            .bind(login)
            .fetch_optional(db)
            .await?;
    decrypt_github_token(key, login, stored.flatten()).ok_or_else(|| {
        AppError::Auth(format!(
            "No usable GitHub token on file for '{login}'; sign in again"
        ))
    })
}

/// [`load_github_token`] for a request handler.
pub async fn github_token_for(state: &AppState, login: &str) -> Result<String, AppError> {
    load_github_token(&state.db, &state.config.secrets_encryption_key, login).await
}

/// The token of an intake or Sentry source's `run_as_user`. Their missing
/// token isn't the caller's session problem, so it becomes `wrap(message)`:
/// `Internal` for background jobs, `BadRequest` for admin pages.
pub async fn load_run_as_user_token(
    db: &PgPool,
    key: &str,
    login: &str,
    wrap: fn(String) -> AppError,
) -> Result<String, AppError> {
    load_github_token(db, key, login)
        .await
        .map_err(|e| match e {
            AppError::Auth(_) => wrap(format!(
                "run_as_user '{login}' not found or has no usable GitHub token; they must sign in again"
            )),
            other => other,
        })
}

fn decrypt_github_token(key: &str, login: &str, encrypted: Option<String>) -> Option<String> {
    let encrypted = encrypted.filter(|e| !e.is_empty())?;
    match crate::secrets::decrypt_secret(key, &encrypted) {
        Ok(token) => Some(token).filter(|t| !t.is_empty()),
        Err(e) => {
            tracing::warn!("Stored GitHub token for {login} could not be decrypted: {e}");
            None
        }
    }
}

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
            .ok_or_else(|| (StatusCode::UNAUTHORIZED, "Not authenticated").into_response())?;

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
         client_id={}&redirect_uri={}&scope=repo%20read:org%20admin:repo_hook&allow_signup=false",
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
    let name = user_info.name.unwrap_or_else(|| user_info.login.clone());

    // Determine role for new users: first user ever gets 'admin', others get 'member'
    let user_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(&state.db)
        .await?;
    let role_for_new_user = if user_count.0 == 0 { "admin" } else { "member" };

    let encrypted_token = crate::secrets::encrypt_secret(
        &state.config.secrets_encryption_key,
        &token_resp.access_token,
    )?;
    sqlx::query(
        "INSERT INTO users (github_login, name, email, github_token_encrypted, role)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT(github_login) DO UPDATE SET
           name = EXCLUDED.name,
           email = EXCLUDED.email,
           github_token_encrypted = EXCLUDED.github_token_encrypted,
           github_token = NULL,
           updated_at = NOW()",
    )
    .bind(&user_info.login)
    .bind(&name)
    .bind(&user_info.email)
    .bind(&encrypted_token)
    .bind(role_for_new_user)
    .execute(&state.db)
    .await?;

    // Fetch the user's current role (preserved on conflict, so we read it back)
    let (role,): (String,) = sqlx::query_as("SELECT role FROM users WHERE github_login = $1")
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

    // Audit: auth.login
    crate::audit::log_event(
        &state.db,
        "auth.login",
        &claims.sub,
        None,
        serde_json::json!({ "role": &claims.role }),
        None,
    )
    .await;

    Ok((jar.add(cookie), Redirect::temporary("/")))
}

pub async fn logout(State(state): State<AppState>, jar: CookieJar) -> (CookieJar, Redirect) {
    // Try to extract the user from the cookie for audit logging
    if let Some(claims) = extract_user_from_jar(&jar, &state) {
        crate::audit::log_event(
            &state.db,
            "auth.logout",
            &claims.sub,
            None,
            serde_json::json!({}),
            None,
        )
        .await;
    }
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

// ── Test-only login endpoint ──

#[derive(Deserialize)]
pub struct TestLoginRequest {
    pub login: String,
    pub role: String,
}

/// POST /api/auth/test-login — only available when TEST_MODE=true.
/// Accepts { "login": "testadmin", "role": "admin" } and returns a valid
/// JWT cookie identical to what the real OAuth callback produces.
pub async fn test_login(
    State(state): State<AppState>,
    jar: CookieJar,
    axum::Json(req): axum::Json<TestLoginRequest>,
) -> Result<(CookieJar, axum::Json<serde_json::Value>), AppError> {
    let exp = chrono::Utc::now().timestamp() as usize + JWT_EXPIRY_SECS;
    let claims = Claims {
        sub: req.login.clone(),
        name: req.login.clone(),
        role: req.role.clone(),
        exp,
    };
    let key = EncodingKey::from_secret(state.config.jwt_secret.as_bytes());
    let token = encode(&Header::default(), &claims, &key)
        .map_err(|e| AppError::Internal(format!("JWT encode error: {e}")))?;

    let cookie = Cookie::build((COOKIE_NAME, token))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .secure(false)
        .max_age(time::Duration::days(7));

    Ok((
        jar.add(cookie),
        axum::Json(serde_json::json!({
            "login": req.login,
            "role": req.role,
        })),
    ))
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

/// Check if a user can access a task.
/// Access is granted if the user is an admin, created the task, or has
/// permission to the task's repo (via org membership or explicit grant).
pub async fn check_task_ownership(
    db: &PgPool,
    task_id: &str,
    github_login: &str,
    role: &str,
    github_org: &str,
) -> Result<(), AppError> {
    if role == "admin" {
        return Ok(());
    }

    let row: Option<(String, String)> =
        sqlx::query_as("SELECT created_by, repo FROM tasks WHERE id = $1")
            .bind(task_id)
            .fetch_optional(db)
            .await?;

    let (created_by, repo) = match row {
        Some(r) => r,
        None => {
            return Err(AppError::Forbidden(
                "You do not have permission to access this task".into(),
            ));
        }
    };

    // Allow if user created the task
    if created_by == github_login {
        return Ok(());
    }

    // Allow if user has access to the task's repo
    if check_repo_permission(db, github_login, &repo, role, github_org)
        .await
        .is_ok()
    {
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
    let rows: Vec<(String, String, String)> =
        sqlx::query_as("SELECT github_login, name, role FROM users ORDER BY created_at ASC")
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

    let result =
        sqlx::query("UPDATE users SET role = $1, updated_at = NOW() WHERE github_login = $2")
            .bind(&req.role)
            .bind(&login)
            .execute(&state.db)
            .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound(format!("User '{login}' not found")));
    }

    // Audit: admin.role_change
    crate::audit::log_event(
        &state.db,
        "admin.role_change",
        &_claims.sub,
        Some(&login),
        serde_json::json!({ "new_role": &req.role }),
        None,
    )
    .await;

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
    let exists: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users WHERE github_login = $1")
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
        sqlx::query("INSERT INTO user_repo_permissions (github_login, repo) VALUES ($1, $2)")
            .bind(&login)
            .bind(repo)
            .execute(&state.db)
            .await?;
    }

    // Audit: admin.user_repos_changed
    crate::audit::log_event(
        &state.db,
        "admin.user_repos_changed",
        &_claims.sub,
        Some(&login),
        serde_json::json!({ "repos": &req.repos }),
        None,
    )
    .await;

    Ok(axum::Json(req.repos))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_token_roundtrips_through_encryption() {
        let enc = crate::secrets::encrypt_secret("key-a", "gho_secret").unwrap();
        assert!(!enc.contains("gho_secret"));
        assert_eq!(
            decrypt_github_token("key-a", "u", Some(enc)),
            Some("gho_secret".into())
        );
    }

    #[test]
    fn missing_github_token_is_none() {
        assert_eq!(decrypt_github_token("key-a", "u", None), None);
        assert_eq!(
            decrypt_github_token("key-a", "u", Some(String::new())),
            None
        );
    }

    #[test]
    fn undecryptable_github_token_is_none() {
        let enc = crate::secrets::encrypt_secret("key-a", "gho_secret").unwrap();
        assert_eq!(decrypt_github_token("key-b", "u", Some(enc)), None);
        assert_eq!(
            decrypt_github_token("key-a", "u", Some("not-base64!".into())),
            None
        );
    }
}
