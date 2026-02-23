use serde::{Deserialize, Serialize};

// ── Auth ──

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,   // email
    pub name: String,
    pub exp: usize,
}

#[derive(Debug, Serialize)]
pub struct UserInfo {
    pub email: String,
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct GoogleTokenResponse {
    pub access_token: String,
}

#[derive(Debug, Deserialize)]
pub struct GoogleUserInfo {
    pub email: String,
    pub name: Option<String>,
    pub hd: Option<String>, // hosted domain
}

// ── Previews ──

#[derive(Debug, Serialize, Clone)]
pub struct Preview {
    pub slug: String,
    pub repo: String,
    pub branch: String,
    pub preview_type: String,
    pub url: String,
}

#[derive(Debug, Deserialize)]
pub struct CreatePreviewRequest {
    pub repo: String,
    pub branch: String,
    pub slug: Option<String>,
    #[serde(rename = "type", default = "default_preview_type")]
    pub preview_type: String,
}

fn default_preview_type() -> String {
    "node".into()
}

// ── Tasks ──

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct Task {
    pub id: String,
    pub prompt: String,
    pub repo: String,
    pub base_branch: String,
    pub branch_name: Option<String>,
    pub agent_name: Option<String>,
    pub status: String,
    pub preview_slug: Option<String>,
    pub preview_url: Option<String>,
    pub error_message: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub parent_task_id: Option<String>,
    pub created_by: Option<String>,
    pub screenshot_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateTaskRequest {
    pub prompt: String,
    pub repo: String,
    pub base_branch: Option<String>,
    pub parent_task_id: Option<String>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct TaskLog {
    pub id: i64,
    pub task_id: String,
    pub timestamp: String,
    pub line: String,
}

// ── Task Messages ──

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct TaskMessage {
    pub id: i64,
    pub task_id: String,
    pub sender: String,
    pub content: String,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct SendMessageRequest {
    pub content: String,
}

// ── Classify ──

#[derive(Debug, Deserialize)]
pub struct ClassifyRequest {
    pub prompt: String,
}

#[derive(Debug, Serialize)]
pub struct ClassifyResponse {
    pub repo: String,
}
