use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use axum::extract::{Path, State};
use axum::Json;
use dashmap::DashMap;
use regex::Regex;
use tokio::sync::broadcast;

use crate::auth::{check_repo_permission, AuthUser, MemberUser};
use crate::error::AppError;
use crate::models::{AutoresearchExperiment, AutoresearchRun, CreateAutoresearchRunRequest};
use crate::shell;

pub type StopFlags = Arc<DashMap<String, Arc<AtomicBool>>>;

pub fn new_stop_flags() -> StopFlags {
    Arc::new(DashMap::new())
}

const RUN_QUERY: &str = "SELECT id, name, repo, base_branch, branch_name, agent_name, \
     benchmark_server_id, benchmark_command, objective, metric_regex, optimization_direction, \
     target_files, frozen_files, max_experiments, time_budget_minutes, status, \
     baseline_metric, best_metric, total_experiments, accepted_experiments, \
     total_cost_usd, error_message, created_by, \
     TO_CHAR(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as created_at, \
     TO_CHAR(updated_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as updated_at, \
     pr_url, pr_number \
     FROM autoresearch_runs";

const EXP_QUERY: &str = "SELECT id, run_id, experiment_number, status, diff, \
     metric_value, metric_raw_output, accepted, hypothesis, claude_response, \
     input_tokens, output_tokens, cost_usd, duration_seconds, \
     TO_CHAR(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as created_at \
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
         (id, repo, base_branch, branch_name, benchmark_command, objective, metric_regex, \
          optimization_direction, target_files, frozen_files, max_experiments, \
          time_budget_minutes, benchmark_server_id, created_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)",
    )
    .bind(&id)
    .bind(&req.repo)
    .bind(base_branch)
    .bind(&branch_name)
    .bind(&req.benchmark_command)
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
        run.benchmark_command,
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

        log_and_persist(
            db,
            &tx,
            run_id,
            &format!(
                "[SETUP] Syncing repo to benchmark server {}...",
                server.name
            ),
        )
        .await;
        // Create the destination directory on the benchmark server
        ensure_remote_dir(&server, "autoresearch/repo").await?;
        sync_to_benchmark_server(&agent_name, &server).await?;

        Some(server)
    } else {
        None
    };

    let server_id_for_cleanup = benchmark_server.as_ref().map(|s| s.id);

    // Phase 2: Baseline benchmark
    log_and_persist(db, &tx, run_id, "[BASELINE] Running baseline benchmark...").await;
    let baseline_output = run_benchmark(
        &agent_name,
        &run.benchmark_command,
        benchmark_server.as_ref(),
    )
    .await?;

    // Ask Claude to analyze the baseline output and extract the metric
    let baseline_analysis = analyze_benchmark_output(
        &agent_name,
        &baseline_output,
        &run,
        None, // no previous metric for baseline
    )
    .await?;

    let baseline_metric = baseline_analysis.metric_value;

    sqlx::query(
        "UPDATE autoresearch_runs SET baseline_metric = $2, best_metric = $2, updated_at = NOW() WHERE id = $1",
    )
    .bind(run_id)
    .bind(baseline_metric)
    .execute(db)
    .await?;

    log_and_persist(
        db,
        &tx,
        run_id,
        &format!(
            "[BASELINE] Baseline metric: {baseline_metric} ({})",
            baseline_analysis.metric_description
        ),
    )
    .await;

    // Phase 3: Experiment loop
    update_run_status(db, run_id, "running").await?;
    let start_time = Instant::now();
    let mut experiment_number = 0i32;
    let mut best_metric = baseline_metric;
    let metric_description = baseline_analysis.metric_description;

    loop {
        // Check stop conditions
        if stop_flag.load(Ordering::Relaxed) {
            log_and_persist(db, &tx, run_id, "[STOP] Run stopped by user.").await;
            update_run_status(db, run_id, "stopped").await?;
            let _ = push_branch(&agent_name, branch_name).await;
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

        log_and_persist(
            db,
            &tx,
            run_id,
            &format!("[EXP {experiment_number}] Starting experiment..."),
        )
        .await;

        // Insert experiment row
        let exp_id: i64 = sqlx::query_scalar(
            "INSERT INTO autoresearch_experiments (run_id, experiment_number) VALUES ($1, $2) RETURNING id",
        )
        .bind(run_id)
        .bind(experiment_number)
        .fetch_one(db)
        .await?;

        // Build prompt
        let recent_experiments = get_recent_experiments(db, run_id, 5).await?;
        let prompt = build_experiment_prompt(
            &run,
            best_metric,
            baseline_metric,
            &metric_description,
            experiment_number,
            &recent_experiments,
        );

        // Run Claude
        log_and_persist(
            db,
            &tx,
            run_id,
            &format!("[EXP {experiment_number}] Asking Claude for optimization..."),
        )
        .await;

        // Escape single quotes in the prompt for shell
        let escaped_prompt = prompt.replace('\'', "'\\''");
        let claude_result = shell::agent_exec_capture(
            &agent_name,
            &format!(
                "cd /home/agent/repo && claude --dangerously-skip-permissions --output-format text -p '{escaped_prompt}'"
            ),
        )
        .await;

        let claude_response = match claude_result {
            Ok(output) => output,
            Err(e) => {
                log_and_persist(
                    db,
                    &tx,
                    run_id,
                    &format!("[EXP {experiment_number}] Claude error: {e}"),
                )
                .await;
                let _ = sqlx::query(
                    "UPDATE autoresearch_experiments SET status = 'error', claude_response = $2 WHERE id = $1",
                )
                .bind(exp_id)
                .bind(format!("{e}"))
                .execute(db)
                .await;
                let _ = sqlx::query(
                    "UPDATE autoresearch_runs SET total_experiments = total_experiments + 1, updated_at = NOW() WHERE id = $1",
                )
                .bind(run_id)
                .execute(db)
                .await;
                continue;
            }
        };

        // Check if Claude made any changes
        let diff = shell::agent_exec_capture(&agent_name, "cd /home/agent/repo && git diff")
            .await
            .unwrap_or_default();

        if diff.trim().is_empty() {
            log_and_persist(
                db,
                &tx,
                run_id,
                &format!("[EXP {experiment_number}] No changes made, skipping."),
            )
            .await;
            let _ = sqlx::query(
                "UPDATE autoresearch_experiments SET status = 'rejected', accepted = false, \
                 claude_response = $2, diff = '', metric_raw_output = 'No changes' WHERE id = $1",
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
            continue;
        }

        // Commit changes locally
        shell::agent_exec_capture(
            &agent_name,
            "cd /home/agent/repo && git add -A && git commit -m 'autoresearch experiment'",
        )
        .await?;

        // Sync to benchmark server if needed
        if let Some(ref server) = benchmark_server {
            sync_to_benchmark_server(&agent_name, server).await?;
        }

        // Run benchmark
        log_and_persist(
            db,
            &tx,
            run_id,
            &format!("[EXP {experiment_number}] Running benchmark..."),
        )
        .await;
        let _ = sqlx::query(
            "UPDATE autoresearch_experiments SET status = 'benchmarking' WHERE id = $1",
        )
        .bind(exp_id)
        .execute(db)
        .await;

        let benchmark_output = run_benchmark(
            &agent_name,
            &run.benchmark_command,
            benchmark_server.as_ref(),
        )
        .await;

        let (metric_value, raw_output, is_better) = match benchmark_output {
            Ok(output) => {
                match analyze_benchmark_output(&agent_name, &output, &run, Some(best_metric)).await
                {
                    Ok(analysis) => (Some(analysis.metric_value), output, analysis.is_improvement),
                    Err(e) => {
                        log_and_persist(
                            db,
                            &tx,
                            run_id,
                            &format!("[EXP {experiment_number}] Metric analysis failed: {e}"),
                        )
                        .await;
                        (None, output, false)
                    }
                }
            }
            Err(e) => {
                log_and_persist(
                    db,
                    &tx,
                    run_id,
                    &format!("[EXP {experiment_number}] Benchmark failed: {e}"),
                )
                .await;
                (None, format!("{e}"), false)
            }
        };

        let duration = exp_start.elapsed().as_secs() as i32;

        if is_better {
            let val = metric_value.unwrap();
            log_and_persist(
                db,
                &tx,
                run_id,
                &format!("[EXP {experiment_number}] ACCEPTED: {val} (improved from {best_metric})"),
            )
            .await;
            best_metric = val;

            let _ = push_branch(&agent_name, branch_name).await;

            let _ = sqlx::query(
                "UPDATE autoresearch_experiments SET status = 'accepted', accepted = true, \
                 diff = $2, metric_value = $3, metric_raw_output = $4, \
                 claude_response = $5, duration_seconds = $6 WHERE id = $1",
            )
            .bind(exp_id)
            .bind(&diff)
            .bind(val)
            .bind(&raw_output)
            .bind(&claude_response)
            .bind(duration)
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
            let val_str = metric_value
                .map(|v| format!("{v}"))
                .unwrap_or_else(|| "N/A".into());
            log_and_persist(
                db,
                &tx,
                run_id,
                &format!("[EXP {experiment_number}] REJECTED: {val_str} (best: {best_metric})"),
            )
            .await;

            // Revert
            let _ = shell::agent_exec_capture(
                &agent_name,
                "cd /home/agent/repo && git reset --hard HEAD~1",
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
    }

    // Push final state
    let _ = push_branch(&agent_name, branch_name).await;
    update_run_status(db, run_id, "completed").await?;

    let improvement = if baseline_metric != 0.0 {
        ((best_metric - baseline_metric) / baseline_metric.abs()) * 100.0
    } else {
        0.0
    };
    log_and_persist(
        db,
        &tx,
        run_id,
        &format!(
            "[DONE] Completed. Best: {best_metric} (baseline: {baseline_metric}, change: {improvement:.1}%)"
        ),
    )
    .await;

    crate::audit::log_event(
        db,
        "autoresearch.completed",
        run.created_by.as_deref().unwrap_or("system"),
        Some(run_id),
        serde_json::json!({
            "total_experiments": experiment_number,
            "baseline_metric": baseline_metric,
            "best_metric": best_metric,
        }),
        None,
    )
    .await;

    cleanup(config, db, &agent_name, server_id_for_cleanup).await;
    Ok(())
}

// ── Helpers ──

async fn run_benchmark(
    agent_name: &str,
    benchmark_command: &str,
    server: Option<&crate::models::BenchmarkServer>,
) -> Result<String, AppError> {
    if let Some(s) = server {
        let mut ssh_args = vec![
            "-o".to_string(),
            "StrictHostKeyChecking=no".to_string(),
            "-o".to_string(),
            "ConnectTimeout=30".to_string(),
        ];
        if let Some(ref key) = s.ssh_key_path {
            ssh_args.extend_from_slice(&["-i".to_string(), key.clone()]);
        }
        ssh_args.push(format!("{}@{}", s.ssh_user, s.hostname));
        ssh_args.push(format!("cd autoresearch/repo && {benchmark_command}"));

        let output = tokio::process::Command::new("ssh")
            .args(&ssh_args)
            .output()
            .await
            .map_err(|e| AppError::Internal(format!("SSH to benchmark server failed: {e}")))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        Ok(format!("{stdout}\n{stderr}"))
    } else {
        shell::agent_exec_capture(
            agent_name,
            &format!("cd /home/agent/repo && {benchmark_command}"),
        )
        .await
    }
}

/// Create a directory on the benchmark server via SSH from the host.
async fn ensure_remote_dir(
    server: &crate::models::BenchmarkServer,
    dir: &str,
) -> Result<(), AppError> {
    let mut args = vec![
        "-o".to_string(),
        "StrictHostKeyChecking=no".to_string(),
        "-o".to_string(),
        "UserKnownHostsFile=/dev/null".to_string(),
    ];
    if let Some(ref key) = server.ssh_key_path {
        args.extend(["-i".to_string(), key.clone()]);
    }
    args.push(format!("{}@{}", server.ssh_user, server.hostname));
    args.push(format!("mkdir -p {dir}"));

    let output = tokio::process::Command::new("ssh")
        .args(&args)
        .output()
        .await
        .map_err(|e| AppError::Internal(format!("SSH mkdir failed: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::Internal(format!(
            "mkdir on benchmark server failed: {stderr}"
        )));
    }
    Ok(())
}

/// Sync repo from agent container to benchmark server.
/// Runs on the host: pulls from agent via SSH, pushes to benchmark server via SSH.
/// The host holds both keys — no private keys are copied into the agent container.
async fn sync_to_benchmark_server(
    agent_name: &str,
    server: &crate::models::BenchmarkServer,
) -> Result<(), AppError> {
    let agent_ip = shell::agent_ip_public(agent_name)?;
    let agent_src = format!("agent@{agent_ip}:/home/agent/repo/");
    let dest = format!("{}@{}:autoresearch/repo/", server.ssh_user, server.hostname);

    // Build SSH options for the benchmark server side
    let mut server_ssh = vec![
        "-o".to_string(),
        "StrictHostKeyChecking=no".to_string(),
        "-o".to_string(),
        "UserKnownHostsFile=/dev/null".to_string(),
    ];
    if let Some(ref key) = server.ssh_key_path {
        server_ssh.extend(["-i".to_string(), key.clone()]);
    }

    // Step 1: rsync from agent container to a temp dir on the host
    let tmp_dir = format!("/tmp/autoresearch-sync-{agent_name}");
    let pull_output = tokio::process::Command::new("rsync")
        .args([
            "-az",
            "--delete",
            "--exclude",
            ".git",
            "-e",
            "ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null",
            &agent_src,
            &format!("{tmp_dir}/"),
        ])
        .output()
        .await
        .map_err(|e| AppError::Internal(format!("rsync from agent failed: {e}")))?;

    if !pull_output.status.success() {
        let stderr = String::from_utf8_lossy(&pull_output.stderr);
        return Err(AppError::Internal(format!(
            "rsync from agent failed: {stderr}"
        )));
    }

    // Step 2: rsync from host temp dir to benchmark server
    let server_ssh_str = server_ssh.join(" ");
    let push_output = tokio::process::Command::new("rsync")
        .args([
            "-az",
            "--delete",
            "-e",
            &format!("ssh {server_ssh_str}"),
            &format!("{tmp_dir}/"),
            &dest,
        ])
        .output()
        .await
        .map_err(|e| AppError::Internal(format!("rsync to benchmark server failed: {e}")))?;

    if !push_output.status.success() {
        let stderr = String::from_utf8_lossy(&push_output.stderr);
        // Clean up temp dir
        let _ = tokio::fs::remove_dir_all(&tmp_dir).await;
        return Err(AppError::Internal(format!(
            "rsync to benchmark server failed: {stderr}"
        )));
    }

    // Clean up temp dir
    let _ = tokio::fs::remove_dir_all(&tmp_dir).await;
    Ok(())
}

async fn push_branch(agent_name: &str, branch_name: &str) -> Result<(), AppError> {
    shell::agent_exec_capture(
        agent_name,
        &format!("cd /home/agent/repo && git push -u origin {branch_name} --force"),
    )
    .await?;
    Ok(())
}

struct BenchmarkAnalysis {
    metric_value: f64,
    metric_description: String,
    is_improvement: bool,
}

/// Ask Claude to analyze benchmark output and extract the metric.
async fn analyze_benchmark_output(
    agent_name: &str,
    benchmark_output: &str,
    run: &AutoresearchRun,
    previous_best: Option<f64>,
) -> Result<BenchmarkAnalysis, AppError> {
    let objective = run.objective.as_deref().unwrap_or("improve performance");
    let prev_context = match previous_best {
        Some(val) => format!("The previous best metric value was {val}."),
        None => {
            "This is the baseline run — there is no previous value to compare against.".to_string()
        }
    };

    let truncated = &benchmark_output[..benchmark_output.len().min(4000)];
    let prompt = format!(
        "You are analyzing benchmark output for an optimization task.\n\
         \n\
         OBJECTIVE: {objective}\n\
         Benchmark command: {}\n\
         {prev_context}\n\
         \n\
         BENCHMARK OUTPUT:\n\
         {truncated}\n\
         \n\
         Respond with EXACTLY three lines, nothing else:\n\
         METRIC: <a single number representing the key metric>\n\
         DESCRIPTION: <brief description of what the metric is, e.g. \"execution time in ms\" or \"throughput in ops/sec\">\n\
         IMPROVED: <true or false — did the metric improve compared to the previous best? If this is the baseline, say true>",
        run.benchmark_command,
    );

    let escaped_prompt = prompt.replace('\'', "'\\''");
    let output = shell::agent_exec_capture(
        agent_name,
        &format!(
            "claude --dangerously-skip-permissions --output-format text -p '{escaped_prompt}'"
        ),
    )
    .await?;

    let mut metric_value = None;
    let mut metric_description = String::new();
    let mut is_improvement = false;

    for line in output.lines() {
        let line = line.trim();
        if let Some(val) = line.strip_prefix("METRIC:") {
            let val = val.trim();
            let num_str: String = val
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
                .collect();
            metric_value = num_str.parse::<f64>().ok();
        } else if let Some(desc) = line.strip_prefix("DESCRIPTION:") {
            metric_description = desc.trim().to_string();
        } else if let Some(imp) = line.strip_prefix("IMPROVED:") {
            is_improvement = imp.trim().eq_ignore_ascii_case("true");
        }
    }

    let metric_value = metric_value.ok_or_else(|| {
        AppError::Internal(format!(
            "Claude did not return a valid METRIC value. Response:\n{}",
            &output[..output.len().min(500)]
        ))
    })?;

    Ok(BenchmarkAnalysis {
        metric_value,
        metric_description,
        is_improvement,
    })
}

fn build_experiment_prompt(
    run: &AutoresearchRun,
    best_metric: f64,
    baseline_metric: f64,
    metric_description: &str,
    experiment_number: i32,
    recent_experiments: &[AutoresearchExperiment],
) -> String {
    let objective = run.objective.as_deref().unwrap_or("improve performance");
    let target_desc = run.target_files.as_deref().unwrap_or("any files");
    let frozen_desc = run
        .frozen_files
        .as_deref()
        .map(|f| format!("\n\nFROZEN FILES (do NOT modify): {f}"))
        .unwrap_or_default();

    let mut history = String::new();
    if !recent_experiments.is_empty() {
        history.push_str("\n\nRecent experiment history:\n");
        for exp in recent_experiments {
            let status = if exp.accepted == Some(true) {
                "ACCEPTED"
            } else {
                "REJECTED"
            };
            let metric = exp
                .metric_value
                .map(|v| format!("{v}"))
                .unwrap_or_else(|| "N/A".into());
            let summary = exp.hypothesis.as_deref().unwrap_or("(no summary)");
            history.push_str(&format!(
                "  Exp #{}: {} — metric: {} — {}\n",
                exp.experiment_number, status, metric, summary
            ));
        }
    }

    format!(
        "You are an optimization agent running experiment #{experiment_number}.\n\
         \n\
         OBJECTIVE: {objective}\n\
         Benchmark command: {}\n\
         Metric being tracked: {metric_description}\n\
         Baseline metric: {baseline_metric}\n\
         Current best metric: {best_metric}\n\
         \n\
         TARGET FILES to modify: {target_desc}{frozen_desc}\n\
         {history}\n\
         INSTRUCTIONS:\n\
         - Make a single, focused change that you believe will improve the metric.\n\
         - Briefly explain your hypothesis before making the change.\n\
         - Do NOT modify the benchmark command or evaluation code.\n\
         - Focus on algorithmic improvements, performance optimizations, or parameter tuning.\n\
         - Be creative but targeted — small, testable changes are better than large rewrites.",
        run.benchmark_command
    )
}

async fn get_recent_experiments(
    db: &sqlx::PgPool,
    run_id: &str,
    limit: i32,
) -> Result<Vec<AutoresearchExperiment>, AppError> {
    let exps = sqlx::query_as::<_, AutoresearchExperiment>(&format!(
        "{EXP_QUERY} WHERE run_id = $1 ORDER BY experiment_number DESC LIMIT $2"
    ))
    .bind(run_id)
    .bind(limit)
    .fetch_all(db)
    .await?;
    Ok(exps)
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
