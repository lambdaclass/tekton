use axum::extract::{Query, State};
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::auth::AdminUser;
use crate::error::AppError;

const WEEKLY_ACTIVE_WINDOW_DAYS: i32 = 7;
const DEFAULT_WINDOW_DAYS: i32 = 30;

#[derive(Debug, Deserialize)]
pub struct KpiQuery {
    pub days: Option<i32>,
}

#[derive(Debug, Serialize)]
pub struct KpiTrendPoint {
    pub day: String,
    pub sessions: i64,
    pub tasks_with_pr: i64,
}

#[derive(Debug, Serialize)]
pub struct KpiResponse {
    pub window_days: i32,
    pub weekly_active_prompting_users: i64,
    pub sessions: i64,
    pub tasks_with_pr: i64,
    pub session_to_pr_conversion_rate: f64,
    pub median_time_to_first_pr_seconds: Option<f64>,
    pub trends: Vec<KpiTrendPoint>,
}

/// GET /api/admin/metrics/kpis?days={n}
///
/// Adoption and impact KPIs (tekton issue #73):
/// - Weekly active prompting users: distinct users creating tasks in the last 7 days.
/// - Session-to-PR conversion: share of tasks in the window whose task.pr_created
///   event fired (a reviewable PR was opened). Merge-status tracking is a follow-up.
/// - Median time-to-first-reviewable-PR: median seconds between task creation and
///   the first `task.pr_created` audit event for that task.
pub async fn get_kpis(
    _admin: AdminUser,
    State(state): State<crate::AppState>,
    Query(q): Query<KpiQuery>,
) -> Result<Json<KpiResponse>, AppError> {
    let window_days = q.days.unwrap_or(DEFAULT_WINDOW_DAYS).max(1);

    let weekly_active_prompting_users: i64 = sqlx::query_scalar(
        "SELECT COUNT(DISTINCT created_by) \
         FROM tasks \
         WHERE created_by IS NOT NULL \
           AND created_at >= NOW() - ($1 || ' days')::INTERVAL",
    )
    .bind(WEEKLY_ACTIVE_WINDOW_DAYS.to_string())
    .fetch_one(&state.db)
    .await?;

    let (sessions, tasks_with_pr): (i64, i64) = sqlx::query_as(
        "SELECT \
            COUNT(*)::BIGINT, \
            COUNT(*) FILTER (WHERE pr_url IS NOT NULL)::BIGINT \
         FROM tasks \
         WHERE created_at >= NOW() - ($1 || ' days')::INTERVAL",
    )
    .bind(window_days.to_string())
    .fetch_one(&state.db)
    .await?;

    let session_to_pr_conversion_rate = if sessions > 0 {
        tasks_with_pr as f64 / sessions as f64
    } else {
        0.0
    };

    let median_time_to_first_pr_seconds: Option<f64> = sqlx::query_scalar(
        "SELECT PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY diff_seconds) \
         FROM ( \
             SELECT EXTRACT(EPOCH FROM (MIN(a.created_at) - t.created_at))::DOUBLE PRECISION \
                    AS diff_seconds \
             FROM audit_log a \
             JOIN tasks t ON t.id = a.target \
             WHERE a.event_type = 'task.pr_created' \
               AND t.created_at >= NOW() - ($1 || ' days')::INTERVAL \
             GROUP BY t.id, t.created_at \
         ) sub",
    )
    .bind(window_days.to_string())
    .fetch_one(&state.db)
    .await?;

    let trends = sqlx::query_as::<_, (String, i64, i64)>(
        "SELECT \
            TO_CHAR(created_at::date, 'YYYY-MM-DD') AS day, \
            COUNT(*)::BIGINT AS sessions, \
            COUNT(*) FILTER (WHERE pr_url IS NOT NULL)::BIGINT AS tasks_with_pr \
         FROM tasks \
         WHERE created_at >= NOW() - ($1 || ' days')::INTERVAL \
         GROUP BY created_at::date \
         ORDER BY day",
    )
    .bind(window_days.to_string())
    .fetch_all(&state.db)
    .await?
    .into_iter()
    .map(|(day, sessions, tasks_with_pr)| KpiTrendPoint {
        day,
        sessions,
        tasks_with_pr,
    })
    .collect();

    Ok(Json(KpiResponse {
        window_days,
        weekly_active_prompting_users,
        sessions,
        tasks_with_pr,
        session_to_pr_conversion_rate,
        median_time_to_first_pr_seconds,
        trends,
    }))
}
