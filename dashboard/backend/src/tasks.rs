use axum::extract::{Path, State};
use axum::Json;
use dashmap::DashMap;
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::config::Config;
use crate::error::AppError;
use crate::models::{CreateTaskRequest, Task, TaskLog};
use crate::shell;

pub type TaskChannels = Arc<DashMap<String, broadcast::Sender<String>>>;

pub fn new_task_channels() -> TaskChannels {
    Arc::new(DashMap::new())
}

pub async fn list_tasks(
    _user: AuthUser,
    State(state): State<crate::AppState>,
) -> Result<Json<Vec<Task>>, AppError> {
    let tasks = sqlx::query_as::<_, Task>("SELECT * FROM tasks ORDER BY created_at DESC")
        .fetch_all(&state.db)
        .await?;
    Ok(Json(tasks))
}

pub async fn get_task(
    _user: AuthUser,
    State(state): State<crate::AppState>,
    Path(id): Path<String>,
) -> Result<Json<Task>, AppError> {
    let task = sqlx::query_as::<_, Task>("SELECT * FROM tasks WHERE id = ?")
        .bind(&id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound("Task not found".into()))?;
    Ok(Json(task))
}

pub async fn create_task(
    _user: AuthUser,
    State(state): State<crate::AppState>,
    Json(req): Json<CreateTaskRequest>,
) -> Result<Json<Task>, AppError> {
    // Validate repo is allowed
    if !state.config.allowed_repos.is_empty()
        && !state.config.allowed_repos.contains(&req.repo)
    {
        return Err(AppError::BadRequest(format!(
            "Repo '{}' is not in the allowed list",
            req.repo
        )));
    }

    let id = Uuid::new_v4().to_string();
    let short_id = &id[..6];
    let base_branch = req.base_branch.as_deref().unwrap_or("main");

    sqlx::query(
        "INSERT INTO tasks (id, prompt, repo, base_branch, status) VALUES (?, ?, ?, ?, 'pending')",
    )
    .bind(&id)
    .bind(&req.prompt)
    .bind(&req.repo)
    .bind(base_branch)
    .execute(&state.db)
    .await?;

    let task = sqlx::query_as::<_, Task>("SELECT * FROM tasks WHERE id = ?")
        .bind(&id)
        .fetch_one(&state.db)
        .await?;

    // Create broadcast channel for this task
    let (tx, _) = broadcast::channel(1024);
    state.task_channels.insert(id.clone(), tx.clone());

    // Spawn background task
    let config = state.config.clone();
    let db = state.db.clone();
    let task_id = id.clone();
    let short = short_id.to_string();
    let prompt = req.prompt.clone();
    let repo = req.repo.clone();
    let base = base_branch.to_string();
    let channels = state.task_channels.clone();

    tokio::spawn(async move {
        let agent_name = format!("t-{short}");
        let result = run_task_pipeline(
            &config, &db, &task_id, &short, &prompt, &repo, &base, tx.clone(),
        )
        .await;

        if let Err(e) = &result {
            let _ = update_task_status(&db, &task_id, "failed", Some(&e.to_string())).await;
            let _ = tx.send(format!("[ERROR] Task failed: {e}"));
            // Clean up agent container on failure
            let _ = shell::destroy_agent(&config, &agent_name).await;
        }

        // Clean up channel after a delay so late WS subscribers can still read
        let channels2 = channels;
        let tid = task_id.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            channels2.remove(&tid);
        });
    });

    Ok(Json(task))
}

async fn run_task_pipeline(
    config: &Config,
    db: &SqlitePool,
    task_id: &str,
    short_id: &str,
    prompt: &str,
    repo: &str,
    base_branch: &str,
    tx: broadcast::Sender<String>,
) -> Result<(), AppError> {
    let agent_name = format!("t-{short_id}");
    let branch_name = format!("claude/{short_id}");

    // Step 1: Create agent container
    update_task_status(db, task_id, "creating_agent", None).await?;
    log_and_send(db, task_id, &tx, &format!("[STEP] Creating agent container '{agent_name}'..."));
    shell::create_agent(config, &agent_name).await?;
    update_task_field(db, task_id, "agent_name", &agent_name).await?;

    // Step 2: Clone repo and create branch
    update_task_status(db, task_id, "cloning", None).await?;
    log_and_send(db, task_id, &tx, &format!("[STEP] Cloning {repo} and creating branch {branch_name}..."));

    // Get a fresh GitHub token from the webhook's internal endpoint
    let token = get_github_token().await?;
    let clone_url = format!("https://x-access-token:{token}@github.com/{repo}.git");

    let clone_cmd = format!(
        "cd /home/agent && git clone --branch {base_branch} --single-branch '{clone_url}' repo && \
         cd repo && git checkout -b '{branch_name}'"
    );
    shell::agent_exec(&agent_name, &clone_cmd, tx.clone()).await?;
    update_task_field(db, task_id, "branch_name", &branch_name).await?;

    // Step 3: Run Claude
    update_task_status(db, task_id, "running_claude", None).await?;
    log_and_send(db, task_id, &tx, "[STEP] Running Claude...");

    let escaped_prompt = prompt.replace('\'', "'\\''");
    let claude_cmd = format!(
        "cd /home/agent/repo && claude --dangerously-skip-permissions -p '{escaped_prompt}'"
    );
    shell::agent_exec(&agent_name, &claude_cmd, tx.clone()).await?;

    // Step 3b: Commit any changes Claude made
    log_and_send(db, task_id, &tx, "[STEP] Checking for changes...");
    let commit_cmd = format!(
        "cd /home/agent/repo && \
         git add -A && \
         if git diff --cached --quiet; then \
           echo 'No changes to commit'; \
         else \
           git commit -m 'Claude: {escaped_prompt}'; \
         fi"
    );
    shell::agent_exec(&agent_name, &commit_cmd, tx.clone()).await?;

    // Step 4: Push branch
    update_task_status(db, task_id, "pushing", None).await?;
    log_and_send(db, task_id, &tx, "[STEP] Pushing branch to GitHub...");

    // Re-fetch token in case it expired
    let token = get_github_token().await?;
    let push_cmd = format!(
        "cd /home/agent/repo && \
         git remote set-url origin 'https://x-access-token:{token}@github.com/{repo}.git' && \
         git push -u origin '{branch_name}'"
    );
    shell::agent_exec(&agent_name, &push_cmd, tx.clone()).await?;

    // Step 5: Destroy agent, then create preview (same name slot)
    log_and_send(db, task_id, &tx, "[STEP] Destroying agent container...");
    let _ = shell::destroy_agent(config, &agent_name).await;

    update_task_status(db, task_id, "creating_preview", None).await?;
    let preview_slug = format!("t-{short_id}");
    log_and_send(db, task_id, &tx, &format!("[STEP] Creating preview '{preview_slug}'..."));

    let preview_type = if config.vertex_repos.contains(&repo.to_string()) { "vertex" } else { "node" };
    shell::create_preview(config, repo, &branch_name, Some(&preview_slug), preview_type).await?;

    let preview_url = format!("https://{preview_slug}.{}", config.preview_domain);
    update_task_field(db, task_id, "preview_slug", &preview_slug).await?;
    update_task_field(db, task_id, "preview_url", &preview_url).await?;

    // Done
    update_task_status(db, task_id, "completed", None).await?;
    log_and_send(db, task_id, &tx, &format!("[DONE] Preview available at {preview_url}"));

    Ok(())
}

async fn get_github_token() -> Result<String, AppError> {
    let resp = reqwest::Client::new()
        .get("http://127.0.0.1:3100/internal/token")
        .send()
        .await?
        .error_for_status()
        .map_err(|e| AppError::Internal(format!("Failed to get GitHub token: {e}")))?;

    let body: serde_json::Value = resp.json().await?;
    body["token"]
        .as_str()
        .map(String::from)
        .ok_or_else(|| AppError::Internal("Token response missing 'token' field".into()))
}

fn log_and_send(
    db: &SqlitePool,
    task_id: &str,
    tx: &broadcast::Sender<String>,
    msg: &str,
) {
    let _ = tx.send(msg.to_string());
    // Fire-and-forget DB persist
    let db = db.clone();
    let task_id = task_id.to_string();
    let msg = msg.to_string();
    tokio::spawn(async move {
        let _ = sqlx::query("INSERT INTO task_logs (task_id, line) VALUES (?, ?)")
            .bind(&task_id)
            .bind(&msg)
            .execute(&db)
            .await;
    });
}

async fn update_task_status(
    db: &SqlitePool,
    task_id: &str,
    status: &str,
    error: Option<&str>,
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE tasks SET status = ?, error_message = ?, updated_at = datetime('now') WHERE id = ?",
    )
    .bind(status)
    .bind(error)
    .bind(task_id)
    .execute(db)
    .await?;
    Ok(())
}

async fn update_task_field(
    db: &SqlitePool,
    task_id: &str,
    field: &str,
    value: &str,
) -> Result<(), AppError> {
    // Safe because field is always a hardcoded string from our code
    let query = format!(
        "UPDATE tasks SET {field} = ?, updated_at = datetime('now') WHERE id = ?"
    );
    sqlx::query(&query)
        .bind(value)
        .bind(task_id)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn get_task_logs(
    _user: AuthUser,
    State(state): State<crate::AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<TaskLog>>, AppError> {
    let logs = sqlx::query_as::<_, TaskLog>(
        "SELECT * FROM task_logs WHERE task_id = ? ORDER BY id ASC",
    )
    .bind(&id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(logs))
}
