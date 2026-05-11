use axum::extract::{Query, State};
use axum::Json;

use crate::auth::AuthUser;
use crate::error::AppError;
use crate::models::CostByQuery;

/// Parse days from either `days` (integer) or `period` (e.g. "7d", "30d", "90d"). Default 30.
fn parse_days(days: Option<i32>, period: Option<&str>) -> i32 {
    if let Some(d) = days {
        return d;
    }
    match period {
        Some("7d") => 7,
        Some("90d") => 90,
        _ => 30,
    }
}

/// GET /api/metrics/summary?days={n}
/// Overall usage summary: active users, total users, tasks broken down by status,
/// total cost, and average cost per task for the selected period.
pub async fn summary(
    _user: AuthUser,
    State(state): State<crate::AppState>,
    Query(q): Query<CostByQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let days = parse_days(q.days, q.period.as_deref());
    let days_str = days.to_string();

    // Active users = distinct users with an auth.login event in the current period
    let active_users: (i64,) = sqlx::query_as(
        "SELECT COUNT(DISTINCT actor)::BIGINT FROM audit_log \
         WHERE event_type = 'auth.login' \
         AND created_at >= NOW() - ($1 || ' days')::INTERVAL",
    )
    .bind(&days_str)
    .fetch_one(&state.db)
    .await?;

    // Active users in the previous equal-length period (for trend comparison)
    let prev_active_users: (i64,) = sqlx::query_as(
        "SELECT COUNT(DISTINCT actor)::BIGINT FROM audit_log \
         WHERE event_type = 'auth.login' \
         AND created_at >= NOW() - ($1 || ' days')::INTERVAL * 2 \
         AND created_at <  NOW() - ($1 || ' days')::INTERVAL",
    )
    .bind(&days_str)
    .fetch_one(&state.db)
    .await?;

    // Total registered users (all time)
    let total_users: (i64,) = sqlx::query_as("SELECT COUNT(*)::BIGINT FROM users")
        .fetch_one(&state.db)
        .await?;

    // Current-period task counts by status + cost + tokens
    let task_row: (i64, i64, i64, i64, f64, i64, i64) = sqlx::query_as(
        "SELECT \
            COUNT(*)::BIGINT, \
            COUNT(*) FILTER (WHERE status = 'completed')::BIGINT, \
            COUNT(*) FILTER (WHERE status = 'failed')::BIGINT, \
            COUNT(*) FILTER (WHERE status NOT IN ('completed', 'failed'))::BIGINT, \
            COALESCE(SUM(total_cost_usd), 0)::DOUBLE PRECISION, \
            COALESCE(SUM(total_input_tokens), 0)::BIGINT, \
            COALESCE(SUM(total_output_tokens), 0)::BIGINT \
         FROM tasks \
         WHERE created_at >= NOW() - ($1 || ' days')::INTERVAL",
    )
    .bind(&days_str)
    .fetch_one(&state.db)
    .await?;

    // Previous-period task count + cost (for trend comparison)
    let prev_row: (i64, f64) = sqlx::query_as(
        "SELECT \
            COUNT(*)::BIGINT, \
            COALESCE(SUM(total_cost_usd), 0)::DOUBLE PRECISION \
         FROM tasks \
         WHERE created_at >= NOW() - ($1 || ' days')::INTERVAL * 2 \
           AND created_at <  NOW() - ($1 || ' days')::INTERVAL",
    )
    .bind(&days_str)
    .fetch_one(&state.db)
    .await?;

    let total_tasks = task_row.0;
    let total_cost_usd = task_row.4;
    let avg_cost_per_task = if total_tasks > 0 {
        total_cost_usd / total_tasks as f64
    } else {
        0.0
    };

    Ok(Json(serde_json::json!({
        "days": days,
        "active_users": active_users.0,
        "prev_active_users": prev_active_users.0,
        "total_users": total_users.0,
        "total_tasks": total_tasks,
        "prev_total_tasks": prev_row.0,
        "completed_tasks": task_row.1,
        "failed_tasks": task_row.2,
        "in_progress_tasks": task_row.3,
        "total_cost_usd": total_cost_usd,
        "prev_total_cost_usd": prev_row.1,
        "total_input_tokens": task_row.5,
        "total_output_tokens": task_row.6,
        "avg_cost_per_task": avg_cost_per_task,
    })))
}

/// GET /api/metrics/tasks-over-time?days={n}
///
/// Daily activity counts + cost for charting. A task is counted as activity on
/// every day where *something happened* for it: it was created, it received a
/// message, or it produced a log line. Agent logs count as activity too — a new
/// log line only appears either because a person sent a new message that day or
/// because the agent was still working through a long-running task on that day,
/// and both of those are legitimately "worked on".
///
/// Cost is attributed to every in-window activity day of a task in equal shares
/// so `total_cost_usd` is split evenly across the days the task was worked on
/// within the selected window. This keeps the cost curve visually consistent
/// with the task-activity curve rather than piling cost onto a single
/// `updated_at` day (which would show cost as zero on days that clearly had
/// tasks running).
pub async fn tasks_over_time(
    _user: AuthUser,
    State(state): State<crate::AppState>,
    Query(q): Query<CostByQuery>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let days = parse_days(q.days, q.period.as_deref());

    let rows: Vec<(String, i64, f64)> = sqlx::query_as(
        "WITH days AS (\
            SELECT generate_series(\
                DATE_TRUNC('day', NOW() - ($1 || ' days')::INTERVAL + INTERVAL '1 day'), \
                DATE_TRUNC('day', NOW()), \
                INTERVAL '1 day'\
            ) AS day\
         ), \
         activity AS (\
            SELECT id AS task_id, DATE_TRUNC('day', created_at) AS day FROM tasks \
             WHERE created_at >= NOW() - ($1 || ' days')::INTERVAL \
            UNION \
            SELECT task_id, DATE_TRUNC('day', created_at) FROM task_messages \
             WHERE created_at >= NOW() - ($1 || ' days')::INTERVAL \
            UNION \
            SELECT task_id, DATE_TRUNC('day', timestamp) FROM task_logs \
             WHERE timestamp >= NOW() - ($1 || ' days')::INTERVAL \
         ), \
         task_day_counts AS (\
            SELECT task_id, COUNT(DISTINCT day)::BIGINT AS n_days \
            FROM activity \
            GROUP BY task_id\
         ), \
         daily_rows AS (\
            SELECT \
                a.day, \
                a.task_id, \
                (COALESCE(t.total_cost_usd, 0) / NULLIF(tdc.n_days, 0))::DOUBLE PRECISION AS day_cost \
            FROM activity a \
            JOIN tasks t ON t.id = a.task_id \
            JOIN task_day_counts tdc ON tdc.task_id = a.task_id\
         ) \
         SELECT \
            TO_CHAR(d.day, 'YYYY-MM-DD') AS day, \
            COALESCE(COUNT(DISTINCT dr.task_id), 0)::BIGINT AS total, \
            COALESCE(SUM(dr.day_cost), 0)::DOUBLE PRECISION AS cost_usd \
         FROM days d \
         LEFT JOIN daily_rows dr ON dr.day = d.day \
         GROUP BY d.day \
         ORDER BY d.day ASC",
    )
    .bind(days.to_string())
    .fetch_all(&state.db)
    .await?;

    let out: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|(day, total, cost_usd)| {
            serde_json::json!({
                "day": day,
                "total": total,
                "cost_usd": cost_usd,
            })
        })
        .collect();

    Ok(Json(out))
}

/// GET /api/metrics/top-users?days={n}
/// Top 10 users by task count in the period, with their total cost.
pub async fn top_users(
    _user: AuthUser,
    State(state): State<crate::AppState>,
    Query(q): Query<CostByQuery>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let days = parse_days(q.days, q.period.as_deref());

    let rows: Vec<(String, i64, f64)> = sqlx::query_as(
        "SELECT \
            created_by, \
            COUNT(*)::BIGINT AS task_count, \
            COALESCE(SUM(total_cost_usd), 0)::DOUBLE PRECISION AS cost_usd \
         FROM tasks \
         WHERE created_at >= NOW() - ($1 || ' days')::INTERVAL \
           AND created_by IS NOT NULL \
         GROUP BY created_by \
         ORDER BY task_count DESC, cost_usd DESC \
         LIMIT 10",
    )
    .bind(days.to_string())
    .fetch_all(&state.db)
    .await?;

    let out: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|(login, task_count, cost_usd)| {
            serde_json::json!({
                "login": login,
                "task_count": task_count,
                "cost_usd": cost_usd,
            })
        })
        .collect();

    Ok(Json(out))
}

/// GET /api/metrics/top-repos?days={n}
/// Top 10 repos by task count in the period, with their total cost.
pub async fn top_repos(
    _user: AuthUser,
    State(state): State<crate::AppState>,
    Query(q): Query<CostByQuery>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let days = parse_days(q.days, q.period.as_deref());

    let rows: Vec<(String, i64, f64)> = sqlx::query_as(
        "SELECT \
            repo, \
            COUNT(*)::BIGINT AS task_count, \
            COALESCE(SUM(total_cost_usd), 0)::DOUBLE PRECISION AS cost_usd \
         FROM tasks \
         WHERE created_at >= NOW() - ($1 || ' days')::INTERVAL \
         GROUP BY repo \
         ORDER BY task_count DESC, cost_usd DESC \
         LIMIT 10",
    )
    .bind(days.to_string())
    .fetch_all(&state.db)
    .await?;

    let out: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|(repo, task_count, cost_usd)| {
            serde_json::json!({
                "repo": repo,
                "task_count": task_count,
                "cost_usd": cost_usd,
            })
        })
        .collect();

    Ok(Json(out))
}
