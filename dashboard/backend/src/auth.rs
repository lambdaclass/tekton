use axum::extract::{FromRequestParts, Query, State};
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use axum_extra::extract::cookie::{Cookie, SameSite};
use axum_extra::extract::CookieJar;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::Deserialize;

use crate::error::AppError;
use crate::models::{Claims, GitHubTokenResponse, GitHubUserInfo, UserInfo};
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

pub async fn me(AuthUser(claims): AuthUser) -> axum::Json<UserInfo> {
    axum::Json(UserInfo {
        login: claims.sub,
        name: claims.name,
    })
}
