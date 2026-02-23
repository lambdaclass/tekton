use axum::extract::{Multipart, Path, State};
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

const MAX_UPLOAD_SIZE: usize = 10 * 1024 * 1024; // 10MB

pub async fn upload_image(
    _user: AuthUser,
    State(state): State<crate::AppState>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, AppError> {
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(format!("Invalid multipart data: {e}")))?
    {
        let name = field.name().unwrap_or("").to_string();
        if name != "image" {
            continue;
        }

        let content_type = field
            .content_type()
            .unwrap_or("application/octet-stream")
            .to_string();

        let ext = match content_type.as_str() {
            "image/png" => "png",
            "image/jpeg" | "image/jpg" => "jpg",
            "image/gif" => "gif",
            "image/webp" => "webp",
            _ => {
                return Err(AppError::BadRequest(format!(
                    "Unsupported image type: {content_type}. Allowed: png, jpg, gif, webp"
                )));
            }
        };

        let data = field
            .bytes()
            .await
            .map_err(|e| AppError::BadRequest(format!("Failed to read upload: {e}")))?;

        if data.len() > MAX_UPLOAD_SIZE {
            return Err(AppError::BadRequest(format!(
                "Image too large ({} bytes). Max: {} bytes",
                data.len(),
                MAX_UPLOAD_SIZE
            )));
        }

        let filename = format!("{}.{}", Uuid::new_v4(), ext);
        let uploads_dir = format!("{}/uploads", state.config.static_dir);
        tokio::fs::create_dir_all(&uploads_dir)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to create uploads dir: {e}")))?;

        let file_path = format!("{uploads_dir}/{filename}");
        tokio::fs::write(&file_path, &data)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to save upload: {e}")))?;

        let url = format!("/uploads/{filename}");
        return Ok(Json(serde_json::json!({ "url": url })));
    }

    Err(AppError::BadRequest(
        "No 'image' field found in upload".into(),
    ))
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

    let image_url_json = req
        .image_urls
        .as_ref()
        .filter(|v| !v.is_empty())
        .map(|v| serde_json::to_string(v).unwrap());

    sqlx::query(
        "INSERT INTO tasks (id, prompt, repo, base_branch, status, parent_task_id, created_by, image_url) VALUES (?, ?, ?, ?, 'pending', ?, ?, ?)",
    )
    .bind(&id)
    .bind(&req.prompt)
    .bind(&req.repo)
    .bind(base_branch)
    .bind(&req.parent_task_id)
    .bind(created_by)
    .bind(&image_url_json)
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
    let image_url_json2 = image_url_json.clone();
    let channels = state.task_channels.clone();

    tokio::spawn(async move {
        let agent_name = format!("a-{short}");
        let result = run_task_pipeline(
            &config, &db, &task_id, &short, &prompt, &repo, &base, image_url_json2.as_deref(), tx.clone(),
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
    image_url_json: Option<&str>,
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

    // Step 3: Copy images and run Claude (streaming)
    let effective_prompt = augment_prompt_with_images(config, &agent_name, image_url_json, prompt, &tx).await?;

    update_task_status(db, task_id, "running_claude", None).await?;
    log_and_send(db, task_id, &tx, "[STEP] Running Claude...");
    run_claude_streaming(&agent_name, &effective_prompt, tx.clone()).await?;

    // Step 3b: Commit any changes — retry once if Claude made no edits
    let made_changes = commit_changes_in_agent(&agent_name, prompt, tx.clone()).await?;
    if !made_changes {
        log_and_send(db, task_id, &tx, "[STEP] Re-running Claude — asking it to make actual edits...");
        let retry_prompt = format!(
            "You were given this task but made no file changes. You MUST edit the code to accomplish the task. \
             Do not just analyze — use the Edit or Write tools to make the changes.\n\n{effective_prompt}"
        );
        run_claude_streaming(&agent_name, &retry_prompt, tx.clone()).await?;
        let made_changes_retry = commit_changes_in_agent(&agent_name, prompt, tx.clone()).await?;
        if !made_changes_retry {
            log_and_send(db, task_id, &tx, "[WARN] Claude still made no changes after retry.");
        }
    }

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

/// Parse image_url JSON and copy all images into an agent container.
/// Returns a list of remote paths that were successfully copied.
async fn copy_images_to_agent(
    config: &Config,
    agent_name: &str,
    image_url_json: &str,
    tx: &broadcast::Sender<String>,
) -> Result<Vec<String>, AppError> {
    let urls: Vec<String> = serde_json::from_str(image_url_json).unwrap_or_default();
    if urls.is_empty() {
        return Ok(vec![]);
    }

    let _ = tx.send(format!("[STEP] Copying {} image(s) to agent container...", urls.len()));

    // Create uploads dir in agent
    if let Err(e) = shell::agent_exec(
        agent_name,
        "mkdir -p /home/agent/uploads",
        tx.clone(),
    )
    .await
    {
        let _ = tx.send(format!("[WARN] Failed to create uploads dir in agent: {e}"));
        return Ok(vec![]);
    }

    let mut remote_paths = Vec::new();
    for url in &urls {
        let filename = url.rsplit('/').next().unwrap_or("image.png");
        let local_path = format!("{}{}", config.static_dir, url);
        let remote_path = format!("/home/agent/uploads/{filename}");

        match shell::scp_to_agent(agent_name, &local_path, &remote_path).await {
            Ok(_) => remote_paths.push(remote_path),
            Err(e) => {
                let _ = tx.send(format!("[WARN] Failed to copy image {filename}: {e}"));
            }
        }
    }

    Ok(remote_paths)
}

/// Augment a prompt with image references if any images are present.
async fn augment_prompt_with_images(
    config: &Config,
    agent_name: &str,
    image_url_json: Option<&str>,
    prompt: &str,
    tx: &broadcast::Sender<String>,
) -> Result<String, AppError> {
    let json = match image_url_json {
        Some(j) if !j.is_empty() => j,
        _ => return Ok(prompt.to_string()),
    };

    let remote_paths = copy_images_to_agent(config, agent_name, json, tx).await?;
    if remote_paths.is_empty() {
        return Ok(prompt.to_string());
    }

    let paths_list = remote_paths
        .iter()
        .map(|p| format!("- {p}"))
        .collect::<Vec<_>>()
        .join("\n");

    Ok(format!(
        "I've attached {} reference image(s). Read them first to see what I'm referring to:\n{paths_list}\n\n{prompt}",
        remote_paths.len()
    ))
}

/// Check if the agent repo has uncommitted changes.
async fn agent_has_changes(agent_name: &str) -> Result<bool, AppError> {
    let ip = shell::agent_ip_public(agent_name)?;
    let output = tokio::process::Command::new("ssh")
        .args([
            "-o", "StrictHostKeyChecking=no",
            "-o", "UserKnownHostsFile=/dev/null",
            &format!("agent@{ip}"),
            "cd /home/agent/repo && git add -A && git diff --cached --quiet; echo $?",
        ])
        .output()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to check changes: {e}")))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    // exit code 1 means there are differences (changes exist)
    Ok(stdout.ends_with('1'))
}

async fn commit_changes_in_agent(
    agent_name: &str,
    prompt: &str,
    tx: broadcast::Sender<String>,
) -> Result<bool, AppError> {
    let has_changes = agent_has_changes(agent_name).await?;
    if !has_changes {
        let _ = tx.send("[WARN] Claude made no file changes.".to_string());
        return Ok(false);
    }
    let escaped_prompt = prompt.replace('\'', "'\\''");
    let commit_cmd = format!(
        "cd /home/agent/repo && git add -A && git commit -m 'Claude: {escaped_prompt}'"
    );
    shell::agent_exec(agent_name, &commit_cmd, tx).await?;
    Ok(true)
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
    let mut last_seen_id: i64 = 0;

    // Fetch the original task prompt for context
    let original_prompt = sqlx::query_scalar::<_, String>("SELECT prompt FROM tasks WHERE id = ?")
        .bind(task_id)
        .fetch_one(db)
        .await?;

    // Track conversation history for context
    let mut conversation_history: Vec<String> = Vec::new();

    update_task_status(db, task_id, "awaiting_followup", None).await?;
    let _ = tx.send("[STATUS] Waiting for follow-up messages (click 'Mark Done' to finish)...".to_string());

    loop {
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;

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

            // Copy images if present, augment prompt
            let effective_content = augment_prompt_with_images(
                config, agent_name, msg.image_url.as_deref(), &msg.content, tx,
            ).await?;

            // Build context-aware prompt with original task + conversation history
            let context_prompt = build_followup_prompt(&original_prompt, &conversation_history, &effective_content);

            // Add this message to history for future follow-ups
            conversation_history.push(format!("{}: {}", msg.sender, msg.content));

            // Re-invoke Claude with the follow-up message
            let _ = tx.send(format!("[FOLLOWUP] Running Claude with follow-up from {}...", msg.sender));
            update_task_status(db, task_id, "running_claude", None).await?;

            run_claude_streaming(agent_name, &context_prompt, tx.clone()).await?;
            let _ = commit_changes_in_agent(agent_name, &msg.content, tx.clone()).await?;

            // Push and update preview
            push_and_preview(config, db, task_id, short_id, agent_name, repo, branch_name, preview_created, tx).await?;

            update_task_status(db, task_id, "awaiting_followup", None).await?;
            let _ = tx.send("[STATUS] Waiting for more follow-up messages...".to_string());
        }
    }
}

/// Build a follow-up prompt that includes the original task context and conversation history.
fn build_followup_prompt(original_prompt: &str, history: &[String], new_message: &str) -> String {
    let mut prompt = format!("ORIGINAL TASK:\n{original_prompt}\n\n");

    if !history.is_empty() {
        prompt.push_str("PREVIOUS FOLLOW-UP MESSAGES (already addressed):\n");
        for msg in history {
            prompt.push_str(&format!("- {msg}\n"));
        }
        prompt.push('\n');
    }

    prompt.push_str(&format!("NEW FOLLOW-UP (address this now):\n{new_message}"));
    prompt
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

    let has_images = req.image_urls.as_ref().is_some_and(|v| !v.is_empty());
    if req.content.trim().is_empty() && !has_images {
        return Err(AppError::BadRequest("Message content cannot be empty".into()));
    }

    let image_url_json = req
        .image_urls
        .as_ref()
        .filter(|v| !v.is_empty())
        .map(|v| serde_json::to_string(v).unwrap());

    let sender = &user.0.sub;

    sqlx::query(
        "INSERT INTO task_messages (task_id, sender, content, image_url) VALUES (?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(sender)
    .bind(&req.content)
    .bind(&image_url_json)
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

pub async fn reopen_task(
    _user: AuthUser,
    State(state): State<crate::AppState>,
    Path(id): Path<String>,
) -> Result<Json<Task>, AppError> {
    let task = sqlx::query_as::<_, Task>("SELECT * FROM tasks WHERE id = ?")
        .bind(&id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound("Task not found".into()))?;

    if task.status != "completed" && task.status != "failed" {
        return Err(AppError::BadRequest(format!(
            "Can only reopen completed or failed tasks, current status: {}",
            task.status
        )));
    }

    let branch_name = task.branch_name.as_deref().ok_or_else(|| {
        AppError::BadRequest("Task has no branch — cannot reopen".into())
    })?;

    let short_id = &id[..6];

    // Reset status and error
    update_task_status(&state.db, &id, "creating_agent", None).await?;

    // Create broadcast channel
    let (tx, _) = broadcast::channel(1024);
    state.task_channels.insert(id.clone(), tx.clone());

    // Spawn background pipeline
    let config = state.config.clone();
    let db = state.db.clone();
    let task_id = id.clone();
    let short = short_id.to_string();
    let repo = task.repo.clone();
    let branch = branch_name.to_string();
    let has_preview = task.preview_url.is_some();
    let channels = state.task_channels.clone();

    tokio::spawn(async move {
        let agent_name = format!("a-{short}");
        let result = run_reopen_pipeline(
            &config, &db, &task_id, &short, &repo, &branch, has_preview, tx.clone(),
        )
        .await;

        if let Err(e) = &result {
            let _ = update_task_status(&db, &task_id, "failed", Some(&e.to_string())).await;
            let _ = tx.send(format!("[ERROR] Reopen failed: {e}"));
            let _ = shell::destroy_agent(&config, &agent_name).await;
        }

        let channels2 = channels;
        let tid = task_id.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            channels2.remove(&tid);
        });
    });

    let task = sqlx::query_as::<_, Task>("SELECT * FROM tasks WHERE id = ?")
        .bind(&id)
        .fetch_one(&state.db)
        .await?;
    Ok(Json(task))
}

async fn run_reopen_pipeline(
    config: &Config,
    db: &SqlitePool,
    task_id: &str,
    short_id: &str,
    repo: &str,
    branch_name: &str,
    had_preview: bool,
    tx: broadcast::Sender<String>,
) -> Result<(), AppError> {
    let agent_name = format!("a-{short_id}");

    // Step 1: Create agent container
    log_and_send(db, task_id, &tx, &format!("[STEP] Creating agent container '{agent_name}' (reopen)..."));
    shell::create_agent(config, &agent_name).await?;
    update_task_field(db, task_id, "agent_name", &agent_name).await?;

    // Step 2: Clone repo and checkout existing branch
    update_task_status(db, task_id, "cloning", None).await?;
    log_and_send(db, task_id, &tx, &format!("[STEP] Cloning {repo} and checking out existing branch {branch_name}..."));

    let token = get_github_token().await?;
    let clone_url = format!("https://x-access-token:{token}@github.com/{repo}.git");

    let clone_cmd = format!(
        "cd /home/agent && git clone '{clone_url}' repo && \
         cd repo && git checkout '{branch_name}'"
    );
    shell::agent_exec(&agent_name, &clone_cmd, tx.clone()).await?;

    // Step 3: Go straight into follow-up loop
    let mut preview_created = had_preview;
    follow_up_loop(config, db, task_id, short_id, &agent_name, repo, branch_name, &mut preview_created, &tx).await?;

    // Step 4: Destroy agent
    log_and_send(db, task_id, &tx, "[STEP] Destroying agent container...");
    let _ = shell::destroy_agent(config, &agent_name).await;

    update_task_status(db, task_id, "completed", None).await?;
    log_and_send(db, task_id, &tx, "[DONE] Task completed.");

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
