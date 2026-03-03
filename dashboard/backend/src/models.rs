use serde::{Deserialize, Serialize};

// ── Auth ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String, // github_login
    pub name: String,
    pub role: String,
    pub exp: usize,
}

#[derive(Debug, Serialize)]
pub struct UserInfo {
    pub login: String,
    pub name: String,
    pub role: String,
}

#[derive(Debug, Deserialize)]
pub struct SetUserRoleRequest {
    pub role: String,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct UserRepoPermission {
    pub github_login: String,
    pub repo: String,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct SecretEntry {
    pub id: i64,
    pub repo: String,
    pub name: String,
    pub created_by: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateSecretRequest {
    pub repo: String,
    pub name: String,
    pub value: String,
}

#[derive(Debug, Deserialize)]
pub struct GitHubTokenResponse {
    pub access_token: String,
}

#[derive(Debug, Deserialize)]
pub struct GitHubUserInfo {
    pub login: String,
    pub name: Option<String>,
    pub email: Option<String>,
}

// ── Previews ──

#[derive(Debug, Serialize, Clone)]
pub struct Preview {
    pub slug: String,
    pub repo: String,
    pub branch: String,
    pub url: String,
}

#[derive(Debug, Deserialize)]
pub struct CreatePreviewRequest {
    pub repo: String,
    pub branch: String,
    pub slug: Option<String>,
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
    pub image_url: Option<String>,
    pub total_input_tokens: Option<i64>,
    pub total_output_tokens: Option<i64>,
    pub total_cost_usd: Option<f64>,
    pub name: Option<String>,
    pub pr_url: Option<String>,
    pub pr_number: Option<i32>,
    pub compute_seconds: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct CreateTaskRequest {
    pub prompt: String,
    pub repo: String,
    pub base_branch: Option<String>,
    pub parent_task_id: Option<String>,
    pub image_urls: Option<Vec<String>>,
    pub custom_branch_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateTaskNameRequest {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct LinkPrRequest {
    pub pr_url: String,
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
    pub image_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SendMessageRequest {
    pub content: String,
    pub image_urls: Option<Vec<String>>,
}

// ── Task Actions ──

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct TaskAction {
    pub id: i64,
    pub task_id: String,
    pub action_type: String,
    pub tool_name: Option<String>,
    pub tool_input: Option<serde_json::Value>,
    pub summary: Option<String>,
    pub created_at: String,
}

// ── Task State Transitions ──

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct TaskStateTransition {
    pub id: i64,
    pub task_id: String,
    pub from_status: Option<String>,
    pub to_status: String,
    pub created_at: String,
}

// ── Query params ──

#[derive(Debug, Deserialize)]
pub struct ListTasksQuery {
    pub status: Option<String>,
    pub repo: Option<String>,
    pub created_by: Option<String>,
    pub search: Option<String>,
    pub page: Option<u32>,
    pub per_page: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct ListMessagesQuery {
    pub limit: Option<i64>,
    pub before_id: Option<i64>,
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

// ── Repo Policies ──

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct RepoPolicy {
    pub id: i64,
    pub repo: String,
    pub protected_branches: Vec<String>,
    pub allowed_tools: Option<serde_json::Value>,
    pub network_egress: Option<serde_json::Value>,
    pub max_cost_usd: Option<f64>,
    pub require_approval_above_usd: Option<f64>,
    pub created_by: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateRepoPolicyRequest {
    pub repo: String,
    pub protected_branches: Option<Vec<String>>,
    pub allowed_tools: Option<serde_json::Value>,
    pub network_egress: Option<serde_json::Value>,
    pub max_cost_usd: Option<f64>,
    pub require_approval_above_usd: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateRepoPolicyRequest {
    pub protected_branches: Option<Vec<String>>,
    pub allowed_tools: Option<serde_json::Value>,
    pub network_egress: Option<serde_json::Value>,
    pub max_cost_usd: Option<f64>,
    pub require_approval_above_usd: Option<f64>,
}

// ── Org Policies ──

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct OrgPolicy {
    pub id: i64,
    pub org: String,
    pub protected_branches: Vec<String>,
    pub allowed_tools: Option<serde_json::Value>,
    pub network_egress: Option<serde_json::Value>,
    pub max_cost_usd: Option<f64>,
    pub require_approval_above_usd: Option<f64>,
    pub created_by: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateOrgPolicyRequest {
    pub org: String,
    pub protected_branches: Option<Vec<String>>,
    pub allowed_tools: Option<serde_json::Value>,
    pub network_egress: Option<serde_json::Value>,
    pub max_cost_usd: Option<f64>,
    pub require_approval_above_usd: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateOrgPolicyRequest {
    pub protected_branches: Option<Vec<String>>,
    pub allowed_tools: Option<serde_json::Value>,
    pub network_egress: Option<serde_json::Value>,
    pub max_cost_usd: Option<f64>,
    pub require_approval_above_usd: Option<f64>,
}

// ── Cost & Budgets ──

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct CostSummaryRow {
    pub group_key: String,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub total_compute_seconds: i64,
    pub cost_usd: f64,
}


#[derive(Debug, Deserialize)]
pub struct CostByQuery {
    pub user: Option<String>,
    pub repo: Option<String>,
    pub period: Option<String>,
    pub days: Option<i32>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct DailyCostRow {
    pub day: String,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub total_compute_seconds: i64,
    pub cost_usd: f64,
    pub task_count: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct Budget {
    pub id: i64,
    pub scope: String,
    pub scope_type: String,
    pub monthly_limit_usd: f64,
    pub alert_threshold_pct: i32,
    pub created_by: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateBudgetRequest {
    pub scope: String,
    pub scope_type: String,
    pub monthly_limit_usd: f64,
    pub alert_threshold_pct: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateBudgetRequest {
    pub monthly_limit_usd: Option<f64>,
    pub alert_threshold_pct: Option<i32>,
}

// ── Audit Log ──

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct AuditLogEntry {
    pub id: i64,
    pub event_type: String,
    pub actor: String,
    pub target: Option<String>,
    pub detail: Option<serde_json::Value>,
    pub ip_address: Option<String>,
    pub created_at: String,
}

// ── Paginated response ──

#[derive(Debug, Serialize)]
pub struct PaginatedTasks {
    pub tasks: Vec<Task>,
    pub total: i64,
    pub page: u32,
    pub per_page: u32,
}
