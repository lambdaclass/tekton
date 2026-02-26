use axum::extract::{FromRequestParts, Query, State};
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use axum_extra::extract::cookie::{Cookie, SameSite};
use axum_extra::extract::CookieJar;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use rand::RngCore;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::error::AppError;
use crate::models::{Claims, GitHubTokenResponse, GitHubUserInfo, UserInfo};
use crate::AppState;

const COOKIE_NAME: &str = "dashboard_session";
const JWT_EXPIRY_SECS: usize = 86400 * 7; // 7 days

const CLAUDE_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const CLAUDE_AUTHORIZE_URL: &str = "https://claude.ai/oauth/authorize";
const CLAUDE_TOKEN_URL: &str = "https://console.anthropic.com/v1/oauth/token";

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

// ── GitHub OAuth Handlers ──

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

    sqlx::query(
        "INSERT INTO users (github_login, name, email, github_token)
         VALUES ($1, $2, $3, $4)
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
    .execute(&state.db)
    .await?;

    // Issue JWT
    let exp = chrono::Utc::now().timestamp() as usize + JWT_EXPIRY_SECS;
    let claims = Claims {
        sub: user_info.login,
        name,
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

pub async fn me(
    AuthUser(claims): AuthUser,
    State(state): State<AppState>,
) -> Result<axum::Json<UserInfo>, AppError> {
    let has_claude_key = sqlx::query_scalar::<_, bool>(
        "SELECT claude_access_token IS NOT NULL FROM users WHERE github_login = $1",
    )
    .bind(&claims.sub)
    .fetch_optional(&state.db)
    .await?
    .unwrap_or(false);

    Ok(axum::Json(UserInfo {
        login: claims.sub,
        name: claims.name,
        has_claude_key,
    }))
}

// ── Claude OAuth Handlers ──

#[derive(Deserialize)]
pub struct ClaudeCallbackQuery {
    pub code: String,
    pub state: String,
}

pub async fn claude_login(
    AuthUser(claims): AuthUser,
    State(state): State<AppState>,
) -> Result<Redirect, AppError> {
    let redirect_uri = &state.config.claude_oauth_redirect_uri;
    if redirect_uri.is_empty() {
        return Err(AppError::Internal(
            "CLAUDE_OAUTH_REDIRECT_URI is not configured".into(),
        ));
    }

    // Generate PKCE code_verifier (32 random bytes, base64url encoded)
    let mut verifier_bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut verifier_bytes);
    let code_verifier = URL_SAFE_NO_PAD.encode(verifier_bytes);

    // Generate code_challenge = base64url(SHA256(code_verifier))
    let mut hasher = Sha256::new();
    hasher.update(code_verifier.as_bytes());
    let code_challenge = URL_SAFE_NO_PAD.encode(hasher.finalize());

    // Generate random state token for CSRF protection
    let mut state_bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut state_bytes);
    let csrf_state = URL_SAFE_NO_PAD.encode(state_bytes);

    // Store (state, github_login, code_verifier) in DB
    sqlx::query(
        "INSERT INTO claude_oauth_states (state, github_login, code_verifier)
         VALUES ($1, $2, $3)",
    )
    .bind(&csrf_state)
    .bind(&claims.sub)
    .bind(&code_verifier)
    .execute(&state.db)
    .await?;

    // Clean up old states (older than 10 minutes)
    let _ = sqlx::query(
        "DELETE FROM claude_oauth_states WHERE created_at < NOW() - INTERVAL '10 minutes'",
    )
    .execute(&state.db)
    .await;

    let url = format!(
        "{CLAUDE_AUTHORIZE_URL}?\
         client_id={CLAUDE_CLIENT_ID}\
         &response_type=code\
         &redirect_uri={}\
         &scope=user:inference+user:profile\
         &code_challenge={code_challenge}\
         &code_challenge_method=S256\
         &state={csrf_state}",
        urlencoding::encode(redirect_uri),
    );

    Ok(Redirect::temporary(&url))
}

pub async fn claude_callback(
    State(state): State<AppState>,
    Query(query): Query<ClaudeCallbackQuery>,
) -> Result<Redirect, AppError> {
    // Look up PKCE state
    let row = sqlx::query_as::<_, (String, String)>(
        "SELECT github_login, code_verifier FROM claude_oauth_states WHERE state = $1",
    )
    .bind(&query.state)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::BadRequest("Invalid or expired OAuth state".into()))?;

    let (github_login, code_verifier) = row;

    // Delete the state record
    let _ = sqlx::query("DELETE FROM claude_oauth_states WHERE state = $1")
        .bind(&query.state)
        .execute(&state.db)
        .await;

    // Exchange code for tokens
    let client = reqwest::Client::new();
    let token_resp = client
        .post(CLAUDE_TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", &query.code),
            ("code_verifier", &code_verifier),
            ("client_id", CLAUDE_CLIENT_ID),
            ("redirect_uri", &state.config.claude_oauth_redirect_uri),
        ])
        .send()
        .await?;

    if !token_resp.status().is_success() {
        let status = token_resp.status();
        let body = token_resp.text().await.unwrap_or_default();
        return Err(AppError::Internal(format!(
            "Claude token exchange failed ({status}): {body}"
        )));
    }

    let token_data: serde_json::Value = token_resp.json().await?;

    let access_token = token_data["access_token"]
        .as_str()
        .ok_or_else(|| AppError::Internal("Missing access_token in Claude response".into()))?;
    let refresh_token = token_data["refresh_token"]
        .as_str()
        .ok_or_else(|| AppError::Internal("Missing refresh_token in Claude response".into()))?;
    let expires_in = token_data["expires_in"]
        .as_i64()
        .unwrap_or(28800); // default 8 hours

    let expires_at = chrono::Utc::now().timestamp() + expires_in;

    // Store tokens in users table
    sqlx::query(
        "UPDATE users SET
           claude_access_token = $1,
           claude_refresh_token = $2,
           claude_token_expires_at = $3,
           updated_at = NOW()
         WHERE github_login = $4",
    )
    .bind(access_token)
    .bind(refresh_token)
    .bind(expires_at)
    .bind(&github_login)
    .execute(&state.db)
    .await?;

    tracing::info!("Claude OAuth tokens stored for user '{github_login}'");

    Ok(Redirect::temporary("/tasks"))
}

pub async fn claude_disconnect(
    AuthUser(claims): AuthUser,
    State(state): State<AppState>,
) -> Result<axum::Json<serde_json::Value>, AppError> {
    sqlx::query(
        "UPDATE users SET
           claude_access_token = NULL,
           claude_refresh_token = NULL,
           claude_token_expires_at = NULL,
           updated_at = NOW()
         WHERE github_login = $1",
    )
    .bind(&claims.sub)
    .execute(&state.db)
    .await?;

    tracing::info!("Claude OAuth tokens removed for user '{}'", claims.sub);

    Ok(axum::Json(serde_json::json!({ "ok": true })))
}
