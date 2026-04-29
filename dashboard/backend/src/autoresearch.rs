use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use axum::extract::{Path, State};
use axum::Json;
use dashmap::DashMap;
use regex::Regex;
use sqlx::PgPool;
use tokio::sync::broadcast;

use crate::auth::{check_repo_permission, AuthUser, MemberUser};
use crate::error::AppError;
use crate::expb;
use crate::models::{
    AutoresearchExperiment, AutoresearchMessage, AutoresearchRun, CreateAutoresearchRunRequest,
    SendMessageRequest,
};
use crate::shell;

pub type StopFlags = Arc<DashMap<String, Arc<AtomicBool>>>;

pub fn new_stop_flags() -> StopFlags {
    Arc::new(DashMap::new())
}

const RUN_QUERY: &str = "SELECT id, name, repo, base_branch, branch_name, agent_name, \
     benchmark_server_id, benchmark_command, benchmark_type, \
     ethrex_repo_path, benchmarks_repo_path, expb_baseline_metrics, \
     objective, metric_regex, optimization_direction, \
     target_files, frozen_files, max_experiments, time_budget_minutes, status, \
     baseline_metric, best_metric, total_experiments, accepted_experiments, \
     total_cost_usd, error_message, created_by, \
     TO_CHAR(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as created_at, \
     TO_CHAR(updated_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as updated_at, \
     pr_url, pr_number \
     FROM autoresearch_runs";

const EXP_QUERY: &str = "SELECT id, run_id, experiment_number, status, diff, \
     metric_value, metric_raw_output, accepted, hypothesis, claude_response, \
     input_tokens, output_tokens, cost_usd, duration_seconds, pr_url, \
     TO_CHAR(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as created_at, \
     mgas_avg, latency_avg_ms, latency_p50_ms, latency_p95_ms, latency_p99_ms, \
     expb_tier_reached \
     FROM autoresearch_experiments";

// ── API handlers ──

/// GET /api/autoresearch/runs
pub async fn list_runs(
    user: AuthUser,
    State(state): State<crate::AppState>,
) -> Result<Json<Vec<AutoresearchRun>>, AppError> {
    let runs = if user.0.role == "admin" {
        sqlx::query_as::<_, AutoresearchRun>(&format!(
            "{RUN_QUERY} ORDER BY created_at DESC LIMIT 100"
        ))
        .fetch_all(&state.db)
        .await?
    } else {
        sqlx::query_as::<_, AutoresearchRun>(&format!(
            "{RUN_QUERY} WHERE created_by = $1 ORDER BY created_at DESC LIMIT 100"
        ))
        .bind(&user.0.sub)
        .fetch_all(&state.db)
        .await?
    };
    Ok(Json(runs))
}

/// POST /api/autoresearch/runs
pub async fn create_run_handler(
    user: MemberUser,
    State(state): State<crate::AppState>,
    Json(req): Json<CreateAutoresearchRunRequest>,
) -> Result<Json<AutoresearchRun>, AppError> {
    check_repo_permission(
        &state.db,
        &user.0.sub,
        &req.repo,
        &user.0.role,
        &state.config.github_org,
    )
    .await?;

    let benchmark_type = req.benchmark_type.as_deref().unwrap_or("shell");
    match benchmark_type {
        "shell" => {
            if req
                .benchmark_command
                .as_deref()
                .map(|s| s.trim().is_empty())
                .unwrap_or(true)
            {
                return Err(AppError::BadRequest(
                    "benchmark_command is required for shell benchmark runs".into(),
                ));
            }
        }
        "expb" => {
            if req.benchmark_server_id.is_none() {
                return Err(AppError::BadRequest(
                    "benchmark_server_id is required for EXPB runs".into(),
                ));
            }
            if req
                .ethrex_repo_path
                .as_deref()
                .map(|s| s.trim().is_empty())
                .unwrap_or(true)
            {
                return Err(AppError::BadRequest(
                    "ethrex_repo_path is required for EXPB runs".into(),
                ));
            }
            if req
                .benchmarks_repo_path
                .as_deref()
                .map(|s| s.trim().is_empty())
                .unwrap_or(true)
            {
                return Err(AppError::BadRequest(
                    "benchmarks_repo_path is required for EXPB runs".into(),
                ));
            }
        }
        other => {
            return Err(AppError::BadRequest(format!(
                "Unknown benchmark_type '{other}' (expected 'shell' or 'expb')"
            )));
        }
    }

    if let Some(ref regex) = req.metric_regex {
        Regex::new(regex)
            .map_err(|e| AppError::BadRequest(format!("Invalid metric regex: {e}")))?;
    }

    if req.max_experiments.is_none() && req.time_budget_minutes.is_none() {
        return Err(AppError::BadRequest(
            "Provide at least one of max_experiments or time_budget_minutes".into(),
        ));
    }

    let id = uuid::Uuid::new_v4().to_string();
    let base_branch = req.base_branch.as_deref().unwrap_or("main");
    let branch_name = format!("autoresearch/{}", &id[..8]);

    sqlx::query(
        "INSERT INTO autoresearch_runs \
         (id, repo, base_branch, branch_name, benchmark_command, benchmark_type, \
          ethrex_repo_path, benchmarks_repo_path, objective, metric_regex, \
          optimization_direction, target_files, frozen_files, max_experiments, \
          time_budget_minutes, benchmark_server_id, created_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)",
    )
    .bind(&id)
    .bind(&req.repo)
    .bind(base_branch)
    .bind(&branch_name)
    .bind(&req.benchmark_command)
    .bind(benchmark_type)
    .bind(&req.ethrex_repo_path)
    .bind(&req.benchmarks_repo_path)
    .bind(&req.objective)
    .bind(&req.metric_regex)
    .bind(&req.optimization_direction)
    .bind(&req.target_files)
    .bind(&req.frozen_files)
    .bind(req.max_experiments)
    .bind(req.time_budget_minutes)
    .bind(req.benchmark_server_id)
    .bind(&user.0.sub)
    .execute(&state.db)
    .await?;

    let run = sqlx::query_as::<_, AutoresearchRun>(&format!("{RUN_QUERY} WHERE id = $1"))
        .bind(&id)
        .fetch_one(&state.db)
        .await?;

    crate::audit::log_event(
        &state.db,
        "autoresearch.created",
        &user.0.sub,
        Some(&id),
        serde_json::json!({ "repo": &req.repo, "benchmark_command": &req.benchmark_command }),
        None,
    )
    .await;

    // Create broadcast channel and stop flag
    let (tx, _) = broadcast::channel(4096);
    state.autoresearch_channels.insert(id.clone(), tx.clone());
    let stop_flag = Arc::new(AtomicBool::new(false));
    state
        .autoresearch_stop_flags
        .insert(id.clone(), stop_flag.clone());

    // Get user's GitHub token
    let github_token: String =
        sqlx::query_scalar("SELECT github_token FROM users WHERE github_login = $1")
            .bind(&user.0.sub)
            .fetch_one(&state.db)
            .await
            .map_err(|_| AppError::Auth("User not found".into()))?;

    // Spawn the pipeline
    let config = state.config.clone();
    let db = state.db.clone();
    let channels = state.autoresearch_channels.clone();
    let stop_flags = state.autoresearch_stop_flags.clone();
    let run_id = id.clone();

    tokio::spawn(async move {
        if let Err(e) =
            run_autoresearch_pipeline(&config, &db, &run_id, &github_token, tx, stop_flag).await
        {
            tracing::error!("Autoresearch run {run_id} failed: {e}");
            let _ = sqlx::query(
                "UPDATE autoresearch_runs SET status = 'failed', error_message = $2, updated_at = NOW() WHERE id = $1",
            )
            .bind(&run_id)
            .bind(format!("{e}"))
            .execute(&db)
            .await;
            // Pipeline cleanup is handled inside run_autoresearch_pipeline on success/stop,
            // but on error the agent might still be running. Try to clean up.
            let agent_name: Option<String> =
                sqlx::query_scalar("SELECT agent_name FROM autoresearch_runs WHERE id = $1")
                    .bind(&run_id)
                    .fetch_one(&db)
                    .await
                    .ok()
                    .flatten();
            if let Some(name) = agent_name {
                let _ = shell::destroy_agent(&config, &name).await;
            }
            // Release benchmark server
            let server_id: Option<i64> = sqlx::query_scalar(
                "SELECT benchmark_server_id FROM autoresearch_runs WHERE id = $1",
            )
            .bind(&run_id)
            .fetch_one(&db)
            .await
            .ok()
            .flatten();
            if let Some(sid) = server_id {
                let _ = sqlx::query(
                    "UPDATE benchmark_servers SET status = 'ready', updated_at = NOW() WHERE id = $1",
                )
                .bind(sid)
                .execute(&db)
                .await;
            }
        }

        // Cleanup channels after a delay
        let ch = channels;
        let sf = stop_flags;
        let rid = run_id;
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            ch.remove(&rid);
            sf.remove(&rid);
        });
    });

    Ok(Json(run))
}

/// GET /api/autoresearch/runs/{id}
pub async fn get_run(
    user: AuthUser,
    State(state): State<crate::AppState>,
    Path(id): Path<String>,
) -> Result<Json<AutoresearchRun>, AppError> {
    let run = sqlx::query_as::<_, AutoresearchRun>(&format!("{RUN_QUERY} WHERE id = $1"))
        .bind(&id)
        .fetch_one(&state.db)
        .await
        .map_err(|_| AppError::NotFound("Run not found".into()))?;

    if user.0.role != "admin" && run.created_by.as_deref() != Some(&user.0.sub) {
        return Err(AppError::Forbidden("Not authorized".into()));
    }

    Ok(Json(run))
}

/// GET /api/autoresearch/runs/{id}/experiments
pub async fn list_experiments(
    user: AuthUser,
    State(state): State<crate::AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<AutoresearchExperiment>>, AppError> {
    let run = sqlx::query_as::<_, AutoresearchRun>(&format!("{RUN_QUERY} WHERE id = $1"))
        .bind(&id)
        .fetch_one(&state.db)
        .await
        .map_err(|_| AppError::NotFound("Run not found".into()))?;

    if user.0.role != "admin" && run.created_by.as_deref() != Some(&user.0.sub) {
        return Err(AppError::Forbidden("Not authorized".into()));
    }

    let experiments = sqlx::query_as::<_, AutoresearchExperiment>(&format!(
        "{EXP_QUERY} WHERE run_id = $1 ORDER BY experiment_number ASC"
    ))
    .bind(&id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(experiments))
}

/// POST /api/autoresearch/runs/{id}/stop
pub async fn stop_run(
    user: AuthUser,
    State(state): State<crate::AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let run = sqlx::query_as::<_, AutoresearchRun>(&format!("{RUN_QUERY} WHERE id = $1"))
        .bind(&id)
        .fetch_one(&state.db)
        .await
        .map_err(|_| AppError::NotFound("Run not found".into()))?;

    if user.0.role != "admin" && run.created_by.as_deref() != Some(&user.0.sub) {
        return Err(AppError::Forbidden("Not authorized".into()));
    }

    if run.status != "running" && run.status != "setting_up" {
        return Err(AppError::BadRequest(format!(
            "Cannot stop a run in '{}' status",
            run.status
        )));
    }

    if let Some(flag) = state.autoresearch_stop_flags.get(&id) {
        flag.store(true, Ordering::Relaxed);
    }

    crate::audit::log_event(
        &state.db,
        "autoresearch.stopped",
        &user.0.sub,
        Some(&id),
        serde_json::json!({}),
        None,
    )
    .await;

    Ok(Json(serde_json::json!({ "status": "stopping" })))
}

/// GET /api/autoresearch/runs/{id}/stats
pub async fn get_run_stats(
    user: AuthUser,
    State(state): State<crate::AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let run = sqlx::query_as::<_, AutoresearchRun>(&format!("{RUN_QUERY} WHERE id = $1"))
        .bind(&id)
        .fetch_one(&state.db)
        .await
        .map_err(|_| AppError::NotFound("Run not found".into()))?;

    if user.0.role != "admin" && run.created_by.as_deref() != Some(&user.0.sub) {
        return Err(AppError::Forbidden("Not authorized".into()));
    }

    let improvement_pct = match (run.baseline_metric, run.best_metric) {
        (Some(baseline), Some(best)) if baseline != 0.0 => {
            let raw = ((best - baseline) / baseline.abs()) * 100.0;
            if run.optimization_direction.as_deref() == Some("lower") {
                -raw
            } else {
                raw
            }
        }
        _ => 0.0,
    };

    let elapsed_seconds: Option<f64> = sqlx::query_scalar(
        "SELECT EXTRACT(EPOCH FROM (NOW() - created_at)) FROM autoresearch_runs WHERE id = $1",
    )
    .bind(&id)
    .fetch_one(&state.db)
    .await
    .ok();

    let experiments_per_hour = match elapsed_seconds {
        Some(secs) if secs > 0.0 && run.total_experiments > 0 => {
            (run.total_experiments as f64 / secs) * 3600.0
        }
        _ => 0.0,
    };

    let accept_rate = if run.total_experiments > 0 {
        (run.accepted_experiments as f64 / run.total_experiments as f64) * 100.0
    } else {
        0.0
    };

    let est_remaining_minutes = if let Some(max_exp) = run.max_experiments {
        let remaining = max_exp - run.total_experiments;
        if experiments_per_hour > 0.0 {
            Some((remaining as f64 / experiments_per_hour) * 60.0)
        } else {
            None
        }
    } else if let Some(budget) = run.time_budget_minutes {
        elapsed_seconds.map(|secs| (budget as f64) - (secs / 60.0))
    } else {
        None
    };

    Ok(Json(serde_json::json!({
        "improvement_pct": improvement_pct,
        "experiments_per_hour": experiments_per_hour,
        "accept_rate": accept_rate,
        "est_remaining_minutes": est_remaining_minutes,
    })))
}

/// GET /api/autoresearch/runs/{id}/messages
pub async fn list_messages(
    user: AuthUser,
    State(state): State<crate::AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<AutoresearchMessage>>, AppError> {
    let run = sqlx::query_as::<_, AutoresearchRun>(&format!("{RUN_QUERY} WHERE id = $1"))
        .bind(&id)
        .fetch_one(&state.db)
        .await
        .map_err(|_| AppError::NotFound("Run not found".into()))?;

    if user.0.role != "admin" && run.created_by.as_deref() != Some(&user.0.sub) {
        return Err(AppError::Forbidden("Not authorized".into()));
    }

    let messages = sqlx::query_as::<_, AutoresearchMessage>(
        "SELECT id, run_id, sender, content, \
         TO_CHAR(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as created_at \
         FROM autoresearch_messages WHERE run_id = $1 ORDER BY id ASC",
    )
    .bind(&id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(messages))
}

/// POST /api/autoresearch/runs/{id}/messages
pub async fn send_message(
    user: MemberUser,
    State(state): State<crate::AppState>,
    Path(id): Path<String>,
    Json(req): Json<SendMessageRequest>,
) -> Result<Json<AutoresearchMessage>, AppError> {
    let run = sqlx::query_as::<_, AutoresearchRun>(&format!("{RUN_QUERY} WHERE id = $1"))
        .bind(&id)
        .fetch_one(&state.db)
        .await
        .map_err(|_| AppError::NotFound("Run not found".into()))?;

    if user.0.role != "admin" && run.created_by.as_deref() != Some(&user.0.sub) {
        return Err(AppError::Forbidden("Not authorized".into()));
    }

    if req.content.trim().is_empty() {
        return Err(AppError::BadRequest("Message cannot be empty".into()));
    }

    sqlx::query("INSERT INTO autoresearch_messages (run_id, sender, content) VALUES ($1, $2, $3)")
        .bind(&id)
        .bind(&user.0.sub)
        .bind(&req.content)
        .execute(&state.db)
        .await?;

    let message = sqlx::query_as::<_, AutoresearchMessage>(
        "SELECT id, run_id, sender, content, \
         TO_CHAR(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as created_at \
         FROM autoresearch_messages WHERE run_id = $1 ORDER BY id DESC LIMIT 1",
    )
    .bind(&id)
    .fetch_one(&state.db)
    .await?;

    // Also broadcast it so it shows in the logs
    if let Some(tx) = state.autoresearch_channels.get(&id) {
        let _ = tx.send(format!("[USER] {}: {}", user.0.sub, req.content));
    }

    Ok(Json(message))
}

/// POST /api/autoresearch/runs/{id}/create-pr
pub async fn create_run_pr(
    user: MemberUser,
    State(state): State<crate::AppState>,
    Path(id): Path<String>,
) -> Result<Json<AutoresearchRun>, AppError> {
    let run = sqlx::query_as::<_, AutoresearchRun>(&format!("{RUN_QUERY} WHERE id = $1"))
        .bind(&id)
        .fetch_one(&state.db)
        .await
        .map_err(|_| AppError::NotFound("Run not found".into()))?;

    if user.0.role != "admin" && run.created_by.as_deref() != Some(&user.0.sub) {
        return Err(AppError::Forbidden("Not authorized".into()));
    }

    if run.pr_url.is_some() {
        return Err(AppError::BadRequest(
            "PR already exists for this run".into(),
        ));
    }

    let branch_name = run
        .branch_name
        .as_deref()
        .ok_or_else(|| AppError::BadRequest("Run has no branch".into()))?;

    let github_token: String =
        sqlx::query_scalar("SELECT github_token FROM users WHERE github_login = $1")
            .bind(&user.0.sub)
            .fetch_one(&state.db)
            .await
            .map_err(|_| AppError::Auth("User not found".into()))?;

    // Build PR title and body
    let title = format!(
        "Autoresearch: {} optimizations for {}",
        run.accepted_experiments, run.repo
    );

    let improvement = match (run.baseline_metric, run.best_metric) {
        (Some(baseline), Some(best)) if baseline != 0.0 => {
            let raw = ((best - baseline) / baseline.abs()) * 100.0;
            if run.optimization_direction.as_deref() == Some("lower") {
                -raw
            } else {
                raw
            }
        }
        _ => 0.0,
    };

    let objective = run.objective.as_deref().unwrap_or("improve performance");
    let body = format!(
        "## Autoresearch Results\n\n\
         - **Objective:** {}\n\
         - **Baseline metric:** {}\n\
         - **Best metric:** {}\n\
         - **Improvement:** {:.1}%\n\
         - **Experiments:** {} total, {} accepted\n\
         - **Benchmark command:** `{}`\n\n\
         Generated automatically by Tekton Autoresearch.",
        objective,
        run.baseline_metric
            .map(|v| format!("{v}"))
            .unwrap_or("N/A".into()),
        run.best_metric
            .map(|v| format!("{v}"))
            .unwrap_or("N/A".into()),
        improvement,
        run.total_experiments,
        run.accepted_experiments,
        run.benchmark_command.as_deref().unwrap_or(""),
    );

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("https://api.github.com/repos/{}/pulls", run.repo))
        .header("Authorization", format!("token {github_token}"))
        .header("User-Agent", "tekton-dashboard")
        .header("Accept", "application/vnd.github+json")
        .json(&serde_json::json!({
            "title": title,
            "body": body,
            "head": branch_name,
            "base": run.base_branch,
        }))
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("GitHub API request failed: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(AppError::Internal(format!(
            "GitHub API returned {status}: {text}"
        )));
    }

    let pr_data: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to parse PR response: {e}")))?;

    let pr_url = pr_data["html_url"].as_str().unwrap_or("").to_string();
    let pr_number = pr_data["number"].as_i64().unwrap_or(0) as i32;

    sqlx::query("UPDATE autoresearch_runs SET pr_url = $2, pr_number = $3, updated_at = NOW() WHERE id = $1")
        .bind(&id)
        .bind(&pr_url)
        .bind(pr_number)
        .execute(&state.db)
        .await?;

    crate::audit::log_event(
        &state.db,
        "autoresearch.pr_created",
        &user.0.sub,
        Some(&id),
        serde_json::json!({ "pr_url": &pr_url, "pr_number": pr_number }),
        None,
    )
    .await;

    let updated = sqlx::query_as::<_, AutoresearchRun>(&format!("{RUN_QUERY} WHERE id = $1"))
        .bind(&id)
        .fetch_one(&state.db)
        .await?;

    Ok(Json(updated))
}

// ── Pipeline ──

async fn update_run_status(db: &sqlx::PgPool, id: &str, status: &str) -> Result<(), AppError> {
    sqlx::query("UPDATE autoresearch_runs SET status = $2, updated_at = NOW() WHERE id = $1")
        .bind(id)
        .bind(status)
        .execute(db)
        .await?;
    Ok(())
}

async fn log_and_persist(
    db: &sqlx::PgPool,
    tx: &broadcast::Sender<String>,
    run_id: &str,
    msg: &str,
) {
    let _ = tx.send(msg.to_string());
    let _ = sqlx::query("INSERT INTO autoresearch_logs (run_id, line) VALUES ($1, $2)")
        .bind(run_id)
        .bind(msg)
        .execute(db)
        .await;
}

async fn run_autoresearch_pipeline(
    config: &crate::config::Config,
    db: &sqlx::PgPool,
    run_id: &str,
    github_token: &str,
    tx: broadcast::Sender<String>,
    stop_flag: Arc<AtomicBool>,
) -> Result<(), AppError> {
    let run = sqlx::query_as::<_, AutoresearchRun>(&format!("{RUN_QUERY} WHERE id = $1"))
        .bind(run_id)
        .fetch_one(db)
        .await?;

    let branch_name = run.branch_name.as_deref().unwrap_or("autoresearch/unknown");
    let agent_name = format!("ar-{}", &run_id[..8]);

    // Background task: persist ALL broadcast messages to DB (including Claude streaming output)
    // This ensures logs survive tab switches / WebSocket reconnects.
    let persist_db = db.clone();
    let persist_run_id = run_id.to_string();
    let mut persist_rx = tx.subscribe();
    let persist_handle = tokio::spawn(async move {
        while let Ok(line) = persist_rx.recv().await {
            let _ = sqlx::query("INSERT INTO autoresearch_logs (run_id, line) VALUES ($1, $2)")
                .bind(&persist_run_id)
                .bind(&line)
                .execute(&persist_db)
                .await;
        }
    });

    // Phase 1: Setup
    update_run_status(db, run_id, "setting_up").await?;
    log_and_persist(db, &tx, run_id, "[SETUP] Creating agent container...").await;

    shell::create_agent(config, &agent_name).await?;
    sqlx::query("UPDATE autoresearch_runs SET agent_name = $2, updated_at = NOW() WHERE id = $1")
        .bind(run_id)
        .bind(&agent_name)
        .execute(db)
        .await?;

    // Helper closure for cleanup (called on success and error paths)
    async fn cleanup(
        config: &crate::config::Config,
        db: &sqlx::PgPool,
        agent_name: &str,
        server_id: Option<i64>,
    ) {
        let _ = shell::destroy_agent(config, agent_name).await;
        if let Some(sid) = server_id {
            let _ = sqlx::query(
                "UPDATE benchmark_servers SET status = 'ready', updated_at = NOW() WHERE id = $1",
            )
            .bind(sid)
            .execute(db)
            .await;
        }
    }

    // Clone repo
    log_and_persist(
        db,
        &tx,
        run_id,
        &format!("[SETUP] Cloning {}/{}...", run.repo, run.base_branch),
    )
    .await;

    let clone_url = format!(
        "https://x-access-token:{github_token}@github.com/{}.git",
        run.repo
    );
    shell::agent_exec_capture(
        &agent_name,
        &format!(
            "git clone --branch {} --single-branch {} /home/agent/repo && \
             cd /home/agent/repo && \
             git checkout -b {} && \
             git config user.name 'Autoresearch' && \
             git config user.email 'autoresearch@tekton'",
            run.base_branch, clone_url, branch_name
        ),
    )
    .await?;

    log_and_persist(db, &tx, run_id, "[SETUP] Repo cloned and branch created.").await;

    // If using a dedicated benchmark server, set up rsync
    let benchmark_server = if let Some(server_id) = run.benchmark_server_id {
        let server = sqlx::query_as::<_, crate::models::BenchmarkServer>(
            "SELECT id, name, hostname, ssh_user, ssh_key_path, hardware_description, \
             status, setup_log, error_message, created_by, \
             TO_CHAR(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as created_at, \
             TO_CHAR(updated_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as updated_at \
             FROM benchmark_servers WHERE id = $1",
        )
        .bind(server_id)
        .fetch_one(db)
        .await?;

        sqlx::query(
            "UPDATE benchmark_servers SET status = 'busy', updated_at = NOW() WHERE id = $1",
        )
        .bind(server_id)
        .execute(db)
        .await?;

        // The classic shell flow rsyncs the repo onto the benchmark server so
        // the host can run the benchmark command against it. Each iteration
        // will then `git fetch origin <exp-N>` from GitHub. EXPB runs maintain
        // their own ethrex clone on the host (`ethrex_repo_path`), so this
        // initial clone is unnecessary there.
        if run.benchmark_type != "expb" {
            log_and_persist(
                db,
                &tx,
                run_id,
                &format!(
                    "[SETUP] Cloning repo on benchmark server {}...",
                    server.name
                ),
            )
            .await;
            clone_on_benchmark_server(&server, &run.repo, &run.base_branch, github_token).await?;
            log_and_persist(db, &tx, run_id, "[SETUP] Repo cloned on benchmark server.").await;
        }

        Some(server)
    } else {
        None
    };

    let server_id_for_cleanup = benchmark_server.as_ref().map(|s| s.id);

    // Phase 2: Baseline benchmark
    let bench_start = Instant::now();
    let baseline_output = if run.benchmark_type == "expb" {
        let server = benchmark_server.as_ref().expect("expb validated server");
        log_and_persist(
            db,
            &tx,
            run_id,
            "[BASELINE] Running EXPB baselines on main: fast → gigablocks → slow (this will take a while)...",
        )
        .await;
        let ethrex_repo_path = run
            .ethrex_repo_path
            .as_deref()
            .ok_or_else(|| AppError::BadRequest("Run is missing ethrex_repo_path".into()))?;
        let benchmarks_repo_path = run
            .benchmarks_repo_path
            .as_deref()
            .ok_or_else(|| AppError::BadRequest("Run is missing benchmarks_repo_path".into()))?;
        run_expb_baseline(
            db,
            run_id,
            &run.base_branch,
            server,
            ethrex_repo_path,
            benchmarks_repo_path,
        )
        .await?
    } else {
        log_and_persist(
            db, &tx, run_id,
            &format!("[BASELINE] Running baseline benchmark: {}  (first run may take a while if compiling from scratch)", run.benchmark_command.as_deref().unwrap_or("")),
        ).await;
        run_benchmark(
            &agent_name,
            run.benchmark_command.as_deref().unwrap_or(""),
            benchmark_server.as_ref(),
        )
        .await?
    };
    let bench_secs = bench_start.elapsed().as_secs();

    log_and_persist(
        db,
        &tx,
        run_id,
        &format!("[BASELINE] Benchmark complete ({bench_secs}s). Sending output to Claude..."),
    )
    .await;

    // Phase 3: Start Claude conversation with the baseline output and objective
    // Build Claude auth env (API key / OAuth token)
    let created_by = run.created_by.as_deref().unwrap_or("system");
    let (auth_env, model_flag) =
        crate::tasks::build_claude_auth_env(db, &config.secrets_encryption_key, created_by).await?;

    update_run_status(db, run_id, "running").await?;
    let start_time = Instant::now();
    let mut experiment_number = 0i32;
    let mut best_metric: Option<f64> = None;
    let mut last_seen_message_id: i64 = 0;

    let base_branch = run.base_branch.clone();
    let objective = run.objective.as_deref().unwrap_or("improve performance");
    let target_desc = run.target_files.as_deref().unwrap_or("any relevant files");
    let frozen_desc = run
        .frozen_files
        .as_deref()
        .map(|f| format!("\nFROZEN FILES (do NOT modify these): {f}"))
        .unwrap_or_default();

    // Initial prompt with baseline output
    let initial_prompt = format!(
        "OBJECTIVE (this is the most important instruction — everything else \
         on this page is supporting context, not a substitute):\n\
         {objective}\n\
         \n\
         If your objective involves research (reading external URLs, surveying \
         ideas from another project, exploring an unfamiliar subsystem), do \
         that research NOW, before touching any code. Use WebFetch on URLs in \
         the objective, take notes on what you learn, and only then pick \
         which idea you will try first.\n\
         \n\
         Do NOT default to micro-optimizing the obvious hot file of the \
         benchmark. That is the most common failure mode of agents in this \
         loop, and it directly contradicts a research-shaped objective. The \
         metric below is how we measure whether a change helps — it is NOT \
         a license to ignore the objective and chase the hottest function. \
         A modest metric gain that genuinely advances the objective is more \
         valuable than a clever metric hack that doesn't.\n\
         \n\
         Context for grounding your work:\n\
         - Benchmark command: {}\n\
         - Target files (suggestion only — your objective may direct you elsewhere): {target_desc}{frozen_desc}\n\
         - BASELINE benchmark output:\n\
         ```\n\
         {}\n\
         ```\n\
         \n\
         IMPORTANT RULES:\n\
         - Do NOT run benchmarks or tests yourself — benchmarks are run for you on a dedicated server after each change.\n\
         - Focus on reading files (and external sources, when the objective calls for it) and making edits. You may compile to check your work.\n\
         - Make focused, targeted changes. One change per experiment.\n\
         - Do NOT modify the benchmark command or evaluation code.\n\
         - Do NOT \"optimize\" the metric by moving existing work into or out of the timed/measured \
         window (e.g. relocating validation, hashing, or DB I/O to before/after the benchmark's \
         start/stop instants, or behind a background thread that isn't waited on). The metric must \
         reflect a real reduction in work — not a relocation of work outside the measurement.\n\
         - NEVER ask questions — you are autonomous. Pick the most reasonable option and record it.\n\
         - When you make a significant choice (algorithm, approach, file to modify, tradeoff), record it on its own line as:\n\
         DECISION: <what you decided> | ALTERNATIVES: <other options you considered>\n\
         \n\
         End your response with exactly these lines:\n\
         METRIC: <the numeric value of the key metric from the baseline>\n\
         DESCRIPTION: <what the metric measures, e.g. \"execution time in ns\">\n\
         \n\
         After identifying the metric, make your first concrete change toward the objective.",
        run.benchmark_command.as_deref().unwrap_or(""),
        &baseline_output[..baseline_output.len().min(8000)],
    );

    let escaped = initial_prompt.replace('\'', "'\\''");
    log_and_persist(
        db,
        &tx,
        run_id,
        "[CLAUDE] Sending initial prompt (analyze baseline + first optimization)...",
    )
    .await;
    let claude_result = shell::agent_exec_claude_streaming(
        &agent_name,
        &format!(
            "{auth_env} && cd /home/agent/repo && claude --dangerously-skip-permissions --output-format stream-json --verbose {model_flag} -p '{escaped}'"
        ),
        tx.clone(),
    ).await;

    let claude_response = match claude_result {
        Ok(result) => result.text,
        Err(e) => {
            log_and_persist(
                db,
                &tx,
                run_id,
                &format!("[ERROR] Initial Claude call failed: {e}"),
            )
            .await;
            cleanup(config, db, &agent_name, server_id_for_cleanup).await;
            return Err(AppError::Internal(format!(
                "Initial Claude call failed: {e}"
            )));
        }
    };

    // Parse baseline metric from Claude's response
    let (baseline_metric, metric_description) = parse_metric_response(&claude_response);
    if let Some(bm) = baseline_metric {
        best_metric = Some(bm);
        sqlx::query(
            "UPDATE autoresearch_runs SET baseline_metric = $2, best_metric = $2, updated_at = NOW() WHERE id = $1",
        )
        .bind(run_id)
        .bind(bm)
        .execute(db)
        .await?;
        log_and_persist(
            db,
            &tx,
            run_id,
            &format!("[BASELINE] Metric: {bm} ({metric_description})"),
        )
        .await;
    } else {
        log_and_persist(
            db,
            &tx,
            run_id,
            "[BASELINE] Claude could not extract a metric. Continuing anyway.",
        )
        .await;
    }

    // Now enter the experiment loop
    loop {
        // Check stop conditions
        if stop_flag.load(Ordering::Relaxed) {
            log_and_persist(db, &tx, run_id, "[STOP] Run stopped by user.").await;
            update_run_status(db, run_id, "stopped").await?;
            cleanup(config, db, &agent_name, server_id_for_cleanup).await;
            return Ok(());
        }

        if let Some(max) = run.max_experiments {
            if experiment_number >= max {
                log_and_persist(
                    db,
                    &tx,
                    run_id,
                    &format!("[DONE] Reached max experiments ({max})."),
                )
                .await;
                break;
            }
        }

        if let Some(budget) = run.time_budget_minutes {
            if start_time.elapsed().as_secs() >= (budget as u64 * 60) {
                log_and_persist(
                    db,
                    &tx,
                    run_id,
                    &format!("[DONE] Time budget ({budget}m) exhausted."),
                )
                .await;
                break;
            }
        }

        experiment_number += 1;
        let exp_start = Instant::now();
        // Each experiment gets its own fresh branch off base. No state from
        // prior experiments is carried over — the GitHub branch starts at
        // base SHA and receives a single commit with this experiment's diff.
        let exp_branch = format!("autoresearch/{}/exp-{experiment_number}", &run_id[..8]);
        let mut exp_branch_created = false;
        log_and_persist(
            db,
            &tx,
            run_id,
            &format!("[EXP {experiment_number}] Starting experiment..."),
        )
        .await;

        let exp_id: i64 = sqlx::query_scalar(
            "INSERT INTO autoresearch_experiments (run_id, experiment_number) VALUES ($1, $2) RETURNING id",
        )
        .bind(run_id)
        .bind(experiment_number)
        .fetch_one(db)
        .await?;

        // Check if Claude made any changes (working tree OR committed since base)
        // Claude often commits its own changes via shell, so check diff vs origin/<base_branch>
        let diff = shell::agent_exec_capture(
            &agent_name,
            &format!("cd /home/agent/repo && git diff origin/{base_branch}"),
        )
        .await
        .unwrap_or_default();

        let diff_lines = diff.lines().count();
        if diff.trim().is_empty() {
            log_and_persist(
                db,
                &tx,
                run_id,
                &format!("[EXP {experiment_number}] Claude made no code changes, skipping."),
            )
            .await;
            let _ = sqlx::query(
                "UPDATE autoresearch_experiments SET status = 'rejected', accepted = false, \
                 claude_response = $2, diff = '' WHERE id = $1",
            )
            .bind(exp_id)
            .bind(&claude_response)
            .execute(db)
            .await;
            let _ = sqlx::query(
                "UPDATE autoresearch_runs SET total_experiments = total_experiments + 1, updated_at = NOW() WHERE id = $1",
            )
            .bind(run_id)
            .execute(db)
            .await;

            // Ask Claude to try again
            let retry_prompt = "You didn't make any code changes. Please make a concrete code change to optimize the metric.";
            let escaped = retry_prompt.replace('\'', "'\\''");
            let _ = shell::agent_exec_claude_streaming(
                &agent_name,
                &format!("{auth_env} && cd /home/agent/repo && claude --dangerously-skip-permissions --output-format stream-json --verbose {model_flag} --continue -p '{escaped}'"),
                tx.clone(),
            ).await;
            continue;
        }

        // Log what changed
        log_and_persist(
            db,
            &tx,
            run_id,
            &format!("[EXP {experiment_number}] Claude modified {diff_lines} lines. Committing..."),
        )
        .await;

        // Commit any uncommitted changes (Claude may have already committed via shell — that's fine)
        let _ = shell::agent_exec_capture(
            &agent_name,
            "cd /home/agent/repo && git add -A && (git diff --cached --quiet || git commit -m 'autoresearch experiment')",
        )
        .await;

        // Push this experiment's diff to its own dedicated branch on GitHub
        // (classic shell flow — EXPB does the same push inside
        // run_expb_experiment as part of the benchmark step below).
        if run.benchmark_type != "expb" {
            if let Some(ref server) = benchmark_server {
                log_and_persist(
                    db,
                    &tx,
                    run_id,
                    &format!("[EXP {experiment_number}] Pushing {exp_branch} to GitHub and fetching on benchmark server..."),
                )
                .await;
                push_verified(
                    &agent_name,
                    github_token,
                    &run.repo,
                    &exp_branch,
                    &run.base_branch,
                    &mut exp_branch_created,
                    "autoresearch experiment",
                )
                .await?;
                pull_on_benchmark_server(server, &exp_branch, github_token).await?;
            }
        }

        let _ = sqlx::query(
            "UPDATE autoresearch_experiments SET status = 'benchmarking' WHERE id = $1",
        )
        .bind(exp_id)
        .execute(db)
        .await;

        let bench_start = Instant::now();
        // For EXPB, push + tiered gate returns a synthetic output string, a
        // structural "should keep" decision, and the actual final-tier
        // mgas_avg measured on the bench server. For shell, run_benchmark
        // returns stdout and is_better is parsed later from Claude's response.
        let mut expb_keep_override: Option<bool> = None;
        let mut expb_metric_override: Option<f64> = None;
        let benchmark_result: Result<String, AppError> = if run.benchmark_type == "expb" {
            let server = benchmark_server.as_ref().expect("expb validated server");
            log_and_persist(
                db,
                &tx,
                run_id,
                &format!(
                    "[EXP {experiment_number}] Running tiered EXPB gate (fast → gigablocks → slow)..."
                ),
            )
            .await;
            // Re-fetch the run row so we see the baseline metrics that were
            // written during phase 2.
            let fresh_run =
                sqlx::query_as::<_, AutoresearchRun>(&format!("{RUN_QUERY} WHERE id = $1"))
                    .bind(run_id)
                    .fetch_one(db)
                    .await?;
            match run_expb_experiment(
                db,
                &agent_name,
                github_token,
                &mut exp_branch_created,
                exp_id,
                &fresh_run,
                server,
                &exp_branch,
            )
            .await
            {
                Ok((summary, keep, mgas)) => {
                    expb_keep_override = Some(keep);
                    expb_metric_override = mgas;
                    Ok(summary)
                }
                Err(e) => Err(e),
            }
        } else {
            log_and_persist(
                db,
                &tx,
                run_id,
                &format!(
                    "[EXP {experiment_number}] Running benchmark: {}...",
                    run.benchmark_command.as_deref().unwrap_or("")
                ),
            )
            .await;
            run_benchmark(
                &agent_name,
                run.benchmark_command.as_deref().unwrap_or(""),
                benchmark_server.as_ref(),
            )
            .await
        };
        let bench_secs = bench_start.elapsed().as_secs();

        // EXPB benchmark failures are always infrastructure (SSH push, git
        // fetch, docker build, expb wrapper crash) — never "your code is
        // bad". Don't burn a Claude turn on them: mark the experiment row
        // as errored, reset the agent's branch, and skip ahead to the next
        // iteration so a transient hiccup doesn't poison the comparison.
        if run.benchmark_type == "expb" {
            if let Err(ref e) = benchmark_result {
                log_and_persist(
                    db,
                    &tx,
                    run_id,
                    &format!(
                        "[EXP {experiment_number}] EXPB infrastructure error ({bench_secs}s): {e}\n\
                         Marking experiment as errored and continuing without sending to Claude."
                    ),
                )
                .await;
                let _ = sqlx::query(
                    "UPDATE autoresearch_experiments SET status = 'error', \
                     metric_raw_output = $2, duration_seconds = $3 WHERE id = $1",
                )
                .bind(exp_id)
                .bind(format!("{e}"))
                .bind(bench_secs as i32)
                .execute(db)
                .await;
                let _ = shell::agent_exec_capture(
                    &agent_name,
                    "cd /home/agent/repo && git reset --hard HEAD~1",
                )
                .await;
                continue;
            }
        }

        let raw_output = match &benchmark_result {
            Ok(output) => {
                let preview = if output.len() > 500 {
                    format!("{}...", &output[..500])
                } else {
                    output.clone()
                };
                log_and_persist(db, &tx, run_id, &format!("[EXP {experiment_number}] Benchmark finished ({bench_secs}s). Output:\n{preview}")).await;
                output.clone()
            }
            Err(e) => {
                log_and_persist(
                    db,
                    &tx,
                    run_id,
                    &format!("[EXP {experiment_number}] Benchmark failed ({bench_secs}s): {e}"),
                )
                .await;
                format!("Benchmark failed: {e}")
            }
        };

        // Check for user messages (suggestions/guidance)
        let user_messages: Vec<AutoresearchMessage> = sqlx::query_as(
            "SELECT id, run_id, sender, content, \
             TO_CHAR(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as created_at \
             FROM autoresearch_messages WHERE run_id = $1 AND id > $2 ORDER BY id ASC",
        )
        .bind(run_id)
        .bind(last_seen_message_id)
        .fetch_all(db)
        .await
        .unwrap_or_default();

        let mut user_guidance = String::new();
        for msg in &user_messages {
            last_seen_message_id = msg.id;
            user_guidance.push_str(&format!(
                "\n\nUSER SUGGESTION from {}: {}",
                msg.sender, msg.content
            ));
            log_and_persist(
                db,
                &tx,
                run_id,
                &format!("[USER] {}: {}", msg.sender, msg.content),
            )
            .await;
        }

        // Send benchmark output to Claude and ask for analysis + next optimization
        let best_str = best_metric
            .map(|v| format!("{v}"))
            .unwrap_or("unknown".into());
        let continue_prompt = format!(
            "REMINDER OF YOUR OBJECTIVE (do not lose sight of this — it overrides the obvious \
             local hot-path-optimization instinct):\n\
             {objective}\n\
             \n\
             Here is the benchmark output after your last change:\n\
             ```\n\
             {}\n\
             ```\n\
             \n\
             Previous best metric: {best_str}\n\
             {user_guidance}\n\
             \n\
             Respond with exactly these lines at the END of your response:\n\
             METRIC: <the numeric value of the key metric from this run>\n\
             IMPROVED: <true or false — is this better than the previous best?>\n\
             \n\
             Then make another optimization — a single focused change that advances the OBJECTIVE \
             above. If the objective involves research (reading external sources, surveying ideas), \
             do that research now if you haven't already, and pick the next idea from it; do not \
             default to repeatedly micro-optimizing the same hot file. \
             Do NOT relocate existing work into or out of the timed/measured window to game the \
             metric — the change must reduce real work, not hide it from the benchmark.",
            &raw_output[..raw_output.len().min(8000)],
        );

        let escaped = continue_prompt.replace('\'', "'\\''");
        log_and_persist(db, &tx, run_id, &format!("[EXP {experiment_number}] Sending benchmark output to Claude for analysis + next optimization...")).await;
        let claude_result = shell::agent_exec_claude_streaming(
            &agent_name,
            &format!("{auth_env} && cd /home/agent/repo && claude --dangerously-skip-permissions --output-format stream-json --verbose {model_flag} --continue -p '{escaped}'"),
            tx.clone(),
        ).await;

        let claude_response = match claude_result {
            Ok(result) => result.text,
            Err(e) => {
                log_and_persist(
                    db,
                    &tx,
                    run_id,
                    &format!("[EXP {experiment_number}] Claude error: {e}"),
                )
                .await;
                let _ = sqlx::query(
                    "UPDATE autoresearch_experiments SET status = 'error', claude_response = $2, diff = $3 WHERE id = $1",
                )
                .bind(exp_id)
                .bind(format!("{e}"))
                .bind(&diff)
                .execute(db)
                .await;
                let _ = sqlx::query(
                    "UPDATE autoresearch_runs SET total_experiments = total_experiments + 1, updated_at = NOW() WHERE id = $1",
                )
                .bind(run_id)
                .execute(db)
                .await;
                // Revert on error
                let _ = shell::agent_exec_capture(
                    &agent_name,
                    "cd /home/agent/repo && git reset --hard HEAD~1",
                )
                .await;
                continue;
            }
        };

        // Parse Claude's response for metric and improvement judgment.
        // For EXPB runs we ignore both Claude's METRIC: line and its IMPROVED:
        // flag — the agent has hallucinated metric values in past runs (e.g.
        // claimed 720 mgas/sec when the actual measurement was 673). Instead
        // we trust the structural tiered-gate result and the final-tier
        // mgas_avg measured by the EXPB harness.
        let (claude_metric_value, _) = parse_metric_response(&claude_response);
        let metric_value = expb_metric_override.or(claude_metric_value);
        let is_better =
            expb_keep_override.unwrap_or_else(|| parse_improved_response(&claude_response));

        let duration = exp_start.elapsed().as_secs() as i32;

        if is_better {
            let val = metric_value.unwrap_or(0.0);
            log_and_persist(
                db,
                &tx,
                run_id,
                &format!("[EXP {experiment_number}] ACCEPTED: {val} (best was {best_str})"),
            )
            .await;
            best_metric = Some(val);

            // The experiment's branch was already created and pushed at the
            // top of this iteration; nothing more to push here.
            log_and_persist(
                db,
                &tx,
                run_id,
                &format!("[EXP {experiment_number}] Branch {exp_branch} kept on GitHub."),
            )
            .await;
            let pr_url: Option<String> = None;

            let _ = sqlx::query(
                "UPDATE autoresearch_experiments SET status = 'accepted', accepted = true, \
                 diff = $2, metric_value = $3, metric_raw_output = $4, \
                 claude_response = $5, duration_seconds = $6, pr_url = $7 WHERE id = $1",
            )
            .bind(exp_id)
            .bind(&diff)
            .bind(val)
            .bind(&raw_output)
            .bind(&claude_response)
            .bind(duration)
            .bind(&pr_url)
            .execute(db)
            .await;

            let _ = sqlx::query(
                "UPDATE autoresearch_runs SET best_metric = $2, \
                 total_experiments = total_experiments + 1, \
                 accepted_experiments = accepted_experiments + 1, \
                 updated_at = NOW() WHERE id = $1",
            )
            .bind(run_id)
            .bind(val)
            .execute(db)
            .await;
        } else {
            let val_str = metric_value.map(|v| format!("{v}")).unwrap_or("N/A".into());
            log_and_persist(
                db,
                &tx,
                run_id,
                &format!("[EXP {experiment_number}] REJECTED: {val_str} (best: {best_str})"),
            )
            .await;

            let _ = sqlx::query(
                "UPDATE autoresearch_experiments SET status = 'rejected', accepted = false, \
                 diff = $2, metric_value = $3, metric_raw_output = $4, \
                 claude_response = $5, duration_seconds = $6 WHERE id = $1",
            )
            .bind(exp_id)
            .bind(&diff)
            .bind(metric_value)
            .bind(&raw_output)
            .bind(&claude_response)
            .bind(duration)
            .execute(db)
            .await;

            let _ = sqlx::query(
                "UPDATE autoresearch_runs SET total_experiments = total_experiments + 1, updated_at = NOW() WHERE id = $1",
            )
            .bind(run_id)
            .execute(db)
            .await;
        }

        // Always reset back to clean main for the next experiment (isolated changes)
        log_and_persist(
            db,
            &tx,
            run_id,
            &format!("[EXP {experiment_number}] Resetting to {base_branch}..."),
        )
        .await;
        let _ = shell::agent_exec_capture(
            &agent_name,
            &format!("cd /home/agent/repo && git checkout {base_branch} && git reset --hard origin/{base_branch}"),
        ).await;
        if let Some(ref server) = benchmark_server {
            let _ = run_on_benchmark_server(
                server,
                &format!("cd ~/autoresearch/repo && git checkout {base_branch} && git reset --hard origin/{base_branch}"),
            ).await;
        }

        // Tell Claude we're starting fresh — and re-state the objective so it
        // doesn't drift after dozens of `--continue` turns.
        let reset_body = if is_better {
            "Your optimization was accepted and a branch has been created. Now we're back on the base branch. Try a completely different optimization."
        } else {
            "Your change did not improve the metric. We're back on the base branch. Try a different approach."
        };
        let reset_msg = format!(
            "REMINDER OF YOUR OBJECTIVE (do not lose sight of this — it overrides the obvious \
             local hot-path-optimization instinct):\n\
             {objective}\n\
             \n\
             {reset_body} If your objective involves research (reading external sources, surveying \
             ideas), make sure you've actually done that research and are picking ideas from it \
             rather than defaulting to micro-optimizing the same files repeatedly.\n\
             \n\
             Remember: record any significant choices with DECISION: ... | ALTERNATIVES: ... and \
             never ask questions — always make autonomous decisions."
        );
        let escaped = reset_msg.replace('\'', "'\\''");
        let _ = shell::agent_exec_claude_streaming(
            &agent_name,
            &format!("{auth_env} && cd /home/agent/repo && claude --dangerously-skip-permissions --output-format stream-json --verbose {model_flag} --continue -p '{escaped}'"),
            tx.clone(),
        ).await;
    }

    // No final push needed — each accepted experiment already created its own branch + PR
    update_run_status(db, run_id, "completed").await?;

    let bl = baseline_metric.unwrap_or(0.0);
    let bst = best_metric.unwrap_or(0.0);
    let improvement = if bl != 0.0 {
        ((bst - bl) / bl.abs()) * 100.0
    } else {
        0.0
    };
    log_and_persist(
        db,
        &tx,
        run_id,
        &format!("[DONE] Completed. Best: {bst} (baseline: {bl}, change: {improvement:.1}%)"),
    )
    .await;

    crate::audit::log_event(
        db,
        "autoresearch.completed",
        run.created_by.as_deref().unwrap_or("system"),
        Some(run_id),
        serde_json::json!({
            "total_experiments": experiment_number,
            "baseline_metric": bl,
            "best_metric": bst,
        }),
        None,
    )
    .await;

    cleanup(config, db, &agent_name, server_id_for_cleanup).await;
    persist_handle.abort();
    Ok(())
}

// ── Helpers ──

async fn run_benchmark(
    agent_name: &str,
    benchmark_command: &str,
    server: Option<&crate::models::BenchmarkServer>,
) -> Result<String, AppError> {
    if let Some(s) = server {
        run_on_benchmark_server(
            s,
            &format!(
                "source ~/.cargo/env 2>/dev/null; cd ~/autoresearch/repo && {benchmark_command}"
            ),
        )
        .await
    } else {
        shell::agent_exec_capture(
            agent_name,
            &format!("cd /home/agent/repo && {benchmark_command}"),
        )
        .await
    }
}

/// Run one scenario per tier on the benchmark server's `main` branch,
/// capture the resulting metrics, and store them as a JSONB blob on the run
/// row so later experiments can compare against them. Returns a
/// human-readable summary to feed Claude as the initial baseline context.
async fn run_expb_baseline(
    db: &PgPool,
    run_id: &str,
    base_branch: &str,
    server: &crate::models::BenchmarkServer,
    ethrex_repo_path: &str,
    benchmarks_repo_path: &str,
) -> Result<String, AppError> {
    // Make sure the benchmark server's ethrex checkout is on the run's base
    // branch at the tip of `origin/<base_branch>` before we start.
    let checkout_cmd = format!(
        "cd {ethrex_repo_path} && git fetch origin && git checkout {base_branch} \
         && git reset --hard origin/{base_branch}"
    );
    expb_ssh_setup(server, &checkout_cmd).await?;

    let mut summaries: Vec<String> = Vec::with_capacity(3);
    let mut baselines = serde_json::Map::new();
    let mut fast_metrics: Option<expb::ExpbMetrics> = None;

    for tier in expb::Tier::all() {
        let metrics =
            expb::run_scenario_over_ssh(server, ethrex_repo_path, benchmarks_repo_path, tier)
                .await?;
        summaries.push(format_expb_metrics(tier, &metrics));
        baselines.insert(tier.short_name().to_string(), metrics_to_json(&metrics));
        if tier == expb::Tier::Fast {
            fast_metrics = Some(metrics);
        }
    }

    // Store all three tiers' baselines so experiments can compare against
    // them; also populate `baseline_metric` with fast-tier mgas_avg for the
    // existing UI stat card.
    sqlx::query(
        "UPDATE autoresearch_runs SET expb_baseline_metrics = $1, baseline_metric = $2, \
         updated_at = NOW() WHERE id = $3",
    )
    .bind(serde_json::Value::Object(baselines))
    .bind(fast_metrics.as_ref().and_then(|m| m.mgas_avg))
    .bind(run_id)
    .execute(db)
    .await?;

    Ok(summaries.join("\n\n"))
}

/// Run one experiment through the tiered EXPB gate on the benchmark server.
/// Pushes the agent's HEAD to the ethrex checkout on the server, runs each
/// tier in turn, and compares against the per-tier baselines we stored at
/// run creation. Persists the resulting metrics onto the experiment row.
#[allow(clippy::too_many_arguments)]
async fn run_expb_experiment(
    db: &PgPool,
    agent_name: &str,
    github_token: &str,
    branch_created: &mut bool,
    exp_id: i64,
    run: &AutoresearchRun,
    server: &crate::models::BenchmarkServer,
    branch: &str,
) -> Result<(String, bool, Option<f64>), AppError> {
    let ethrex_repo_path = run
        .ethrex_repo_path
        .as_deref()
        .ok_or_else(|| AppError::BadRequest("Run is missing ethrex_repo_path".into()))?;
    let benchmarks_repo_path = run
        .benchmarks_repo_path
        .as_deref()
        .ok_or_else(|| AppError::BadRequest("Run is missing benchmarks_repo_path".into()))?;

    // 1. Push the agent container's HEAD to GitHub via the verified-commits
    //    flow. The agent container only ever talks to GitHub — it never needs
    //    a network path to the benchmark host.
    push_verified(
        agent_name,
        github_token,
        &run.repo,
        branch,
        &run.base_branch,
        branch_created,
        "autoresearch optimization",
    )
    .await?;

    // 2. Tell the benchmark server to fetch + check out the pushed branch
    //    from GitHub (the host's ethrex clone has GitHub as its origin).
    expb_ssh_setup(
        server,
        &format!(
            "cd {ethrex_repo_path} && git fetch origin {branch} && git checkout {branch} \
             && git reset --hard FETCH_HEAD"
        ),
    )
    .await?;

    // 3. Pull each tier's baseline metrics out of the JSONB we stored.
    let baseline_obj = run.expb_baseline_metrics.as_ref().ok_or_else(|| {
        AppError::Internal(
            "Baselines for this run are missing — the baseline phase did not complete.".into(),
        )
    })?;
    let mut baselines: Vec<(expb::Tier, expb::ExpbMetrics)> = Vec::with_capacity(3);
    for tier in expb::Tier::all() {
        let tier_json = baseline_obj.get(tier.short_name()).ok_or_else(|| {
            AppError::Internal(format!(
                "Baseline metrics for tier '{}' are missing.",
                tier.short_name()
            ))
        })?;
        baselines.push((tier, metrics_from_json(tier_json)));
    }

    // 4. Run the tiered gate.
    let result = expb::run_tiered(server, ethrex_repo_path, benchmarks_repo_path, &baselines).await;

    // 5. Persist metrics + tier reached on the experiment row.
    let tier_name = result.tier_reached.map(|t| t.short_name().to_string());
    let (mgas, latency, p50, p95, p99) = result
        .final_metrics
        .as_ref()
        .map(|m| {
            (
                m.mgas_avg,
                m.latency_avg_ms,
                m.latency_p50_ms,
                m.latency_p95_ms,
                m.latency_p99_ms,
            )
        })
        .unwrap_or((None, None, None, None, None));
    let _ = sqlx::query(
        "UPDATE autoresearch_experiments SET \
            mgas_avg = $1, latency_avg_ms = $2, \
            latency_p50_ms = $3, latency_p95_ms = $4, latency_p99_ms = $5, \
            expb_tier_reached = $6 \
         WHERE id = $7",
    )
    .bind(mgas)
    .bind(latency)
    .bind(p50)
    .bind(p95)
    .bind(p99)
    .bind(&tier_name)
    .bind(exp_id)
    .execute(db)
    .await;

    // 6. Build a textual summary to feed Claude in place of raw stdout.
    let mut summary = String::new();
    if let Some(m) = &result.final_metrics {
        summary.push_str(&format_expb_metrics(
            result.tier_reached.unwrap_or(expb::Tier::Fast),
            m,
        ));
    } else {
        summary.push_str("EXPB produced no metrics (all tiers failed or errored).");
    }
    summary.push_str(&format!(
        "\n\nTier reached: {}.\nPromotion decision: {}.",
        tier_name.as_deref().unwrap_or("none"),
        if result.keep { "KEEP" } else { "DISCARD" }
    ));

    Ok((summary, result.keep, mgas))
}

/// Run a single setup/utility SSH command on the benchmark server and return
/// its stdout. Fails the autoresearch run on non-zero exit — setup steps
/// before the actual benchmark must succeed.
async fn expb_ssh_setup(
    server: &crate::models::BenchmarkServer,
    command: &str,
) -> Result<String, AppError> {
    let mut args: Vec<String> = vec![
        "-o".into(),
        "StrictHostKeyChecking=no".into(),
        "-o".into(),
        "UserKnownHostsFile=/dev/null".into(),
        "-o".into(),
        "LogLevel=ERROR".into(),
    ];
    if let Some(key) = server.ssh_key_path.as_deref() {
        args.push("-i".into());
        args.push(key.to_string());
    }
    args.push(format!("{}@{}", server.ssh_user, server.hostname));
    args.push(command.to_string());

    let output = tokio::process::Command::new("ssh")
        .args(&args)
        .output()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to spawn ssh: {e}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::Internal(format!(
            "SSH setup command failed on {}: {}",
            server.hostname,
            stderr.trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn metrics_to_json(m: &expb::ExpbMetrics) -> serde_json::Value {
    serde_json::json!({
        "mgas_avg": m.mgas_avg,
        "latency_avg_ms": m.latency_avg_ms,
        "latency_p50_ms": m.latency_p50_ms,
        "latency_p95_ms": m.latency_p95_ms,
        "latency_p99_ms": m.latency_p99_ms,
    })
}

fn metrics_from_json(v: &serde_json::Value) -> expb::ExpbMetrics {
    let f = |k: &str| v.get(k).and_then(serde_json::Value::as_f64);
    expb::ExpbMetrics {
        mgas_avg: f("mgas_avg"),
        latency_avg_ms: f("latency_avg_ms"),
        latency_p50_ms: f("latency_p50_ms"),
        latency_p95_ms: f("latency_p95_ms"),
        latency_p99_ms: f("latency_p99_ms"),
    }
}

fn format_expb_metrics(tier: expb::Tier, m: &expb::ExpbMetrics) -> String {
    let fmt = |label: &str, v: Option<f64>| match v {
        Some(x) => format!("  {label}: {x:.4}"),
        None => format!("  {label}: —"),
    };
    format!(
        "[{scenario}]\n{mgas}\n{lat}\n{p50}\n{p95}\n{p99}",
        scenario = tier.short_name(),
        mgas = fmt("mgas_avg", m.mgas_avg),
        lat = fmt("latency_avg_ms", m.latency_avg_ms),
        p50 = fmt("latency_p50_ms", m.latency_p50_ms),
        p95 = fmt("latency_p95_ms", m.latency_p95_ms),
        p99 = fmt("latency_p99_ms", m.latency_p99_ms),
    )
}

async fn run_on_benchmark_server(
    server: &crate::models::BenchmarkServer,
    cmd: &str,
) -> Result<String, AppError> {
    let mut args = vec![
        "-o".to_string(),
        "StrictHostKeyChecking=no".to_string(),
        "-o".to_string(),
        "UserKnownHostsFile=/dev/null".to_string(),
        "-o".to_string(),
        "LogLevel=ERROR".to_string(),
    ];
    if let Some(ref key) = server.ssh_key_path {
        args.extend(["-i".to_string(), key.clone()]);
    }
    args.push(format!("{}@{}", server.ssh_user, server.hostname));
    args.push(cmd.to_string());

    let output = tokio::process::Command::new("ssh")
        .args(&args)
        .output()
        .await
        .map_err(|e| AppError::Internal(format!("SSH to benchmark server failed: {e}")))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() {
        return Err(AppError::Internal(format!(
            "Command on benchmark server failed: {stderr}\n{stdout}"
        )));
    }
    Ok(format!("{stdout}\n{stderr}"))
}

/// Clone the repo on the benchmark server. The token is passed as a one-time
/// env var via SSH from the tekton host — never stored on the benchmark server.
async fn clone_on_benchmark_server(
    server: &crate::models::BenchmarkServer,
    repo: &str,
    branch_name: &str,
    github_token: &str,
) -> Result<(), AppError> {
    let clone_url = format!("https://x-access-token:{github_token}@github.com/{repo}.git");
    // Clone into autoresearch/repo, removing any previous clone
    run_on_benchmark_server(
        server,
        &format!(
            "rm -rf ~/autoresearch/repo && \
             git clone --branch {branch_name} --single-branch {clone_url} ~/autoresearch/repo && \
             cd ~/autoresearch/repo && \
             git remote set-url origin https://github.com/{repo}.git"
        ),
    )
    .await?;
    // The last command strips the token from the stored remote URL
    Ok(())
}

/// Pull latest changes on the benchmark server. Uses a temporary remote URL
/// with the token, fetches, then restores the clean URL. Token is never persisted.
async fn pull_on_benchmark_server(
    server: &crate::models::BenchmarkServer,
    branch_name: &str,
    github_token: &str,
) -> Result<(), AppError> {
    let repo_url: String = run_on_benchmark_server(
        server,
        "cd ~/autoresearch/repo && git remote get-url origin",
    )
    .await?
    .trim()
    .to_string();

    let authed_url = repo_url.replace(
        "https://github.com/",
        &format!("https://x-access-token:{github_token}@github.com/"),
    );

    run_on_benchmark_server(
        server,
        &format!(
            "cd ~/autoresearch/repo && \
             git remote set-url origin '{authed_url}' && \
             git fetch origin {branch_name} && \
             git checkout -B {branch_name} FETCH_HEAD && \
             git reset --hard FETCH_HEAD && \
             git remote set-url origin '{repo_url}'"
        ),
    )
    .await?;
    Ok(())
}

/// Push changes via the GitHub GraphQL API (creates verified/signed commits).
/// Then sync the agent's local repo to match the remote.
async fn push_verified(
    agent_name: &str,
    github_token: &str,
    repo: &str,
    branch_name: &str,
    base_branch: &str,
    branch_created: &mut bool,
    message: &str,
) -> Result<(), AppError> {
    // On first push, create the branch on GitHub
    if !*branch_created {
        let base_sha = shell::agent_exec_capture(
            agent_name,
            &format!("cd /home/agent/repo && git rev-parse 'origin/{base_branch}'"),
        )
        .await?
        .trim()
        .to_string();

        crate::tasks::create_github_branch(github_token, repo, branch_name, &base_sha).await?;
        *branch_created = true;
    }

    // Get the current HEAD OID on the remote branch
    let head_oid = shell::agent_exec_capture(
        agent_name,
        &format!("cd /home/agent/repo && git ls-remote origin refs/heads/{branch_name} | cut -f1"),
    )
    .await?
    .trim()
    .to_string();

    let expected_oid = if head_oid.is_empty() {
        // Branch just created, use base branch HEAD
        shell::agent_exec_capture(
            agent_name,
            &format!("cd /home/agent/repo && git rev-parse 'origin/{base_branch}'"),
        )
        .await?
        .trim()
        .to_string()
    } else {
        head_oid
    };

    // Collect file changes relative to the remote state
    let base_ref = format!("origin/{base_branch}");
    let file_changes = crate::tasks::collect_file_changes(agent_name, &base_ref).await?;

    // Push via GitHub API (creates verified commit)
    crate::tasks::github_create_commit(
        github_token,
        repo,
        branch_name,
        &expected_oid,
        &file_changes,
        message,
    )
    .await?;

    // Sync agent's local repo to match the remote
    crate::tasks::sync_agent_to_remote(agent_name, branch_name).await?;

    Ok(())
}

/// Parse METRIC: and DESCRIPTION: lines from Claude's response.
fn parse_metric_response(response: &str) -> (Option<f64>, String) {
    let mut metric_value = None;
    let mut description = String::new();

    for line in response.lines() {
        let line = line.trim();
        if let Some(val) = line.strip_prefix("METRIC:") {
            let val = val.trim();
            let num_str: String = val
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
                .collect();
            metric_value = num_str.parse::<f64>().ok();
        } else if let Some(desc) = line.strip_prefix("DESCRIPTION:") {
            description = desc.trim().to_string();
        }
    }

    (metric_value, description)
}

/// Parse IMPROVED: line from Claude's response.
fn parse_improved_response(response: &str) -> bool {
    for line in response.lines() {
        let line = line.trim();
        if let Some(imp) = line.strip_prefix("IMPROVED:") {
            return imp.trim().eq_ignore_ascii_case("true");
        }
    }
    false
}

/// Recover interrupted runs on server restart.
pub async fn recover_interrupted_runs(db: &sqlx::PgPool) {
    let interrupted: Vec<(String,)> = sqlx::query_as(
        "SELECT id FROM autoresearch_runs WHERE status IN ('running', 'setting_up', 'pending')",
    )
    .fetch_all(db)
    .await
    .unwrap_or_default();

    for (id,) in &interrupted {
        tracing::warn!("Marking interrupted autoresearch run {id} as failed");
        let _ = sqlx::query(
            "UPDATE autoresearch_runs SET status = 'failed', \
             error_message = 'Interrupted by server restart', updated_at = NOW() WHERE id = $1",
        )
        .bind(id)
        .execute(db)
        .await;
    }

    let _ = sqlx::query(
        "UPDATE benchmark_servers SET status = 'ready', updated_at = NOW() WHERE status = 'busy'",
    )
    .execute(db)
    .await;

    if !interrupted.is_empty() {
        tracing::info!(
            "Recovered {} interrupted autoresearch run(s)",
            interrupted.len()
        );
    }
}
