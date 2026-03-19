use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

#[derive(Debug)]
pub enum AppError {
    Auth(String),
    Forbidden(String),
    NotFound(String),
    BadRequest(String),
    Internal(String),
    /// A user-facing error with a machine-readable code for the frontend to map to friendly messages.
    UserError {
        code: &'static str,
        message: String,
    },
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Auth(msg) => write!(f, "Auth error: {msg}"),
            Self::Forbidden(msg) => write!(f, "Forbidden: {msg}"),
            Self::NotFound(msg) => write!(f, "Not found: {msg}"),
            Self::BadRequest(msg) => write!(f, "Bad request: {msg}"),
            Self::Internal(msg) => write!(f, "Internal error: {msg}"),
            Self::UserError { code, message } => write!(f, "User error [{code}]: {message}"),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message, code) = match &self {
            Self::Auth(msg) => (StatusCode::UNAUTHORIZED, msg.clone(), None),
            Self::Forbidden(msg) => (StatusCode::FORBIDDEN, msg.clone(), None),
            Self::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone(), None),
            Self::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone(), None),
            Self::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone(), None),
            Self::UserError { code, message } => (
                StatusCode::UNPROCESSABLE_ENTITY,
                message.clone(),
                Some(*code),
            ),
        };
        let body = if let Some(code) = code {
            axum::Json(json!({ "error": message, "error_code": code }))
        } else {
            axum::Json(json!({ "error": message }))
        };
        (status, body).into_response()
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        tracing::error!("Database error: {e}");
        Self::Internal("Database error".into())
    }
}

impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        tracing::error!("Internal error: {e}");
        Self::Internal(e.to_string())
    }
}

impl From<reqwest::Error> for AppError {
    fn from(e: reqwest::Error) -> Self {
        tracing::error!("HTTP client error: {e}");
        Self::Internal("External request failed".into())
    }
}
