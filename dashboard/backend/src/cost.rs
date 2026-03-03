use axum::extract::{Path, Query, State};
use axum::Json;
use sqlx::PgPool;

use crate::auth::AdminUser;
use crate::error::AppError;
use crate::models::{
    Budget, CostByQuery, CostSummaryRow, CreateBudgetRequest, DailyCostRow,
    UpdateBudgetRequest,
};

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

// ── Cost aggregation endpoints ──

/// GET /api/admin/cost/summary?days={n}
/// Flat aggregate using real cost from Claude CLI (total_cost_usd on each task).
pub async fn cost_summary(
    _admin: AdminUser,
    State(state): State<crate::AppState>,
    Query(q): Query<CostByQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let days = parse_days(q.days, q.period.as_deref());

    let row: (f64, i64, i64, i64) = sqlx::query_as(
        "SELECT \
            COALESCE(SUM(total_cost_usd), 0)::DOUBLE PRECISION, \
            COALESCE(SUM(total_input_tokens), 0)::BIGINT, \
            COALESCE(SUM(total_output_tokens), 0)::BIGINT, \
            COUNT(*)::BIGINT \
         FROM tasks \
         WHERE created_at >= NOW() - ($1 || ' days')::INTERVAL",
    )
    .bind(days.to_string())
    .fetch_one(&state.db)
    .await?;

    let total_cost_usd = row.0;
    let total_input_tokens = row.1;
    let total_output_tokens = row.2;
    let total_tasks = row.3;
    let avg_cost_per_task = if total_tasks > 0 {
        total_cost_usd / total_tasks as f64
    } else {
        0.0
    };

    Ok(Json(serde_json::json!({
        "total_cost_usd": total_cost_usd,
        "total_tasks": total_tasks,
        "avg_cost_per_task": avg_cost_per_task,
        "total_input_tokens": total_input_tokens,
        "total_output_tokens": total_output_tokens
    })))
}

/// GET /api/admin/cost/by-user?days={n}
/// Aggregated cost per user using real cost from Claude CLI.
pub async fn cost_by_user(
    _admin: AdminUser,
    State(state): State<crate::AppState>,
    Query(q): Query<CostByQuery>,
) -> Result<Json<Vec<CostSummaryRow>>, AppError> {
    let days = parse_days(q.days, q.period.as_deref());

    let rows = sqlx::query_as::<_, CostSummaryRow>(
        "SELECT \
            COALESCE(created_by, 'unknown') as group_key, \
            COALESCE(SUM(total_input_tokens), 0)::BIGINT as total_input_tokens, \
            COALESCE(SUM(total_output_tokens), 0)::BIGINT as total_output_tokens, \
            COALESCE(SUM(compute_seconds), 0)::BIGINT as total_compute_seconds, \
            COALESCE(SUM(total_cost_usd), 0)::DOUBLE PRECISION as cost_usd \
         FROM tasks \
         WHERE created_at >= NOW() - ($1 || ' days')::INTERVAL \
         GROUP BY created_by \
         ORDER BY cost_usd DESC",
    )
    .bind(days.to_string())
    .fetch_all(&state.db)
    .await?;

    Ok(Json(rows))
}

/// GET /api/admin/cost/by-repo?days={n}
/// Aggregated cost per repo using real cost from Claude CLI.
pub async fn cost_by_repo(
    _admin: AdminUser,
    State(state): State<crate::AppState>,
    Query(q): Query<CostByQuery>,
) -> Result<Json<Vec<CostSummaryRow>>, AppError> {
    let days = parse_days(q.days, q.period.as_deref());

    let rows = sqlx::query_as::<_, CostSummaryRow>(
        "SELECT \
            repo as group_key, \
            COALESCE(SUM(total_input_tokens), 0)::BIGINT as total_input_tokens, \
            COALESCE(SUM(total_output_tokens), 0)::BIGINT as total_output_tokens, \
            COALESCE(SUM(compute_seconds), 0)::BIGINT as total_compute_seconds, \
            COALESCE(SUM(total_cost_usd), 0)::DOUBLE PRECISION as cost_usd \
         FROM tasks \
         WHERE created_at >= NOW() - ($1 || ' days')::INTERVAL \
         GROUP BY repo \
         ORDER BY cost_usd DESC",
    )
    .bind(days.to_string())
    .fetch_all(&state.db)
    .await?;

    Ok(Json(rows))
}

/// GET /api/admin/cost/trends?days={n}
/// Daily aggregate totals for burn rate chart, using real cost.
pub async fn cost_trends(
    _admin: AdminUser,
    State(state): State<crate::AppState>,
    Query(q): Query<CostByQuery>,
) -> Result<Json<Vec<DailyCostRow>>, AppError> {
    let days = parse_days(q.days, q.period.as_deref());

    let rows = sqlx::query_as::<_, DailyCostRow>(
        "SELECT \
            TO_CHAR(created_at::date, 'YYYY-MM-DD') as day, \
            COALESCE(SUM(total_input_tokens), 0)::BIGINT as total_input_tokens, \
            COALESCE(SUM(total_output_tokens), 0)::BIGINT as total_output_tokens, \
            COALESCE(SUM(compute_seconds), 0)::BIGINT as total_compute_seconds, \
            COALESCE(SUM(total_cost_usd), 0)::DOUBLE PRECISION as cost_usd, \
            COUNT(*)::BIGINT as task_count \
         FROM tasks \
         WHERE created_at >= NOW() - ($1 || ' days')::INTERVAL \
         GROUP BY created_at::date \
         ORDER BY day",
    )
    .bind(days.to_string())
    .fetch_all(&state.db)
    .await?;

    Ok(Json(rows))
}

// ── Budget CRUD ──

/// GET /api/admin/budgets
pub async fn list_budgets(
    _admin: AdminUser,
    State(state): State<crate::AppState>,
) -> Result<Json<Vec<Budget>>, AppError> {
    let budgets = sqlx::query_as::<_, Budget>(
        "SELECT id, scope, scope_type, monthly_limit_usd, alert_threshold_pct, \
         created_by, \
         TO_CHAR(created_at, 'YYYY-MM-DD HH24:MI:SS') as created_at, \
         TO_CHAR(updated_at, 'YYYY-MM-DD HH24:MI:SS') as updated_at \
         FROM budgets ORDER BY scope_type, scope",
    )
    .fetch_all(&state.db)
    .await?;
    Ok(Json(budgets))
}

/// POST /api/admin/budgets
pub async fn create_budget(
    admin: AdminUser,
    State(state): State<crate::AppState>,
    Json(req): Json<CreateBudgetRequest>,
) -> Result<Json<Budget>, AppError> {
    if req.scope_type != "user" && req.scope_type != "org" {
        return Err(AppError::BadRequest(
            "scope_type must be 'user' or 'org'".into(),
        ));
    }

    let threshold = req.alert_threshold_pct.unwrap_or(80);

    sqlx::query(
        "INSERT INTO budgets (scope, scope_type, monthly_limit_usd, alert_threshold_pct, created_by) \
         VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT (scope, scope_type) DO UPDATE SET \
           monthly_limit_usd = EXCLUDED.monthly_limit_usd, \
           alert_threshold_pct = EXCLUDED.alert_threshold_pct, \
           created_by = EXCLUDED.created_by, \
           updated_at = NOW()",
    )
    .bind(&req.scope)
    .bind(&req.scope_type)
    .bind(req.monthly_limit_usd)
    .bind(threshold)
    .bind(&admin.0.sub)
    .execute(&state.db)
    .await?;

    let budget = sqlx::query_as::<_, Budget>(
        "SELECT id, scope, scope_type, monthly_limit_usd, alert_threshold_pct, \
         created_by, \
         TO_CHAR(created_at, 'YYYY-MM-DD HH24:MI:SS') as created_at, \
         TO_CHAR(updated_at, 'YYYY-MM-DD HH24:MI:SS') as updated_at \
         FROM budgets WHERE scope = $1 AND scope_type = $2",
    )
    .bind(&req.scope)
    .bind(&req.scope_type)
    .fetch_one(&state.db)
    .await?;

    // Audit: admin.budget_create
    crate::audit::log_event(
        &state.db,
        "admin.budget_create",
        &admin.0.sub,
        Some(&req.scope),
        serde_json::json!({ "budget_id": budget.id, "scope_type": &req.scope_type, "monthly_limit_usd": req.monthly_limit_usd }),
        None,
    )
    .await;

    Ok(Json(budget))
}

/// PUT /api/admin/budgets/{id}
pub async fn update_budget(
    _admin: AdminUser,
    State(state): State<crate::AppState>,
    Path(id): Path<i64>,
    Json(req): Json<UpdateBudgetRequest>,
) -> Result<Json<Budget>, AppError> {
    let mut sets = Vec::new();
    let mut param_idx = 2u32; // $1 is id

    if req.monthly_limit_usd.is_some() {
        sets.push(format!("monthly_limit_usd = ${param_idx}"));
        param_idx += 1;
    }
    if req.alert_threshold_pct.is_some() {
        sets.push(format!("alert_threshold_pct = ${param_idx}"));
        param_idx += 1;
    }

    if sets.is_empty() {
        return Err(AppError::BadRequest("No fields to update".into()));
    }

    sets.push("updated_at = NOW()".into());
    let _ = param_idx;

    let sql = format!(
        "UPDATE budgets SET {} WHERE id = $1 \
         RETURNING id, scope, scope_type, monthly_limit_usd, alert_threshold_pct, \
         created_by, \
         TO_CHAR(created_at, 'YYYY-MM-DD HH24:MI:SS') as created_at, \
         TO_CHAR(updated_at, 'YYYY-MM-DD HH24:MI:SS') as updated_at",
        sets.join(", ")
    );

    let mut query = sqlx::query_as::<_, Budget>(&sql).bind(id);

    if let Some(limit) = req.monthly_limit_usd {
        query = query.bind(limit);
    }
    if let Some(threshold) = req.alert_threshold_pct {
        query = query.bind(threshold);
    }

    let budget = query.fetch_optional(&state.db).await?;

    match budget {
        Some(b) => {
            // Audit: admin.budget_update
            crate::audit::log_event(
                &state.db,
                "admin.budget_update",
                &_admin.0.sub,
                Some(&b.scope),
                serde_json::json!({ "budget_id": b.id }),
                None,
            )
            .await;
            Ok(Json(b))
        }
        None => Err(AppError::NotFound("Budget not found".into())),
    }
}

/// DELETE /api/admin/budgets/{id}
pub async fn delete_budget(
    _admin: AdminUser,
    State(state): State<crate::AppState>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Fetch scope before deleting so we can log it
    let budget_scope: Option<String> =
        sqlx::query_scalar("SELECT scope FROM budgets WHERE id = $1")
            .bind(id)
            .fetch_optional(&state.db)
            .await?;

    let result = sqlx::query("DELETE FROM budgets WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Budget not found".into()));
    }

    // Audit: admin.budget_delete
    crate::audit::log_event(
        &state.db,
        "admin.budget_delete",
        &_admin.0.sub,
        budget_scope.as_deref(),
        serde_json::json!({ "budget_id": id }),
        None,
    )
    .await;

    Ok(Json(serde_json::json!({ "deleted": true })))
}

// ── Budget check (called at task creation) ──

/// Check if the user or their org has exceeded their monthly budget.
/// Uses real cost (total_cost_usd) reported by Claude CLI.
pub async fn check_budget(db: &PgPool, github_login: &str, org: &str) -> Result<(), AppError> {
    // Check user-level budget
    if let Some(budget) = load_budget(db, github_login, "user").await? {
        let spent = current_month_cost_for_user(db, github_login).await?;
        if spent >= budget.monthly_limit_usd {
            return Err(AppError::BadRequest(format!(
                "Monthly budget exceeded for user '{}': ${:.2} spent of ${:.2} limit",
                github_login, spent, budget.monthly_limit_usd
            )));
        }
        let threshold_usd =
            budget.monthly_limit_usd * (budget.alert_threshold_pct as f64 / 100.0);
        if spent >= threshold_usd {
            tracing::warn!(
                "Budget alert: user '{}' has spent ${:.2} of ${:.2} monthly limit ({}% threshold reached)",
                github_login, spent, budget.monthly_limit_usd, budget.alert_threshold_pct
            );
        }
    }

    // Check org-level budget
    if !org.is_empty() {
        if let Some(budget) = load_budget(db, org, "org").await? {
            let spent = current_month_cost_for_org(db).await?;
            if spent >= budget.monthly_limit_usd {
                return Err(AppError::BadRequest(format!(
                    "Monthly budget exceeded for org '{}': ${:.2} spent of ${:.2} limit",
                    org, spent, budget.monthly_limit_usd
                )));
            }
            let threshold_usd =
                budget.monthly_limit_usd * (budget.alert_threshold_pct as f64 / 100.0);
            if spent >= threshold_usd {
                tracing::warn!(
                    "Budget alert: org '{}' has spent ${:.2} of ${:.2} monthly limit ({}% threshold reached)",
                    org, spent, budget.monthly_limit_usd, budget.alert_threshold_pct
                );
            }
        }
    }

    Ok(())
}

async fn load_budget(
    db: &PgPool,
    scope: &str,
    scope_type: &str,
) -> Result<Option<Budget>, AppError> {
    let budget = sqlx::query_as::<_, Budget>(
        "SELECT id, scope, scope_type, monthly_limit_usd, alert_threshold_pct, \
         created_by, \
         TO_CHAR(created_at, 'YYYY-MM-DD HH24:MI:SS') as created_at, \
         TO_CHAR(updated_at, 'YYYY-MM-DD HH24:MI:SS') as updated_at \
         FROM budgets WHERE scope = $1 AND scope_type = $2",
    )
    .bind(scope)
    .bind(scope_type)
    .fetch_optional(db)
    .await?;
    Ok(budget)
}

async fn current_month_cost_for_user(
    db: &PgPool,
    github_login: &str,
) -> Result<f64, AppError> {
    let row: (f64,) = sqlx::query_as(
        "SELECT COALESCE(SUM(total_cost_usd), 0)::DOUBLE PRECISION \
         FROM tasks \
         WHERE created_by = $1 \
           AND created_at >= DATE_TRUNC('month', NOW())",
    )
    .bind(github_login)
    .fetch_one(db)
    .await?;
    Ok(row.0)
}

async fn current_month_cost_for_org(
    db: &PgPool,
) -> Result<f64, AppError> {
    let row: (f64,) = sqlx::query_as(
        "SELECT COALESCE(SUM(total_cost_usd), 0)::DOUBLE PRECISION \
         FROM tasks \
         WHERE created_at >= DATE_TRUNC('month', NOW())",
    )
    .fetch_one(db)
    .await?;
    Ok(row.0)
}
