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
use crate::models::{
    ClassifyRequest, ClassifyResponse, CreateTaskRequest, SendMessageRequest, Task, TaskLog,
    TaskMessage,
};
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

pub async fn get_subtasks(
    _user: AuthUser,
    State(state): State<crate::AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<Task>>, AppError> {
    // Verify parent exists
    let _ = sqlx::query_as::<_, Task>("SELECT * FROM tasks WHERE id = ?")
        .bind(&id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound("Task not found".into()))?;

    let subtasks =
        sqlx::query_as::<_, Task>("SELECT * FROM tasks WHERE parent_task_id = ? ORDER BY created_at ASC")
            .bind(&id)
            .fetch_all(&state.db)
            .await?;
    Ok(Json(subtasks))
}

pub async fn create_task(
    user: AuthUser,
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

    // Validate parent_task_id if provided
    if let Some(ref parent_id) = req.parent_task_id {
        let parent_exists = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM tasks WHERE id = ?")
            .bind(parent_id)
            .fetch_one(&state.db)
            .await?;
        if parent_exists == 0 {
            return Err(AppError::BadRequest(format!(
                "Parent task '{}' does not exist",
                parent_id
            )));
        }
    }

    let id = Uuid::new_v4().to_string();
    let short_id = &id[..6];
    let base_branch = req.base_branch.as_deref().unwrap_or("main");
    let created_by = &user.0.sub;

    sqlx::query(
        "INSERT INTO tasks (id, prompt, repo, base_branch, status, parent_task_id, created_by) VALUES (?, ?, ?, ?, 'pending', ?, ?)",
    )
    .bind(&id)
    .bind(&req.prompt)
    .bind(&req.repo)
    .bind(base_branch)
    .bind(&req.parent_task_id)
    .bind(created_by)
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
        let agent_name = format!("a-{short}");
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
    let agent_name = format!("a-{short_id}");
    let branch_name = format!("claude/{short_id}");

    // Step 1: Create agent container
    update_task_status(db, task_id, "creating_agent", None).await?;
    log_and_send(db, task_id, &tx, &format!("[STEP] Creating agent container '{agent_name}'..."));
    shell::create_agent(config, &agent_name).await?;
    update_task_field(db, task_id, "agent_name", &agent_name).await?;

    // Step 2: Clone repo and create branch
    update_task_status(db, task_id, "cloning", None).await?;
    log_and_send(db, task_id, &tx, &format!("[STEP] Cloning {repo} and creating branch {branch_name}..."));

    let token = get_github_token().await?;
    let clone_url = format!("https://x-access-token:{token}@github.com/{repo}.git");

    let clone_cmd = format!(
        "cd /home/agent && git clone --branch {base_branch} --single-branch '{clone_url}' repo && \
         cd repo && git checkout -b '{branch_name}'"
    );
    shell::agent_exec(&agent_name, &clone_cmd, tx.clone()).await?;
    update_task_field(db, task_id, "branch_name", &branch_name).await?;

    // Step 3: Run Claude (streaming)
    update_task_status(db, task_id, "running_claude", None).await?;
    log_and_send(db, task_id, &tx, "[STEP] Running Claude...");
    run_claude_streaming(&agent_name, prompt, tx.clone()).await?;

    // Step 3b: Commit any changes
    commit_changes_in_agent(&agent_name, prompt, tx.clone()).await?;

    // Step 3c: Push and create preview
    let mut preview_created = false;
    push_and_preview(config, db, task_id, short_id, &agent_name, repo, &branch_name, &mut preview_created, &tx).await?;

    // Step 4: Follow-up loop
    follow_up_loop(config, db, task_id, short_id, &agent_name, repo, &branch_name, &mut preview_created, &tx).await?;

    // Step 5: Destroy agent
    log_and_send(db, task_id, &tx, "[STEP] Destroying agent container...");
    let _ = shell::destroy_agent(config, &agent_name).await;

    // Done
    let preview_url = format!("https://t-{short_id}.{}", config.preview_domain);
    update_task_status(db, task_id, "completed", None).await?;
    log_and_send(db, task_id, &tx, &format!("[DONE] Preview available at {preview_url}"));

    Ok(())
}

async fn read_claude_oauth_token() -> Result<String, AppError> {
    tokio::fs::read_to_string("/var/secrets/claude/oauth_token")
        .await
        .map(|s| s.trim().to_string())
        .map_err(|e| AppError::Internal(format!(
            "Failed to read Claude OAuth token from /var/secrets/claude/oauth_token: {e}"
        )))
}

async fn run_claude_streaming(
    agent_name: &str,
    prompt: &str,
    tx: broadcast::Sender<String>,
) -> Result<(), AppError> {
    let oauth_token = read_claude_oauth_token().await?;
    let escaped_prompt = prompt.replace('\'', "'\\''");
    let claude_cmd = format!(
        "export CLAUDE_CODE_OAUTH_TOKEN='{oauth_token}' && cd /home/agent/repo && claude --dangerously-skip-permissions --output-format stream-json --verbose -p '{escaped_prompt}'"
    );
    shell::agent_exec_claude_streaming(agent_name, &claude_cmd, tx).await
}

async fn commit_changes_in_agent(
    agent_name: &str,
    prompt: &str,
    tx: broadcast::Sender<String>,
) -> Result<(), AppError> {
    let escaped_prompt = prompt.replace('\'', "'\\''");
    let commit_cmd = format!(
        "cd /home/agent/repo && \
         git add -A && \
         if git diff --cached --quiet; then \
           echo 'No changes to commit'; \
         else \
           git commit -m 'Claude: {escaped_prompt}'; \
         fi"
    );
    shell::agent_exec(agent_name, &commit_cmd, tx).await
}

#[derive(PartialEq)]
enum FollowUpOutcome {
    Done,
}

async fn follow_up_loop(
    config: &Config,
    db: &SqlitePool,
    task_id: &str,
    short_id: &str,
    agent_name: &str,
    repo: &str,
    branch_name: &str,
    preview_created: &mut bool,
    tx: &broadcast::Sender<String>,
) -> Result<FollowUpOutcome, AppError> {
    let timeout_dur = std::time::Duration::from_secs(5 * 60);
    let mut deadline = tokio::time::Instant::now() + timeout_dur;
    let mut last_seen_id: i64 = 0;

    update_task_status(db, task_id, "awaiting_followup", None).await?;
    let _ = tx.send("[STATUS] Waiting for follow-up messages (send '__done__' or wait 5 min to finish)...".to_string());

    loop {
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;

        if tokio::time::Instant::now() >= deadline {
            let _ = tx.send("[STATUS] Follow-up timeout elapsed, finishing up.".to_string());
            break;
        }

        // Check for new messages since last seen (by ID, not timestamp, to avoid
        // missing messages sent during long-running steps like preview create)
        let new_messages: Vec<TaskMessage> = sqlx::query_as::<_, TaskMessage>(
            "SELECT * FROM task_messages WHERE task_id = ? AND id > ? ORDER BY id ASC",
        )
        .bind(task_id)
        .bind(last_seen_id)
        .fetch_all(db)
        .await?;

        if new_messages.is_empty() {
            continue;
        }

        // Update last seen to latest message id
        last_seen_id = new_messages.last().unwrap().id;

        for msg in &new_messages {
            if msg.content.trim() == "__done__" {
                let _ = tx.send("[STATUS] Received '__done__', finishing up.".to_string());
                return Ok(FollowUpOutcome::Done);
            }

            // Re-invoke Claude with the follow-up message
            let _ = tx.send(format!("[FOLLOWUP] Running Claude with follow-up from {}...", msg.sender));
            update_task_status(db, task_id, "running_claude", None).await?;

            run_claude_streaming(agent_name, &msg.content, tx.clone()).await?;
            commit_changes_in_agent(agent_name, &msg.content, tx.clone()).await?;

            // Push and update preview
            push_and_preview(config, db, task_id, short_id, agent_name, repo, branch_name, preview_created, tx).await?;

            update_task_status(db, task_id, "awaiting_followup", None).await?;
            let _ = tx.send("[STATUS] Waiting for more follow-up messages...".to_string());

            // Reset the timeout
            deadline = tokio::time::Instant::now() + timeout_dur;
        }
    }

    Ok(FollowUpOutcome::Done)
}

async fn push_and_preview(
    config: &Config,
    db: &SqlitePool,
    task_id: &str,
    short_id: &str,
    agent_name: &str,
    repo: &str,
    branch_name: &str,
    preview_created: &mut bool,
    tx: &broadcast::Sender<String>,
) -> Result<(), AppError> {
    // Push
    update_task_status(db, task_id, "pushing", None).await?;
    log_and_send(db, task_id, tx, "[STEP] Pushing branch to GitHub...");

    let token = get_github_token().await?;
    let push_cmd = format!(
        "cd /home/agent/repo && \
         git remote set-url origin 'https://x-access-token:{token}@github.com/{repo}.git' && \
         git push -u origin '{branch_name}'"
    );
    shell::agent_exec(agent_name, &push_cmd, tx.clone()).await?;

    // Preview
    let preview_slug = format!("t-{short_id}");

    if !*preview_created {
        update_task_status(db, task_id, "creating_preview", None).await?;
        log_and_send(db, task_id, tx, &format!("[STEP] Creating preview '{preview_slug}'..."));

        let preview_type = if config.vertex_repos.contains(&repo.to_string()) { "vertex" } else { "node" };
        shell::create_preview(config, repo, branch_name, Some(&preview_slug), preview_type).await?;

        let preview_url = format!("https://{preview_slug}.{}", config.preview_domain);
        update_task_field(db, task_id, "preview_slug", &preview_slug).await?;
        update_task_field(db, task_id, "preview_url", &preview_url).await?;
        *preview_created = true;
    } else {
        log_and_send(db, task_id, tx, &format!("[STEP] Updating preview '{preview_slug}'..."));
        let _ = shell::update_preview(config, &preview_slug).await;
    }

    // Screenshot (fire-and-forget so it doesn't block the follow-up loop)
    let preview_url = format!("https://{preview_slug}.{}", config.preview_domain);
    let config2 = config.clone();
    let db2 = db.clone();
    let task_id2 = task_id.to_string();
    let preview_slug2 = preview_slug.clone();
    let preview_url2 = preview_url.clone();
    let tx2 = tx.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(15)).await;
        take_screenshot(&config2, &db2, &task_id2, &preview_slug2, &preview_url2, &tx2).await;
    });

    Ok(())
}

async fn take_screenshot(
    config: &Config,
    db: &SqlitePool,
    task_id: &str,
    preview_slug: &str,
    preview_url: &str,
    tx: &broadcast::Sender<String>,
) {
    let screenshots_dir = format!("{}/screenshots", config.static_dir);
    if let Err(e) = std::fs::create_dir_all(&screenshots_dir) {
        let _ = tx.send(format!("[WARN] Could not create screenshots dir: {e}"));
        return;
    }

    let screenshot_path = format!("{screenshots_dir}/{preview_slug}.png");
    let screenshot_url = format!("/screenshots/{preview_slug}.png");

    let _ = tx.send(format!("[STEP] Taking screenshot of {preview_url}..."));

    let output = tokio::process::Command::new(&config.chromium_bin)
        .args([
            "--headless",
            "--disable-gpu",
            &format!("--screenshot={screenshot_path}"),
            "--window-size=1280,720",
            preview_url,
        ])
        .output()
        .await;

    match output {
        Ok(o) if o.status.success() => {
            let _ = sqlx::query(
                "UPDATE tasks SET screenshot_url = ?, updated_at = datetime('now') WHERE id = ?",
            )
            .bind(&screenshot_url)
            .bind(task_id)
            .execute(db)
            .await;
            let _ = tx.send(format!("[STEP] Screenshot saved: {screenshot_url}"));
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            let _ = tx.send(format!("[WARN] Screenshot failed: {stderr}"));
        }
        Err(e) => {
            let _ = tx.send(format!("[WARN] Could not run chromium: {e}"));
        }
    }
}

// ── Classify ──

pub async fn classify(
    _user: AuthUser,
    State(state): State<crate::AppState>,
    Json(req): Json<ClassifyRequest>,
) -> Result<Json<ClassifyResponse>, AppError> {
    if state.config.allowed_repos.is_empty() {
        return Err(AppError::BadRequest("No allowed repos configured".into()));
    }

    let repo_list = state
        .config
        .allowed_repos
        .iter()
        .enumerate()
        .map(|(i, r)| format!("{}. {}", i + 1, r))
        .collect::<Vec<_>>()
        .join("\n");

    let system_prompt = format!(
        "You are a repository classifier. Given a task description, pick the single best matching \
         repository from this list and respond with ONLY the repo name (owner/repo format), nothing else.\n\n\
         Available repositories:\n{repo_list}"
    );

    let full_prompt = format!("{system_prompt}\n\nTask: {}", req.prompt);
    let escaped = full_prompt.replace('\'', "'\\''");

    // Run claude CLI on the host using the long-lived OAuth token
    let oauth_token = read_claude_oauth_token().await?;
    let output = tokio::process::Command::new(&state.config.claude_bin)
        .env("CLAUDE_CODE_OAUTH_TOKEN", &oauth_token)
        .args(["--dangerously-skip-permissions", "-p", &escaped])
        .output()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to run claude for classification: {e}")))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();

    // Find which allowed repo the response matches
    let repo = state
        .config
        .allowed_repos
        .iter()
        .find(|r| stdout.contains(r.as_str()))
        .cloned()
        .unwrap_or_else(|| {
            // Fallback: return the raw output truncated, or first repo
            if stdout.is_empty() {
                state.config.allowed_repos[0].clone()
            } else {
                stdout.lines().next().unwrap_or("").to_string()
            }
        });

    Ok(Json(ClassifyResponse { repo }))
}

// ── Messages ──

pub async fn list_messages(
    _user: AuthUser,
    State(state): State<crate::AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<TaskMessage>>, AppError> {
    // Verify task exists
    let _ = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM tasks WHERE id = ?")
        .bind(&id)
        .fetch_one(&state.db)
        .await?;

    let messages = sqlx::query_as::<_, TaskMessage>(
        "SELECT * FROM task_messages WHERE task_id = ? ORDER BY created_at ASC",
    )
    .bind(&id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(messages))
}

pub async fn send_message(
    user: AuthUser,
    State(state): State<crate::AppState>,
    Path(id): Path<String>,
    Json(req): Json<SendMessageRequest>,
) -> Result<Json<TaskMessage>, AppError> {
    // Verify task exists
    let task_exists = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM tasks WHERE id = ?")
        .bind(&id)
        .fetch_one(&state.db)
        .await?;
    if task_exists == 0 {
        return Err(AppError::NotFound("Task not found".into()));
    }

    if req.content.trim().is_empty() {
        return Err(AppError::BadRequest("Message content cannot be empty".into()));
    }

    let sender = &user.0.sub;

    sqlx::query(
        "INSERT INTO task_messages (task_id, sender, content) VALUES (?, ?, ?)",
    )
    .bind(&id)
    .bind(sender)
    .bind(&req.content)
    .execute(&state.db)
    .await?;

    let message = sqlx::query_as::<_, TaskMessage>(
        "SELECT * FROM task_messages WHERE task_id = ? ORDER BY id DESC LIMIT 1",
    )
    .bind(&id)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(message))
}

// ── Helpers ──

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
