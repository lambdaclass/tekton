use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;
use sqlx::PgPool;

use crate::auth::AdminUser;
use crate::error::AppError;
use crate::models::AuditLogEntry;

/// Insert an audit log event. Errors are logged but never propagated
/// so callers don't fail their main operation due to audit logging.
pub async fn log_event(
    db: &PgPool,
    event_type: &str,
    actor: &str,
    target: Option<&str>,
    detail: serde_json::Value,
    ip: Option<&str>,
) {
    let result = sqlx::query(
        "INSERT INTO audit_log (event_type, actor, target, detail, ip_address) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(event_type)
    .bind(actor)
    .bind(target)
    .bind(&detail)
    .bind(ip)
    .execute(db)
    .await;

    if let Err(e) = result {
        tracing::warn!("Failed to write audit log event '{event_type}': {e}");
    }
}

// ── Query API ──

#[derive(Debug, Deserialize)]
pub struct AuditLogQuery {
    pub event_type: Option<String>,
    pub actor: Option<String>,
    pub target: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub page: Option<u32>,
    pub per_page: Option<u32>,
}

#[derive(Debug, serde::Serialize)]
pub struct PaginatedAuditLog {
    pub entries: Vec<AuditLogEntry>,
    pub total: i64,
    pub page: u32,
    pub per_page: u32,
}

/// GET /api/admin/audit-log
pub async fn list_audit_log(
    _admin: AdminUser,
    State(state): State<crate::AppState>,
    Query(params): Query<AuditLogQuery>,
) -> Result<Json<PaginatedAuditLog>, AppError> {
    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(50).min(200);
    let offset = (page - 1) * per_page;

    // Build WHERE clauses dynamically
    let mut conditions: Vec<String> = Vec::new();
    let mut param_idx = 1u32;

    if params.event_type.is_some() {
        conditions.push(format!("event_type = ${param_idx}"));
        param_idx += 1;
    }
    if params.actor.is_some() {
        conditions.push(format!("actor = ${param_idx}"));
        param_idx += 1;
    }
    if params.target.is_some() {
        conditions.push(format!("target = ${param_idx}"));
        param_idx += 1;
    }
    if params.from.is_some() {
        conditions.push(format!("created_at >= ${param_idx}::timestamptz"));
        param_idx += 1;
    }
    if params.to.is_some() {
        conditions.push(format!("created_at <= ${param_idx}::timestamptz"));
        param_idx += 1;
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let count_sql = format!("SELECT COUNT(*) FROM audit_log {where_clause}");
    let data_sql = format!(
        "SELECT id, event_type, actor, target, detail, ip_address, \
         TO_CHAR(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as created_at \
         FROM audit_log {where_clause} \
         ORDER BY created_at DESC \
         LIMIT ${param_idx} OFFSET ${}",
        param_idx + 1
    );

    // Bind parameters for count query
    let mut count_query = sqlx::query_scalar::<_, i64>(&count_sql);
    if let Some(ref v) = params.event_type {
        count_query = count_query.bind(v);
    }
    if let Some(ref v) = params.actor {
        count_query = count_query.bind(v);
    }
    if let Some(ref v) = params.target {
        count_query = count_query.bind(v);
    }
    if let Some(ref v) = params.from {
        count_query = count_query.bind(v);
    }
    if let Some(ref v) = params.to {
        count_query = count_query.bind(v);
    }
    let total = count_query.fetch_one(&state.db).await?;

    // Bind parameters for data query
    let mut data_query = sqlx::query_as::<_, AuditLogEntry>(&data_sql);
    if let Some(ref v) = params.event_type {
        data_query = data_query.bind(v);
    }
    if let Some(ref v) = params.actor {
        data_query = data_query.bind(v);
    }
    if let Some(ref v) = params.target {
        data_query = data_query.bind(v);
    }
    if let Some(ref v) = params.from {
        data_query = data_query.bind(v);
    }
    if let Some(ref v) = params.to {
        data_query = data_query.bind(v);
    }
    data_query = data_query.bind(per_page as i64).bind(offset as i64);

    let entries = data_query.fetch_all(&state.db).await?;

    Ok(Json(PaginatedAuditLog {
        entries,
        total,
        page,
        per_page,
    }))
}
