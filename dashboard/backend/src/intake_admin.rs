use axum::extract::{Path, State};
use axum::Json;

use crate::auth::AdminUser;
use crate::error::AppError;
use crate::models::{
    CreateIntakeSourceRequest, IntakeIssue, IntakeIssueWithDetails, IntakePollLog, IntakeSource,
    UpdateIntakeIssueStatusRequest, UpdateIntakeSourceRequest,
};

const SOURCE_COLUMNS: &str = "id, name, provider, enabled, config, target_repo, \
     target_base_branch, label_filter, prompt_template, run_as_user, \
     poll_interval_secs, max_concurrent_tasks, max_tasks_per_poll, \
     auto_create_pr, created_by, \
     TO_CHAR(created_at, 'YYYY-MM-DD HH24:MI:SS') as created_at, \
     TO_CHAR(updated_at, 'YYYY-MM-DD HH24:MI:SS') as updated_at";

const ISSUE_COLUMNS: &str = "id, source_id, external_id, external_url, external_title, \
     external_body, external_labels, \
     TO_CHAR(external_updated_at, 'YYYY-MM-DD HH24:MI:SS') as external_updated_at, \
     task_id, status, error_message, \
     TO_CHAR(created_at, 'YYYY-MM-DD HH24:MI:SS') as created_at, \
     TO_CHAR(updated_at, 'YYYY-MM-DD HH24:MI:SS') as updated_at";

const LOG_COLUMNS: &str = "id, source_id, \
     TO_CHAR(polled_at, 'YYYY-MM-DD HH24:MI:SS') as polled_at, \
     issues_found, issues_created, issues_skipped, error_message, duration_ms";

/// GET /api/admin/intake/sources
pub async fn list_sources(
    _admin: AdminUser,
    State(state): State<crate::AppState>,
) -> Result<Json<Vec<IntakeSource>>, AppError> {
    let sources = sqlx::query_as::<_, IntakeSource>(&format!(
        "SELECT {SOURCE_COLUMNS} FROM intake_sources ORDER BY created_at DESC"
    ))
    .fetch_all(&state.db)
    .await?;
    Ok(Json(sources))
}

/// POST /api/admin/intake/sources
pub async fn create_source(
    admin: AdminUser,
    State(state): State<crate::AppState>,
    Json(req): Json<CreateIntakeSourceRequest>,
) -> Result<Json<IntakeSource>, AppError> {
    // Validate that run_as_user exists and has a GitHub token
    let _: String = sqlx::query_scalar("SELECT github_token FROM users WHERE github_login = $1")
        .bind(&req.run_as_user)
        .fetch_one(&state.db)
        .await
        .map_err(|_| {
            AppError::BadRequest(format!(
                "User '{}' not found or has no GitHub token",
                req.run_as_user
            ))
        })?;

    let config = req.config.unwrap_or(serde_json::json!({}));
    let target_base_branch = req.target_base_branch.unwrap_or_else(|| "main".into());
    let label_filter = req.label_filter.unwrap_or_default();
    let poll_interval_secs = req.poll_interval_secs.unwrap_or(300);
    let max_concurrent_tasks = req.max_concurrent_tasks.unwrap_or(3);
    let max_tasks_per_poll = req.max_tasks_per_poll.unwrap_or(5);
    let auto_create_pr = req.auto_create_pr.unwrap_or(false);

    let source = sqlx::query_as::<_, IntakeSource>(&format!(
        "INSERT INTO intake_sources \
         (name, provider, config, target_repo, target_base_branch, \
          label_filter, prompt_template, run_as_user, poll_interval_secs, \
          max_concurrent_tasks, max_tasks_per_poll, auto_create_pr, created_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13) \
         RETURNING {SOURCE_COLUMNS}"
    ))
    .bind(&req.name)
    .bind(&req.provider)
    .bind(&config)
    .bind(&req.target_repo)
    .bind(&target_base_branch)
    .bind(&label_filter)
    .bind(&req.prompt_template)
    .bind(&req.run_as_user)
    .bind(poll_interval_secs)
    .bind(max_concurrent_tasks)
    .bind(max_tasks_per_poll)
    .bind(auto_create_pr)
    .bind(&admin.0.sub)
    .fetch_one(&state.db)
    .await?;

    crate::audit::log_event(
        &state.db,
        "admin.intake_source_create",
        &admin.0.sub,
        Some(&req.target_repo),
        serde_json::json!({ "source_id": source.id, "name": &req.name }),
        None,
    )
    .await;

    Ok(Json(source))
}

/// PUT /api/admin/intake/sources/{id}
pub async fn update_source(
    _admin: AdminUser,
    State(state): State<crate::AppState>,
    Path(id): Path<i64>,
    Json(req): Json<UpdateIntakeSourceRequest>,
) -> Result<Json<IntakeSource>, AppError> {
    let mut sets = Vec::new();
    let mut param_idx = 2u32; // $1 is id

    if req.name.is_some() {
        sets.push(format!("name = ${param_idx}"));
        param_idx += 1;
    }
    if req.provider.is_some() {
        sets.push(format!("provider = ${param_idx}"));
        param_idx += 1;
    }
    if req.config.is_some() {
        sets.push(format!("config = ${param_idx}"));
        param_idx += 1;
    }
    if req.target_repo.is_some() {
        sets.push(format!("target_repo = ${param_idx}"));
        param_idx += 1;
    }
    if req.target_base_branch.is_some() {
        sets.push(format!("target_base_branch = ${param_idx}"));
        param_idx += 1;
    }
    if req.label_filter.is_some() {
        sets.push(format!("label_filter = ${param_idx}"));
        param_idx += 1;
    }
    if req.prompt_template.is_some() {
        sets.push(format!("prompt_template = ${param_idx}"));
        param_idx += 1;
    }
    if req.run_as_user.is_some() {
        sets.push(format!("run_as_user = ${param_idx}"));
        param_idx += 1;
    }
    if req.poll_interval_secs.is_some() {
        sets.push(format!("poll_interval_secs = ${param_idx}"));
        param_idx += 1;
    }
    if req.max_concurrent_tasks.is_some() {
        sets.push(format!("max_concurrent_tasks = ${param_idx}"));
        param_idx += 1;
    }
    if req.max_tasks_per_poll.is_some() {
        sets.push(format!("max_tasks_per_poll = ${param_idx}"));
        param_idx += 1;
    }
    if req.auto_create_pr.is_some() {
        sets.push(format!("auto_create_pr = ${param_idx}"));
        param_idx += 1;
    }
    if sets.is_empty() {
        return Err(AppError::BadRequest("No fields to update".into()));
    }

    sets.push("updated_at = NOW()".into());
    let _ = param_idx;

    let sql = format!(
        "UPDATE intake_sources SET {} WHERE id = $1 RETURNING {SOURCE_COLUMNS}",
        sets.join(", ")
    );

    let mut query = sqlx::query_as::<_, IntakeSource>(&sql).bind(id);

    if let Some(ref name) = req.name {
        query = query.bind(name);
    }
    if let Some(ref provider) = req.provider {
        query = query.bind(provider);
    }
    if let Some(ref config) = req.config {
        query = query.bind(config);
    }
    if let Some(ref target_repo) = req.target_repo {
        query = query.bind(target_repo);
    }
    if let Some(ref target_base_branch) = req.target_base_branch {
        query = query.bind(target_base_branch);
    }
    if let Some(ref label_filter) = req.label_filter {
        query = query.bind(label_filter);
    }
    if let Some(ref prompt_template) = req.prompt_template {
        query = query.bind(prompt_template);
    }
    if let Some(ref run_as_user) = req.run_as_user {
        query = query.bind(run_as_user);
    }
    if let Some(poll_interval_secs) = req.poll_interval_secs {
        query = query.bind(poll_interval_secs);
    }
    if let Some(max_concurrent_tasks) = req.max_concurrent_tasks {
        query = query.bind(max_concurrent_tasks);
    }
    if let Some(max_tasks_per_poll) = req.max_tasks_per_poll {
        query = query.bind(max_tasks_per_poll);
    }
    if let Some(auto_create_pr) = req.auto_create_pr {
        query = query.bind(auto_create_pr);
    }
    let source = query.fetch_optional(&state.db).await?;

    match source {
        Some(s) => {
            crate::audit::log_event(
                &state.db,
                "admin.intake_source_update",
                &_admin.0.sub,
                Some(&s.target_repo),
                serde_json::json!({ "source_id": s.id }),
                None,
            )
            .await;
            Ok(Json(s))
        }
        None => Err(AppError::NotFound("Intake source not found".into())),
    }
}

/// DELETE /api/admin/intake/sources/{id}
pub async fn delete_source(
    _admin: AdminUser,
    State(state): State<crate::AppState>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    let source_name: Option<String> =
        sqlx::query_scalar("SELECT name FROM intake_sources WHERE id = $1")
            .bind(id)
            .fetch_optional(&state.db)
            .await?;

    let result = sqlx::query("DELETE FROM intake_sources WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Intake source not found".into()));
    }

    if let Some(name) = source_name {
        crate::audit::log_event(
            &state.db,
            "admin.intake_source_delete",
            &_admin.0.sub,
            None,
            serde_json::json!({ "source_id": id, "name": name }),
            None,
        )
        .await;
    }

    Ok(Json(serde_json::json!({ "deleted": true })))
}

/// POST /api/admin/intake/sources/{id}/toggle
pub async fn toggle_source(
    _admin: AdminUser,
    State(state): State<crate::AppState>,
    Path(id): Path<i64>,
) -> Result<Json<IntakeSource>, AppError> {
    let source = sqlx::query_as::<_, IntakeSource>(&format!(
        "UPDATE intake_sources SET enabled = NOT enabled, updated_at = NOW() \
         WHERE id = $1 RETURNING {SOURCE_COLUMNS}"
    ))
    .bind(id)
    .fetch_optional(&state.db)
    .await?;

    match source {
        Some(s) => {
            crate::audit::log_event(
                &state.db,
                "admin.intake_source_toggle",
                &_admin.0.sub,
                Some(&s.target_repo),
                serde_json::json!({ "source_id": s.id, "enabled": s.enabled }),
                None,
            )
            .await;
            Ok(Json(s))
        }
        None => Err(AppError::NotFound("Intake source not found".into())),
    }
}

/// GET /api/admin/intake/sources/{id}/issues
pub async fn list_source_issues(
    _admin: AdminUser,
    State(state): State<crate::AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Vec<IntakeIssue>>, AppError> {
    let issues = sqlx::query_as::<_, IntakeIssue>(&format!(
        "SELECT {ISSUE_COLUMNS} FROM intake_issues WHERE source_id = $1 ORDER BY created_at DESC"
    ))
    .bind(id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(issues))
}

/// GET /api/admin/intake/sources/{id}/logs
pub async fn list_source_logs(
    _admin: AdminUser,
    State(state): State<crate::AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Vec<IntakePollLog>>, AppError> {
    let logs = sqlx::query_as::<_, IntakePollLog>(&format!(
        "SELECT {LOG_COLUMNS} FROM intake_poll_log WHERE source_id = $1 \
         ORDER BY polled_at DESC LIMIT 100"
    ))
    .bind(id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(logs))
}

/// POST /api/admin/intake/sources/{id}/test
pub async fn test_poll_source(
    _admin: AdminUser,
    State(state): State<crate::AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    // Load the source
    let source = sqlx::query_as::<_, IntakeSource>(&format!(
        "SELECT {SOURCE_COLUMNS} FROM intake_sources WHERE id = $1"
    ))
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("Intake source not found".into()))?;

    // Get the GitHub token from the run_as_user's account
    let api_token: String =
        sqlx::query_scalar("SELECT github_token FROM users WHERE github_login = $1")
            .bind(&source.run_as_user)
            .fetch_one(&state.db)
            .await
            .map_err(|_| {
                AppError::BadRequest(format!(
                    "User '{}' not found or has no GitHub token",
                    source.run_as_user
                ))
            })?;

    // Use the same fetch_github_issues as the real daemon to ensure consistent results
    let issues = crate::intake::fetch_github_issues(&source, &api_token).await?;

    let previews: Vec<serde_json::Value> = issues
        .iter()
        .take(20)
        .map(|issue| {
            serde_json::json!({
                "title": &issue.title,
                "url": &issue.url,
                "number": &issue.id,
                "labels": &issue.labels,
                "created_at": "",
                "updated_at": issue.updated_at.as_deref().unwrap_or(""),
            })
        })
        .collect();

    Ok(Json(previews))
}

/// GET /api/admin/intake/issues
/// List all intake issues across all sources, joined with source name and task status.
pub async fn list_all_issues(
    _admin: AdminUser,
    State(state): State<crate::AppState>,
) -> Result<Json<Vec<IntakeIssueWithDetails>>, AppError> {
    let issues = sqlx::query_as::<_, IntakeIssueWithDetails>(
        "SELECT i.id, i.source_id, i.external_id, i.external_url, i.external_title, \
             i.external_body, i.external_labels, \
             TO_CHAR(i.external_updated_at, 'YYYY-MM-DD HH24:MI:SS') as external_updated_at, \
             i.task_id, i.status, i.error_message, \
             TO_CHAR(i.created_at, 'YYYY-MM-DD HH24:MI:SS') as created_at, \
             TO_CHAR(i.updated_at, 'YYYY-MM-DD HH24:MI:SS') as updated_at, \
             s.name as source_name, \
             s.target_repo as source_repo, \
             t.status as task_status \
         FROM intake_issues i \
         JOIN intake_sources s ON s.id = i.source_id \
         LEFT JOIN tasks t ON t.id = i.task_id \
         ORDER BY i.updated_at DESC",
    )
    .fetch_all(&state.db)
    .await?;
    Ok(Json(issues))
}

/// Returns the set of statuses reachable from the given status via the UI.
/// `task_created` is daemon-only and cannot be targeted from the UI.
/// `done` is terminal.
fn valid_transitions(from: &str) -> &'static [&'static str] {
    match from {
        "backlog" => &["pending", "done"],
        "pending" => &["backlog"],
        "task_created" => &["failed"],
        "review" => &["done", "failed"],
        "failed" => &["backlog", "pending"],
        _ => &[],
    }
}

/// PATCH /api/admin/intake/issues/{id}/status
/// Update the status of an intake issue with transition validation.
pub async fn update_issue_status(
    _admin: AdminUser,
    State(state): State<crate::AppState>,
    Path(id): Path<i64>,
    Json(req): Json<UpdateIntakeIssueStatusRequest>,
) -> Result<Json<IntakeIssue>, AppError> {
    // Fetch current status
    let current: Option<(String,)> =
        sqlx::query_as("SELECT status FROM intake_issues WHERE id = $1")
            .bind(id)
            .fetch_optional(&state.db)
            .await?;

    let current_status = match current {
        Some((s,)) => s,
        None => return Err(AppError::NotFound("Intake issue not found".into())),
    };

    let allowed = valid_transitions(&current_status);
    if !allowed.contains(&req.status.as_str()) {
        return Err(AppError::BadRequest(format!(
            "Cannot transition from '{}' to '{}'. Valid targets: {}",
            current_status,
            req.status,
            if allowed.is_empty() {
                "none (terminal status)".to_string()
            } else {
                allowed.join(", ")
            }
        )));
    }

    // When retrying (failed → backlog/pending), clear task_id and error_message
    let clear_task =
        current_status == "failed" && (req.status == "backlog" || req.status == "pending");

    let issue = if clear_task {
        sqlx::query_as::<_, IntakeIssue>(&format!(
            "UPDATE intake_issues SET status = $1, task_id = NULL, error_message = NULL, \
             updated_at = NOW() WHERE id = $2 RETURNING {ISSUE_COLUMNS}"
        ))
        .bind(&req.status)
        .bind(id)
        .fetch_optional(&state.db)
        .await?
    } else {
        sqlx::query_as::<_, IntakeIssue>(&format!(
            "UPDATE intake_issues SET status = $1, updated_at = NOW() \
             WHERE id = $2 RETURNING {ISSUE_COLUMNS}"
        ))
        .bind(&req.status)
        .bind(id)
        .fetch_optional(&state.db)
        .await?
    };

    match issue {
        Some(i) => Ok(Json(i)),
        None => Err(AppError::NotFound("Intake issue not found".into())),
    }
}
