use axum::extract::{FromRequestParts, Query, State};
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use axum_extra::extract::cookie::{Cookie, SameSite};
use axum_extra::extract::CookieJar;
use dashmap::DashMap;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::Deserialize;
use sqlx::PgPool;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use crate::error::AppError;
use crate::models::{Claims, GitHubTokenResponse, GitHubUserInfo, UserInfo};
use crate::AppState;

const COOKIE_NAME: &str = "dashboard_session";
const JWT_EXPIRY_SECS: usize = 86400 * 7; // 7 days
const OAUTH_STATE_COOKIE: &str = "oauth_state";
const OAUTH_STATE_PATH: &str = "/api/auth/callback";

/// Try to decode the JWT from the session cookie. Returns None on any failure.
fn extract_user_from_jar(jar: &CookieJar, state: &AppState) -> Option<Claims> {
    let token = jar.get(COOKIE_NAME)?.value();
    let key = DecodingKey::from_secret(state.config.jwt_secret.as_bytes());
    decode::<Claims>(token, &key, &Validation::default())
        .ok()
        .map(|data| data.claims)
}

/// True when the backend runs with `TEST_MODE=true` (e2e suite only).
pub fn test_mode_enabled() -> bool {
    static TEST_MODE: LazyLock<bool> =
        LazyLock::new(|| std::env::var("TEST_MODE").as_deref() == Ok("true"));
    *TEST_MODE
}

// ── GitHub org membership ──

/// How recent the last successful membership check must be for a request.
#[derive(Clone, Copy)]
struct MembershipFreshness {
    /// Re-check with GitHub once the last successful check is older than this.
    max_age_secs: i64,
    /// When GitHub can't be reached, keep honoring a successful check this recent.
    grace_secs: i64,
}

/// Any authenticated request.
const AUTH_FRESHNESS: MembershipFreshness = MembershipFreshness {
    max_age_secs: 10 * 60,
    grace_secs: 60 * 60,
};
/// Member and admin actions.
const PRIVILEGED_FRESHNESS: MembershipFreshness = MembershipFreshness {
    max_age_secs: 2 * 60,
    grace_secs: 10 * 60,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Membership {
    Member,
    NotMember,
    TokenRevoked,
}

/// Maps the status of `GET /orgs/{org}/members/{login}` to an outcome. `None`
/// means the answer says nothing definite (rate limit, outage, SSO, ...).
fn membership_from_status(status: reqwest::StatusCode) -> Option<Membership> {
    match status {
        reqwest::StatusCode::NO_CONTENT => Some(Membership::Member),
        // 302: requester isn't an org member, GitHub points to public_members.
        reqwest::StatusCode::NOT_FOUND | reqwest::StatusCode::FOUND => Some(Membership::NotMember),
        reqwest::StatusCode::UNAUTHORIZED => Some(Membership::TokenRevoked),
        _ => None,
    }
}

static MEMBERSHIP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(10))
        .build()
        .expect("failed to build GitHub HTTP client")
});

/// Asks GitHub whether `login` is a member of `org`, using `token`. Any answer
/// other than a definite yes/no/revoked is an `Err` and should be treated as
/// transient.
pub async fn check_org_membership(
    token: &str,
    org: &str,
    login: &str,
) -> Result<Membership, AppError> {
    let resp = MEMBERSHIP_CLIENT
        .get(format!("https://api.github.com/orgs/{org}/members/{login}"))
        .header("User-Agent", "dashboard")
        .bearer_auth(token)
        .send()
        .await?;
    let status = resp.status();
    membership_from_status(status).ok_or_else(|| {
        AppError::Internal(format!(
            "Unexpected status {status} from GitHub membership check"
        ))
    })
}

/// The stored GitHub token for `login`, or `None` when the user has none.
/// The only place the extractors read the token from.
pub async fn load_github_token(db: &PgPool, login: &str) -> Result<Option<String>, AppError> {
    let token: Option<String> =
        sqlx::query_scalar("SELECT github_token FROM users WHERE github_login = $1")
            .bind(login)
            .fetch_optional(db)
            .await?;
    Ok(token.filter(|t| !t.is_empty()))
}

// ── Extractors ──

fn unauthorized(msg: &'static str) -> Response {
    (StatusCode::UNAUTHORIZED, msg).into_response()
}

fn db_failure(e: impl Into<AppError>) -> Response {
    e.into().into_response()
}

/// Seconds since the last successful membership check (`None` = never).
const MEMBERSHIP_AGE_SQL: &str =
    "EXTRACT(EPOCH FROM NOW() - membership_checked_at)::BIGINT AS membership_age";

/// Decodes the session cookie and validates it against the `users` row, with
/// one query: the user must exist and the session version must match. The
/// returned claims carry the role from the DB, never the one in the JWT.
async fn load_session(
    parts: &mut Parts,
    state: &AppState,
) -> Result<(Claims, Option<i64>), Response> {
    let jar = CookieJar::from_request_parts(parts, state)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())?;

    let token = jar
        .get(COOKIE_NAME)
        .map(|c| c.value().to_string())
        .ok_or_else(|| unauthorized("Not authenticated"))?;

    let key = DecodingKey::from_secret(state.config.jwt_secret.as_bytes());
    let mut claims = decode::<Claims>(&token, &key, &Validation::default())
        .map_err(|_| unauthorized("Invalid session"))?
        .claims;

    let row: Option<(String, i32, Option<i64>)> = sqlx::query_as(&format!(
        "SELECT role, session_version, {MEMBERSHIP_AGE_SQL} FROM users WHERE github_login = $1"
    ))
    .bind(&claims.sub)
    .fetch_optional(&state.db)
    .await
    .map_err(db_failure)?;

    let Some((role, session_version, membership_age)) = row else {
        return Err(unauthorized("Session no longer valid"));
    };
    if session_version != claims.sv {
        return Err(unauthorized("Session no longer valid"));
    }
    claims.role = role;
    Ok((claims, membership_age))
}

/// One in-flight membership re-check per user; concurrent requests wait for it
/// and then see the fresh timestamp instead of each calling GitHub.
static MEMBERSHIP_CHECKS: LazyLock<DashMap<String, Arc<tokio::sync::Mutex<()>>>> =
    LazyLock::new(DashMap::new);

/// Makes sure the user's org membership was confirmed within `freshness.max_age_secs`,
/// re-checking with GitHub when it wasn't.
async fn ensure_membership_fresh(
    state: &AppState,
    claims: &Claims,
    membership_age: Option<i64>,
    freshness: MembershipFreshness,
) -> Result<(), Response> {
    let is_fresh = |age: Option<i64>| age.is_some_and(|a| a <= freshness.max_age_secs);
    // The e2e suite (TEST_MODE) mints sessions for seeded users with fake
    // tokens; it already lets anyone mint any session, so skipping GitHub
    // there costs nothing.
    if is_fresh(membership_age) || test_mode_enabled() {
        return Ok(());
    }

    let lock = MEMBERSHIP_CHECKS
        .entry(claims.sub.clone())
        .or_default()
        .clone();
    let _guard = lock.lock().await;

    // Another request may have re-checked (or revoked the session) while we waited.
    let row: Option<(i32, Option<i64>)> = sqlx::query_as(&format!(
        "SELECT session_version, {MEMBERSHIP_AGE_SQL} FROM users WHERE github_login = $1"
    ))
    .bind(&claims.sub)
    .fetch_optional(&state.db)
    .await
    .map_err(db_failure)?;
    let Some((session_version, membership_age)) = row else {
        return Err(unauthorized("Session no longer valid"));
    };
    if session_version != claims.sv {
        return Err(unauthorized("Session no longer valid"));
    }
    if is_fresh(membership_age) {
        return Ok(());
    }

    let outcome = match load_github_token(&state.db, &claims.sub)
        .await
        .map_err(db_failure)?
    {
        Some(token) => check_org_membership(&token, &state.config.github_org, &claims.sub).await,
        // No credential to verify membership with: the user must sign in again.
        None => Ok(Membership::TokenRevoked),
    };

    match outcome {
        Ok(Membership::Member) => {
            sqlx::query("UPDATE users SET membership_checked_at = NOW() WHERE github_login = $1")
                .bind(&claims.sub)
                .execute(&state.db)
                .await
                .map_err(db_failure)?;
            Ok(())
        }
        Ok(lost) => {
            // Conditional on the version we validated, so racing requests bump once.
            let bumped = sqlx::query(
                "UPDATE users SET session_version = session_version + 1, updated_at = NOW()
                 WHERE github_login = $1 AND session_version = $2",
            )
            .bind(&claims.sub)
            .bind(claims.sv)
            .execute(&state.db)
            .await
            .map_err(db_failure)?;
            if bumped.rows_affected() > 0 {
                let reason = match lost {
                    Membership::NotMember => "not_org_member",
                    _ => "github_token_revoked",
                };
                crate::audit::log_event(
                    &state.db,
                    "auth.session_revoked",
                    &claims.sub,
                    None,
                    serde_json::json!({ "reason": reason }),
                    None,
                )
                .await;
            }
            Err(unauthorized("GitHub access was revoked; sign in again"))
        }
        Err(e) => {
            // Transient: never revoke on it. Honor a recent successful check.
            if membership_age.is_some_and(|a| a < freshness.grace_secs) {
                tracing::warn!(
                    "Membership re-check for {} failed, using last check: {e}",
                    claims.sub
                );
                Ok(())
            } else {
                tracing::warn!("Membership re-check for {} failed: {e}", claims.sub);
                Err((
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Could not verify GitHub organization membership; try again shortly",
                )
                    .into_response())
            }
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
        let (claims, membership_age) = load_session(parts, state).await?;
        ensure_membership_fresh(state, &claims, membership_age, AUTH_FRESHNESS).await?;
        Ok(AuthUser(claims))
    }
}

// ── Handlers ──

#[derive(Deserialize)]
pub struct CallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    /// Set by GitHub instead of `code` when the user cancels (`access_denied`).
    pub error: Option<String>,
}

fn generate_oauth_state() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Constant-time check that the `state` GitHub echoed back matches the one we
/// stored in the `oauth_state` cookie. Missing or empty values never match.
fn validate_oauth_state(cookie: Option<&str>, query: Option<&str>) -> bool {
    let (Some(cookie), Some(query)) = (cookie, query) else {
        return false;
    };
    let (a, b) = (cookie.as_bytes(), query.as_bytes());
    if a.is_empty() || a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn clear_oauth_state(jar: CookieJar) -> CookieJar {
    jar.remove(Cookie::build((OAUTH_STATE_COOKIE, "")).path(OAUTH_STATE_PATH))
}

pub async fn login(State(state): State<AppState>, jar: CookieJar) -> (CookieJar, Redirect) {
    let oauth_state = generate_oauth_state();
    let url = format!(
        "https://github.com/login/oauth/authorize?\
         client_id={}&redirect_uri={}&scope=repo%20read:org%20admin:repo_hook&allow_signup=false&state={}",
        state.config.github_client_id,
        urlencoding::encode(&state.config.github_redirect_uri),
        oauth_state,
    );
    let cookie = Cookie::build((OAUTH_STATE_COOKIE, oauth_state))
        .path(OAUTH_STATE_PATH)
        .http_only(true)
        .same_site(SameSite::Lax)
        .secure(true)
        .max_age(time::Duration::minutes(10));
    (jar.add(cookie), Redirect::temporary(&url))
}

pub async fn callback(
    State(state): State<AppState>,
    Query(query): Query<CallbackQuery>,
    jar: CookieJar,
) -> Result<(CookieJar, Redirect), (CookieJar, AppError)> {
    let stored_state = jar.get(OAUTH_STATE_COOKIE).map(|c| c.value().to_string());
    let jar = clear_oauth_state(jar);
    if !validate_oauth_state(stored_state.as_deref(), query.state.as_deref()) {
        return Err((
            jar,
            AppError::Auth("Invalid or missing OAuth state".to_string()),
        ));
    }
    let code = match (query.error, query.code) {
        (None, Some(code)) => code,
        _ => return Ok((jar, Redirect::temporary("/"))),
    };
    match complete_login(&state, &code).await {
        Ok(session) => Ok((jar.add(session), Redirect::temporary("/"))),
        Err(e) => Err((jar, e)),
    }
}

async fn complete_login(state: &AppState, code: &str) -> Result<Cookie<'static>, AppError> {
    let client = reqwest::Client::new();

    // Exchange code for access token
    let token_resp: GitHubTokenResponse = client
        .post("https://github.com/login/oauth/access_token")
        .header("Accept", "application/json")
        .form(&[
            ("code", code),
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

    let membership = check_org_membership(
        &token_resp.access_token,
        &state.config.github_org,
        &user_info.login,
    )
    .await
    .map_err(|e| {
        tracing::warn!("Login membership check for {} failed: {e}", user_info.login);
        AppError::Auth(format!(
            "Could not verify membership in '{}' with GitHub; try again",
            state.config.github_org
        ))
    })?;
    match membership {
        Membership::Member => {}
        Membership::NotMember => {
            return Err(AppError::Auth(format!(
                "User '{}' is not a member of the '{}' organization",
                user_info.login, state.config.github_org
            )));
        }
        Membership::TokenRevoked => {
            return Err(AppError::Auth(
                "GitHub rejected the access token".to_string(),
            ));
        }
    }

    // Upsert into users table
    let name = user_info.name.unwrap_or_else(|| user_info.login.clone());

    // Determine role for new users: first user ever gets 'admin', others get 'member'
    let user_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(&state.db)
        .await?;
    let role_for_new_user = if user_count.0 == 0 { "admin" } else { "member" };

    sqlx::query(
        "INSERT INTO users (github_login, name, email, github_token, role, membership_checked_at)
         VALUES ($1, $2, $3, $4, $5, NOW())
         ON CONFLICT(github_login) DO UPDATE SET
           name = EXCLUDED.name,
           email = EXCLUDED.email,
           github_token = EXCLUDED.github_token,
           membership_checked_at = NOW(),
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
    // and session version (the new JWT must match it)
    let (role, sv): (String, i32) =
        sqlx::query_as("SELECT role, session_version FROM users WHERE github_login = $1")
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
        sv,
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

    Ok(cookie.build())
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
/// JWT cookie identical to what the real OAuth callback produces. The user
/// row is created if missing and given the requested role, since sessions are
/// validated against it.
pub async fn test_login(
    State(state): State<AppState>,
    jar: CookieJar,
    axum::Json(req): axum::Json<TestLoginRequest>,
) -> Result<(CookieJar, axum::Json<serde_json::Value>), AppError> {
    if !matches!(req.role.as_str(), "admin" | "member" | "viewer") {
        return Err(AppError::BadRequest(format!("Invalid role '{}'", req.role)));
    }
    let (sv,): (i32,) = sqlx::query_as(
        "INSERT INTO users (github_login, name, github_token, role, membership_checked_at)
         VALUES ($1, $1, '', $2, NOW())
         ON CONFLICT(github_login) DO UPDATE SET
           role = EXCLUDED.role,
           membership_checked_at = NOW(),
           updated_at = NOW()
         RETURNING session_version",
    )
    .bind(&req.login)
    .bind(&req.role)
    .fetch_one(&state.db)
    .await?;

    let exp = chrono::Utc::now().timestamp() as usize + JWT_EXPIRY_SECS;
    let claims = Claims {
        sub: req.login.clone(),
        name: req.login.clone(),
        role: req.role.clone(),
        exp,
        sv,
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
        let (claims, membership_age) = load_session(parts, state).await?;

        if claims.role != "admin" {
            return Err((StatusCode::FORBIDDEN, "Admin access required").into_response());
        }

        ensure_membership_fresh(state, &claims, membership_age, PRIVILEGED_FRESHNESS).await?;
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
        let (claims, membership_age) = load_session(parts, state).await?;

        if claims.role != "admin" && claims.role != "member" {
            return Err((StatusCode::FORBIDDEN, "Member access required").into_response());
        }

        ensure_membership_fresh(state, &claims, membership_age, PRIVILEGED_FRESHNESS).await?;
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

    // A real role change revokes the user's sessions (bumps session_version).
    let result = sqlx::query(
        "UPDATE users SET
           session_version = session_version + CASE WHEN role = $1 THEN 0 ELSE 1 END,
           role = $1,
           updated_at = NOW()
         WHERE github_login = $2",
    )
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
    use axum::http::header::{LOCATION, SET_COOKIE};
    use axum::http::HeaderValue;

    fn test_state() -> AppState {
        let config = crate::config::Config {
            listen_addr: String::new(),
            database_url: String::new(),
            jwt_secret: "test-secret".into(),
            github_client_id: "client-id".into(),
            github_client_secret: "client-secret".into(),
            github_redirect_uri: "http://localhost/api/auth/callback".into(),
            github_org: "org".into(),
            preview_domain: String::new(),
            allowed_repos: Vec::new(),
            preview_bin: String::new(),
            agent_bin: String::new(),
            static_dir: String::new(),
            claude_bin: String::new(),
            claude_config_dir: String::new(),
            chromium_bin: String::new(),
            secrets_encryption_key: String::new(),
            intake_enabled: false,
            intake_max_global_concurrent: 0,
            github_webhook_secret: String::new(),
        };
        AppState {
            config: std::sync::Arc::new(config),
            // Lazy pool never connects; these handlers must not touch the DB.
            db: PgPool::connect_lazy("postgres://localhost/unused").unwrap(),
            task_channels: Default::default(),
        }
    }

    fn set_cookies(resp: &Response) -> Vec<String> {
        resp.headers()
            .get_all(SET_COOKIE)
            .iter()
            .map(|v| v.to_str().unwrap().to_string())
            .collect()
    }

    fn jar_with_state(value: &str) -> CookieJar {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            axum::http::header::COOKIE,
            HeaderValue::from_str(&format!("{OAUTH_STATE_COOKIE}={value}")).unwrap(),
        );
        CookieJar::from_headers(&headers)
    }

    fn query(code: Option<&str>, state: Option<&str>, error: Option<&str>) -> CallbackQuery {
        CallbackQuery {
            code: code.map(Into::into),
            state: state.map(Into::into),
            error: error.map(Into::into),
        }
    }

    fn assert_state_cookie_cleared(resp: &Response) {
        let cookies = set_cookies(resp);
        assert!(
            cookies.iter().any(|c| c.starts_with("oauth_state=")
                && c.contains("Path=/api/auth/callback")
                && c.contains("Max-Age=0")),
            "state cookie not cleared: {cookies:?}"
        );
    }

    #[tokio::test]
    async fn login_sets_state_cookie_and_redirect_state() {
        let resp = login(State(test_state()), CookieJar::new())
            .await
            .into_response();
        let cookies = set_cookies(&resp);
        assert_eq!(cookies.len(), 1);
        let cookie = &cookies[0];
        for attr in [
            "Path=/api/auth/callback",
            "HttpOnly",
            "Secure",
            "SameSite=Lax",
            "Max-Age=600",
        ] {
            assert!(cookie.contains(attr), "{attr} missing from {cookie}");
        }
        let value = cookie
            .strip_prefix("oauth_state=")
            .and_then(|rest| rest.split(';').next())
            .unwrap();
        assert_eq!(value.len(), 64);

        let location = resp.headers().get(LOCATION).unwrap().to_str().unwrap();
        assert!(location.starts_with("https://github.com/login/oauth/authorize?"));
        assert!(location.contains(&format!("&state={value}")));
    }

    #[tokio::test]
    async fn callback_cancelled_on_github_redirects_home_and_clears_cookie() {
        let resp = callback(
            State(test_state()),
            Query(query(None, Some("s3cret"), Some("access_denied"))),
            jar_with_state("s3cret"),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::TEMPORARY_REDIRECT);
        assert_eq!(resp.headers().get(LOCATION).unwrap(), "/");
        assert_state_cookie_cleared(&resp);
    }

    #[tokio::test]
    async fn callback_without_code_redirects_home() {
        let resp = callback(
            State(test_state()),
            Query(query(None, Some("s3cret"), None)),
            jar_with_state("s3cret"),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::TEMPORARY_REDIRECT);
        assert_eq!(resp.headers().get(LOCATION).unwrap(), "/");
        assert_state_cookie_cleared(&resp);
    }

    #[tokio::test]
    async fn callback_bad_state_is_rejected_before_anything_else() {
        for (cookie, q) in [
            (None, query(Some("code"), Some("s3cret"), None)),
            (Some("s3cret"), query(Some("code"), Some("other!"), None)),
            (Some("s3cret"), query(None, None, Some("access_denied"))),
        ] {
            let jar = cookie.map(jar_with_state).unwrap_or_default();
            let resp = callback(State(test_state()), Query(q), jar)
                .await
                .into_response();
            assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
            // The jar only emits a removal for a cookie the browser actually sent.
            if cookie.is_some() {
                assert_state_cookie_cleared(&resp);
            }
        }
    }

    #[test]
    fn membership_status_mapping() {
        use reqwest::StatusCode as S;
        assert_eq!(
            membership_from_status(S::NO_CONTENT),
            Some(Membership::Member)
        );
        assert_eq!(
            membership_from_status(S::NOT_FOUND),
            Some(Membership::NotMember)
        );
        assert_eq!(
            membership_from_status(S::FOUND),
            Some(Membership::NotMember)
        );
        assert_eq!(
            membership_from_status(S::UNAUTHORIZED),
            Some(Membership::TokenRevoked)
        );
        for transient in [
            S::OK,
            S::ACCEPTED,
            S::MOVED_PERMANENTLY,
            S::FORBIDDEN,
            S::TOO_MANY_REQUESTS,
            S::INTERNAL_SERVER_ERROR,
            S::BAD_GATEWAY,
        ] {
            assert_eq!(membership_from_status(transient), None, "{transient}");
        }
    }

    #[test]
    fn claims_without_session_version_decode_as_zero() {
        let key = EncodingKey::from_secret(b"k");
        let legacy = serde_json::json!({
            "sub": "alice", "name": "Alice", "role": "member", "exp": 4_000_000_000u64,
        });
        let token = encode(&Header::default(), &legacy, &key).unwrap();
        let claims = decode::<Claims>(
            &token,
            &DecodingKey::from_secret(b"k"),
            &Validation::default(),
        )
        .unwrap()
        .claims;
        assert_eq!(claims.sv, 0);
    }

    #[test]
    fn oauth_state_matches() {
        assert!(validate_oauth_state(Some("abc123"), Some("abc123")));
    }

    #[test]
    fn oauth_state_missing_either_side() {
        assert!(!validate_oauth_state(None, Some("abc123")));
        assert!(!validate_oauth_state(Some("abc123"), None));
        assert!(!validate_oauth_state(None, None));
    }

    #[test]
    fn oauth_state_empty_strings() {
        assert!(!validate_oauth_state(Some(""), Some("")));
        assert!(!validate_oauth_state(Some(""), Some("abc123")));
        assert!(!validate_oauth_state(Some("abc123"), Some("")));
    }

    #[test]
    fn oauth_state_mismatch() {
        assert!(!validate_oauth_state(Some("abc123"), Some("abc124")));
    }

    #[test]
    fn oauth_state_different_lengths() {
        assert!(!validate_oauth_state(Some("abc123"), Some("abc1234")));
        assert!(!validate_oauth_state(Some("abc1234"), Some("abc123")));
    }

    #[test]
    fn generated_oauth_state_is_64_hex_chars_and_unique() {
        let a = generate_oauth_state();
        let b = generate_oauth_state();
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
        assert!(validate_oauth_state(Some(&a), Some(&a)));
    }
}
