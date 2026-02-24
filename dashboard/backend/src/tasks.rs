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

#[derive(Clone)]
struct GitIdentity {
    token: String,
    name: String,
    email: String,
}

async fn get_git_identity(db: &SqlitePool, github_login: &str) -> Result<GitIdentity, AppError> {
    let row = sqlx::query_as::<_, (String, String, Option<String>)>(
        "SELECT github_token, name, email FROM users WHERE github_login = ?"
    )
    .bind(github_login)
    .fetch_optional(db)
    .await?
    .ok_or_else(|| AppError::Auth(format!("User '{github_login}' not found in database")))?;

    let email = row.2.unwrap_or_else(|| format!("{github_login}@users.noreply.github.com"));

    Ok(GitIdentity {
        token: row.0,
        name: row.1,
        email,
    })
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct FileAddition {
    path: String,
    contents: String, // base64-encoded
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct FileDeletion {
    path: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct FileChanges {
    additions: Vec<FileAddition>,
    deletions: Vec<FileDeletion>,
}

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

    // Get the user's git identity (GitHub token, name, email)
    let git_id = get_git_identity(&state.db, created_by).await?;

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
            &config, &db, &task_id, &short, &prompt, &repo, &base, image_url_json2.as_deref(), &git_id, tx.clone(),
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
    git_id: &GitIdentity,
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

    let clone_url = format!("https://x-access-token:{}@github.com/{repo}.git", git_id.token);

    let escaped_name = git_id.name.replace('\'', "'\\''");
    let escaped_email = git_id.email.replace('\'', "'\\''");
    let clone_cmd = format!(
        "cd /home/agent && git clone --branch {base_branch} --single-branch '{clone_url}' repo && \
         cd repo && git checkout -b '{branch_name}' && \
         git config user.name '{escaped_name}' && git config user.email '{escaped_email}'"
    );
    shell::agent_exec(&agent_name, &clone_cmd, tx.clone()).await?;
    update_task_field(db, task_id, "branch_name", &branch_name).await?;

    // Step 3: Copy images and run Claude (streaming)
    let effective_prompt = augment_prompt_with_images(config, &agent_name, image_url_json, prompt, &tx).await?;

    update_task_status(db, task_id, "running_claude", None).await?;
    log_and_send(db, task_id, &tx, "[STEP] Running Claude...");
    let claude_text = run_claude_streaming(&agent_name, &effective_prompt, tx.clone()).await?;
    save_claude_response_as_message(db, task_id, &claude_text).await?;

    // Step 3b: Commit any changes (if Claude asked a question and made none, that's fine —
    // the user will see the question in chat and respond via the follow-up loop)
    let _ = commit_changes_in_agent(&agent_name, prompt, tx.clone()).await?;

    // Step 3c: Push and create preview
    let mut preview_created = false;
    let mut branch_pushed = false;
    push_and_preview(config, db, task_id, short_id, &agent_name, repo, &branch_name, base_branch, &mut branch_pushed, &mut preview_created, git_id, &tx).await?;

    // Step 4: Follow-up loop
    follow_up_loop(config, db, task_id, short_id, &agent_name, repo, &branch_name, base_branch, &mut branch_pushed, &mut preview_created, git_id, &tx).await?;

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
) -> Result<String, AppError> {
    let oauth_token = read_claude_oauth_token().await?;
    let escaped_prompt = prompt.replace('\'', "'\\''");
    let claude_cmd = format!(
        "export CLAUDE_CODE_OAUTH_TOKEN='{oauth_token}' && cd /home/agent/repo && claude --dangerously-skip-permissions --output-format stream-json --verbose -p '{escaped_prompt}'"
    );
    shell::agent_exec_claude_streaming(agent_name, &claude_cmd, tx).await
}

/// Run Claude with --continue to maintain conversation context for follow-ups.
async fn run_claude_continue(
    agent_name: &str,
    prompt: &str,
    tx: broadcast::Sender<String>,
) -> Result<String, AppError> {
    let oauth_token = read_claude_oauth_token().await?;
    let escaped_prompt = prompt.replace('\'', "'\\''");
    let claude_cmd = format!(
        "export CLAUDE_CODE_OAUTH_TOKEN='{oauth_token}' && cd /home/agent/repo && \
         claude --dangerously-skip-permissions --output-format stream-json --verbose \
         --continue -p '{escaped_prompt}'"
    );
    shell::agent_exec_claude_streaming(agent_name, &claude_cmd, tx).await
}

/// Save Claude's text response as a chat message so it appears in the UI.
async fn save_claude_response_as_message(
    db: &SqlitePool,
    task_id: &str,
    claude_text: &str,
) -> Result<(), AppError> {
    let text = claude_text.trim();
    if text.is_empty() {
        return Ok(());
    }
    // Truncate to a reasonable length for chat display
    let display_text: String = text.chars().take(2000).collect();
    sqlx::query(
        "INSERT INTO task_messages (task_id, sender, content) VALUES (?, 'claude', ?)",
    )
    .bind(task_id)
    .bind(&display_text)
    .execute(db)
    .await?;
    Ok(())
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
    base_branch: &str,
    branch_pushed: &mut bool,
    preview_created: &mut bool,
    git_id: &GitIdentity,
    tx: &broadcast::Sender<String>,
) -> Result<FollowUpOutcome, AppError> {
    let mut last_seen_id: i64 = 0;

    // Track conversation history for context (used as fallback if --continue fails)
    let mut conversation_history: Vec<String> = Vec::new();

    update_task_status(db, task_id, "awaiting_followup", None).await?;
    let _ = tx.send("[STATUS] Waiting for follow-up messages (click 'Mark Done' to finish)...".to_string());

    loop {
        // Check for new messages IMMEDIATELY (no sleep before first check)
        // Filter out claude's own messages so we only process user messages
        let new_messages: Vec<TaskMessage> = sqlx::query_as::<_, TaskMessage>(
            "SELECT * FROM task_messages WHERE task_id = ? AND id > ? AND sender != 'claude' ORDER BY id ASC",
        )
        .bind(task_id)
        .bind(last_seen_id)
        .fetch_all(db)
        .await?;

        if new_messages.is_empty() {
            // Sleep 3s before checking again (reduced from 10s)
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            continue;
        }

        // Update last seen to latest message id
        last_seen_id = new_messages.last().unwrap().id;

        // Check for __done__ in any message
        if new_messages.iter().any(|m| m.content.trim() == "__done__") {
            let _ = tx.send("[STATUS] Received '__done__', finishing up.".to_string());
            return Ok(FollowUpOutcome::Done);
        }

        // Batch all pending messages into one prompt
        let mut combined_parts: Vec<String> = Vec::new();
        for msg in &new_messages {
            let effective = augment_prompt_with_images(
                config, agent_name, msg.image_url.as_deref(), &msg.content, tx,
            ).await?;
            combined_parts.push(effective);
            conversation_history.push(format!("{}: {}", msg.sender, msg.content));
        }
        let combined_prompt = combined_parts.join("\n\n---\n\n");

        // Run Claude with --continue to maintain conversation context
        update_task_status(db, task_id, "running_claude", None).await?;
        let _ = tx.send("[FOLLOWUP] Running Claude with follow-up...".to_string());

        let claude_text = match run_claude_continue(agent_name, &combined_prompt, tx.clone()).await {
            Ok(text) => text,
            Err(e) => {
                // Fallback: if --continue fails, use fresh session with full context
                let _ = tx.send(format!("[WARN] --continue failed ({e}), falling back to fresh session..."));
                let original_prompt = sqlx::query_scalar::<_, String>("SELECT prompt FROM tasks WHERE id = ?")
                    .bind(task_id)
                    .fetch_one(db)
                    .await?;
                let context_prompt = build_followup_prompt(&original_prompt, &conversation_history, &combined_prompt);
                run_claude_streaming(agent_name, &context_prompt, tx.clone()).await?
            }
        };
        save_claude_response_as_message(db, task_id, &claude_text).await?;

        let _ = commit_changes_in_agent(agent_name, &combined_parts[0], tx.clone()).await?;

        // Push and update preview
        push_and_preview(config, db, task_id, short_id, agent_name, repo, branch_name, base_branch, branch_pushed, preview_created, git_id, tx).await?;

        update_task_status(db, task_id, "awaiting_followup", None).await?;
        let _ = tx.send("[STATUS] Waiting for more follow-up messages...".to_string());
        // Loop immediately — check for more messages right away (no sleep before next check)
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

/// Collect all file changes in the agent repo relative to a base ref.
/// Runs a Python script inside the container that stages everything,
/// diffs against base_ref, and outputs JSON with additions (base64) and deletions.
async fn collect_file_changes(agent_name: &str, base_ref: &str) -> Result<FileChanges, AppError> {
    // Sanitize base_ref (should be like "origin/main" — never contains quotes)
    let safe_base_ref = base_ref.replace('\'', "").replace('"', "");
    let python_script = r#"
import subprocess, json, base64, os
base_ref = "__BASE_REF__"
os.chdir("/home/agent/repo")
subprocess.run(["git", "add", "-A"], check=True)
if subprocess.run(["git", "diff", "--cached", "--quiet"]).returncode != 0:
    subprocess.run(["git", "commit", "-m", "temp"], check=True)
result = subprocess.run(
    ["git", "diff", "--name-status", base_ref + "..HEAD"],
    capture_output=True, text=True, check=True)
changes = {"additions": [], "deletions": []}
for line in result.stdout.strip().split("\n"):
    if not line: continue
    parts = line.split("\t")
    status = parts[0][0]
    if status == "D":
        changes["deletions"].append({"path": parts[1]})
    elif status == "R":
        changes["deletions"].append({"path": parts[1]})
        with open(parts[2], "rb") as f:
            changes["additions"].append({"path": parts[2], "contents": base64.b64encode(f.read()).decode()})
    else:
        with open(parts[-1], "rb") as f:
            changes["additions"].append({"path": parts[-1], "contents": base64.b64encode(f.read()).decode()})
print(json.dumps(changes))
"#
    .replace("__BASE_REF__", &safe_base_ref);

    let cmd = ["python3 -c '", python_script.trim(), "'"].concat();
    let output = shell::agent_exec_capture(agent_name, &cmd).await?;

    // The output may contain SSH warnings before the JSON; find the JSON line
    let json_line = output
        .lines()
        .rev()
        .find(|l| l.starts_with('{'))
        .unwrap_or(output.trim());

    let changes: FileChanges = serde_json::from_str(json_line).map_err(|e| {
        AppError::Internal(format!(
            "Failed to parse file changes JSON: {e}\nRaw output: {output}"
        ))
    })?;
    Ok(changes)
}

/// Create a branch on GitHub via REST API.
async fn create_github_branch(
    token: &str,
    repo: &str,
    branch_name: &str,
    sha: &str,
) -> Result<(), AppError> {
    let client = reqwest::Client::new();
    let url = format!("https://api.github.com/repos/{repo}/git/refs");
    let body = serde_json::json!({
        "ref": format!("refs/heads/{branch_name}"),
        "sha": sha,
    });

    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", "dashboard")
        .header("Accept", "application/vnd.github+json")
        .json(&body)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("GitHub create branch request failed: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(AppError::Internal(format!(
            "GitHub create branch failed ({status}): {body}"
        )));
    }
    Ok(())
}

/// Create a verified commit on a GitHub branch via the GraphQL API.
/// Returns the new commit OID.
async fn github_create_commit(
    token: &str,
    repo: &str,
    branch_name: &str,
    expected_head_oid: &str,
    file_changes: &FileChanges,
    message: &str,
) -> Result<String, AppError> {
    let client = reqwest::Client::new();

    let additions: Vec<serde_json::Value> = file_changes
        .additions
        .iter()
        .map(|a| {
            serde_json::json!({
                "path": a.path,
                "contents": a.contents,
            })
        })
        .collect();

    let deletions: Vec<serde_json::Value> = file_changes
        .deletions
        .iter()
        .map(|d| {
            serde_json::json!({
                "path": d.path,
            })
        })
        .collect();

    let query = "mutation($input: CreateCommitOnBranchInput!) { \
        createCommitOnBranch(input: $input) { \
            commit { oid url } \
        } \
    }";

    let variables = serde_json::json!({
        "input": {
            "branch": {
                "repositoryNameWithOwner": repo,
                "branchName": branch_name,
            },
            "message": { "headline": message },
            "fileChanges": {
                "additions": additions,
                "deletions": deletions,
            },
            "expectedHeadOid": expected_head_oid,
        }
    });

    let body = serde_json::json!({
        "query": query,
        "variables": variables,
    });

    let resp = client
        .post("https://api.github.com/graphql")
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", "dashboard")
        .json(&body)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("GitHub GraphQL request failed: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(AppError::Internal(format!(
            "GitHub GraphQL failed ({status}): {text}"
        )));
    }

    let resp_json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to parse GraphQL response: {e}")))?;

    if let Some(errors) = resp_json.get("errors") {
        return Err(AppError::Internal(format!("GraphQL errors: {errors}")));
    }

    let oid = resp_json
        .pointer("/data/createCommitOnBranch/commit/oid")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::Internal("Missing commit OID in GraphQL response".into()))?
        .to_string();

    Ok(oid)
}

/// Sync the agent's local repo to match the remote branch after an API push.
/// Uses explicit refspec because the repo is cloned with --single-branch.
async fn sync_agent_to_remote(agent_name: &str, branch_name: &str) -> Result<(), AppError> {
    let escaped = branch_name.replace('\'', "'\\''");
    let cmd = format!(
        "cd /home/agent/repo && git fetch origin '{escaped}:refs/remotes/origin/{escaped}' && git reset --hard 'origin/{escaped}'"
    );
    shell::agent_exec_capture(agent_name, &cmd).await?;
    Ok(())
}

async fn push_and_preview(
    config: &Config,
    db: &SqlitePool,
    task_id: &str,
    short_id: &str,
    agent_name: &str,
    repo: &str,
    branch_name: &str,
    base_branch: &str,
    branch_pushed: &mut bool,
    preview_created: &mut bool,
    git_id: &GitIdentity,
    tx: &broadcast::Sender<String>,
) -> Result<(), AppError> {
    // Determine base_ref for diff
    let base_ref = if *branch_pushed {
        format!("origin/{branch_name}")
    } else {
        format!("origin/{base_branch}")
    };

    // Collect file changes via Python script in the agent container
    update_task_status(db, task_id, "pushing", None).await?;
    log_and_send(db, task_id, tx, "[STEP] Collecting file changes...");
    let file_changes = collect_file_changes(agent_name, &base_ref).await?;

    if file_changes.additions.is_empty() && file_changes.deletions.is_empty() {
        log_and_send(db, task_id, tx, "[WARN] No file changes to push.");
        return Ok(());
    }

    log_and_send(
        db, task_id, tx,
        &format!(
            "[STEP] Pushing {} addition(s) and {} deletion(s) via GitHub API...",
            file_changes.additions.len(),
            file_changes.deletions.len()
        ),
    );

    // On first push: get base SHA and create the branch on GitHub
    let expected_oid = if !*branch_pushed {
        let base_sha = shell::agent_exec_capture(
            agent_name,
            &format!("cd /home/agent/repo && git rev-parse '{base_ref}'"),
        )
        .await?
        .trim()
        .to_string();

        create_github_branch(&git_id.token, repo, branch_name, &base_sha).await?;
        log_and_send(db, task_id, tx, &format!("[STEP] Created branch {branch_name} on GitHub."));
        base_sha
    } else {
        shell::agent_exec_capture(
            agent_name,
            &format!("cd /home/agent/repo && git rev-parse 'origin/{branch_name}'"),
        )
        .await?
        .trim()
        .to_string()
    };

    // Build commit message from the task prompt
    let commit_msg = sqlx::query_scalar::<_, String>("SELECT prompt FROM tasks WHERE id = ?")
        .bind(task_id)
        .fetch_one(db)
        .await
        .map(|p| {
            let truncated: String = p.chars().take(72).collect();
            format!("Claude: {truncated}")
        })
        .unwrap_or_else(|_| format!("Claude: {}", &task_id[..6]));

    // Create verified commit via GitHub GraphQL API
    let oid = github_create_commit(
        &git_id.token, repo, branch_name, &expected_oid, &file_changes, &commit_msg,
    )
    .await?;
    log_and_send(db, task_id, tx, &format!("[STEP] Created verified commit {oid}"));

    // Sync local repo to match the API-created commit
    sync_agent_to_remote(agent_name, branch_name).await?;
    *branch_pushed = true;

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
    user: AuthUser,
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

    // Get the reopening user's git identity
    let git_id = get_git_identity(&state.db, &user.0.sub).await?;

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
    let base = task.base_branch.clone();
    let has_preview = task.preview_url.is_some();
    let channels = state.task_channels.clone();

    tokio::spawn(async move {
        let agent_name = format!("a-{short}");
        let result = run_reopen_pipeline(
            &config, &db, &task_id, &short, &repo, &branch, &base, has_preview, &git_id, tx.clone(),
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
    base_branch: &str,
    had_preview: bool,
    git_id: &GitIdentity,
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

    let clone_url = format!("https://x-access-token:{}@github.com/{repo}.git", git_id.token);

    let escaped_name = git_id.name.replace('\'', "'\\''");
    let escaped_email = git_id.email.replace('\'', "'\\''");
    let clone_cmd = format!(
        "cd /home/agent && git clone '{clone_url}' repo && \
         cd repo && git checkout '{branch_name}' && \
         git config user.name '{escaped_name}' && git config user.email '{escaped_email}'"
    );
    shell::agent_exec(&agent_name, &clone_cmd, tx.clone()).await?;

    // Step 3: Go straight into follow-up loop
    // branch_pushed starts as true since the branch already exists on GitHub
    let mut branch_pushed = true;
    let mut preview_created = had_preview;
    follow_up_loop(config, db, task_id, short_id, &agent_name, repo, branch_name, base_branch, &mut branch_pushed, &mut preview_created, git_id, &tx).await?;

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
