use axum::extract::{FromRequestParts, Query, State};
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use axum_extra::extract::cookie::{Cookie, SameSite};
use axum_extra::extract::CookieJar;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::Deserialize;

use crate::error::AppError;
use crate::models::{Claims, GoogleTokenResponse, GoogleUserInfo, UserInfo};
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
        "https://accounts.google.com/o/oauth2/v2/auth?\
         client_id={}&redirect_uri={}&response_type=code&\
         scope=openid%20email%20profile&\
         hd={}&prompt=select_account",
        state.config.google_client_id,
        urlencoding::encode(&state.config.google_redirect_uri),
        state.config.allowed_domain,
    );
    Redirect::temporary(&url)
}

pub async fn callback(
    State(state): State<AppState>,
    Query(query): Query<CallbackQuery>,
    jar: CookieJar,
) -> Result<(CookieJar, Redirect), AppError> {
    // Exchange code for access token
    let token_resp: GoogleTokenResponse = reqwest::Client::new()
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("code", query.code.as_str()),
            ("client_id", state.config.google_client_id.as_str()),
            ("client_secret", state.config.google_client_secret.as_str()),
            ("redirect_uri", state.config.google_redirect_uri.as_str()),
            ("grant_type", "authorization_code"),
        ])
        .send()
        .await?
        .json()
        .await?;

    // Get user info
    let user_info: GoogleUserInfo = reqwest::Client::new()
        .get("https://www.googleapis.com/oauth2/v2/userinfo")
        .bearer_auth(&token_resp.access_token)
        .send()
        .await?
        .json()
        .await?;

    // Verify domain
    let domain = user_info.email.split('@').nth(1).unwrap_or_default();
    if domain != state.config.allowed_domain {
        return Err(AppError::Auth(format!(
            "Email domain '{domain}' is not allowed. Must be @{}",
            state.config.allowed_domain
        )));
    }

    // Issue JWT
    let exp = chrono::Utc::now().timestamp() as usize + JWT_EXPIRY_SECS;
    let claims = Claims {
        sub: user_info.email.clone(),
        name: user_info.name.unwrap_or_else(|| user_info.email.clone()),
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
        email: claims.sub,
        name: claims.name,
    })
}
