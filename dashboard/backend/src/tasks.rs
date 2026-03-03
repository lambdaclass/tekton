use axum::extract::{Multipart, Path, Query, State};
use axum::Json;
use dashmap::DashMap;
use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::auth::{self, AuthUser, MemberUser};
use crate::config::Config;
use crate::error::AppError;
use crate::models::{
    CreateTaskRequest, ListMessagesQuery, ListTasksQuery, PaginatedTasks, SendMessageRequest,
    Task, TaskAction, TaskLog, TaskMessage, UpdateTaskNameRequest,
};
use crate::policies;
use crate::secrets;
use crate::settings;
use crate::shell;

/// Delegate to auth module's check_repo_permission.
async fn check_repo_permission(
    db: &PgPool,
    github_login: &str,
    repo: &str,
    role: &str,
    github_org: &str,
) -> Result<(), AppError> {
    auth::check_repo_permission(db, github_login, repo, role, github_org).await
}

/// Delegate to auth module's check_task_ownership.
async fn check_task_ownership(
    db: &PgPool,
    task_id: &str,
    github_login: &str,
    role: &str,
) -> Result<(), AppError> {
    auth::check_task_ownership(db, task_id, github_login, role).await
}

#[derive(Clone)]
struct GitIdentity {
    token: String,
    name: String,
    email: String,
}

async fn get_git_identity(db: &PgPool, github_login: &str) -> Result<GitIdentity, AppError> {
    let row = sqlx::query_as::<_, (String, String, Option<String>)>(
        "SELECT github_token, name, email FROM users WHERE github_login = $1"
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
    _user: MemberUser,
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
    user: AuthUser,
    State(state): State<crate::AppState>,
    Query(params): Query<ListTasksQuery>,
) -> Result<Json<PaginatedTasks>, AppError> {
    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(50).min(200).max(1);
    let offset = ((page - 1) * per_page) as i64;
    let limit = per_page as i64;

    // Build dynamic WHERE clause
    let mut conditions: Vec<String> = Vec::new();
    let mut bind_idx = 1u32;

    // We'll collect bind values as strings for the dynamic query
    let mut bind_values: Vec<String> = Vec::new();

    // Non-admin users only see their own tasks
    if user.0.role != "admin" {
        conditions.push(format!("created_by = ${bind_idx}"));
        bind_values.push(user.0.sub.clone());
        bind_idx += 1;
    }

    if let Some(ref status) = params.status {
        conditions.push(format!("status = ${bind_idx}"));
        bind_values.push(status.clone());
        bind_idx += 1;
    }
    if let Some(ref repo) = params.repo {
        conditions.push(format!("repo = ${bind_idx}"));
        bind_values.push(repo.clone());
        bind_idx += 1;
    }
    if let Some(ref created_by) = params.created_by {
        conditions.push(format!("created_by = ${bind_idx}"));
        bind_values.push(created_by.clone());
        bind_idx += 1;
    }
    if let Some(ref search) = params.search {
        conditions.push(format!("prompt ILIKE ${bind_idx}"));
        bind_values.push(format!("%{search}%"));
        bind_idx += 1;
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    // Count query
    let count_sql = format!("SELECT COUNT(*) FROM tasks {where_clause}");
    let mut count_query = sqlx::query_scalar::<_, i64>(&count_sql);
    for val in &bind_values {
        count_query = count_query.bind(val);
    }
    let total = count_query.fetch_one(&state.db).await?;

    // Data query
    let data_sql = format!(
        "SELECT id, prompt, repo, base_branch, branch_name, agent_name, status, \
         preview_slug, preview_url, error_message, \
         TO_CHAR(created_at, 'YYYY-MM-DD HH24:MI:SS') as created_at, \
         TO_CHAR(updated_at, 'YYYY-MM-DD HH24:MI:SS') as updated_at, \
         parent_task_id, created_by, screenshot_url, image_url, \
         total_input_tokens, total_output_tokens, total_cost_usd, name, pr_url, pr_number, compute_seconds \
         FROM tasks {where_clause} ORDER BY created_at DESC LIMIT ${bind_idx} OFFSET ${next_idx}",
        bind_idx = bind_idx,
        next_idx = bind_idx + 1,
    );
    let mut data_query = sqlx::query_as::<_, Task>(&data_sql);
    for val in &bind_values {
        data_query = data_query.bind(val);
    }
    data_query = data_query.bind(limit).bind(offset);
    let tasks = data_query.fetch_all(&state.db).await?;

    Ok(Json(PaginatedTasks {
        tasks,
        total,
        page,
        per_page,
    }))
}

pub async fn get_task(
    user: AuthUser,
    State(state): State<crate::AppState>,
    Path(id): Path<String>,
) -> Result<Json<Task>, AppError> {
    check_task_ownership(&state.db, &id, &user.0.sub, &user.0.role).await?;
    let task = sqlx::query_as::<_, Task>(
        "SELECT id, prompt, repo, base_branch, branch_name, agent_name, status, \
         preview_slug, preview_url, error_message, \
         TO_CHAR(created_at, 'YYYY-MM-DD HH24:MI:SS') as created_at, \
         TO_CHAR(updated_at, 'YYYY-MM-DD HH24:MI:SS') as updated_at, \
         parent_task_id, created_by, screenshot_url, image_url, \
         total_input_tokens, total_output_tokens, total_cost_usd, name, pr_url, pr_number, compute_seconds \
         FROM tasks WHERE id = $1"
    )
    .bind(&id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("Task not found".into()))?;
    Ok(Json(task))
}

pub async fn get_subtasks(
    user: AuthUser,
    State(state): State<crate::AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<Task>>, AppError> {
    check_task_ownership(&state.db, &id, &user.0.sub, &user.0.role).await?;

    // Verify parent exists
    let _ = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM tasks WHERE id = $1")
        .bind(&id)
        .fetch_one(&state.db)
        .await?;

    let subtasks = sqlx::query_as::<_, Task>(
        "SELECT id, prompt, repo, base_branch, branch_name, agent_name, status, \
         preview_slug, preview_url, error_message, \
         TO_CHAR(created_at, 'YYYY-MM-DD HH24:MI:SS') as created_at, \
         TO_CHAR(updated_at, 'YYYY-MM-DD HH24:MI:SS') as updated_at, \
         parent_task_id, created_by, screenshot_url, image_url, \
         total_input_tokens, total_output_tokens, total_cost_usd, name, pr_url, pr_number, compute_seconds \
         FROM tasks WHERE parent_task_id = $1 ORDER BY created_at ASC"
    )
    .bind(&id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(subtasks))
}

pub async fn create_task(
    user: MemberUser,
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

    // Check per-user repo permission
    check_repo_permission(&state.db, &user.0.sub, &req.repo, &user.0.role, &state.config.github_org).await?;

    // Check budget limits before creating the task
    crate::cost::check_budget(&state.db, &user.0.sub, &state.config.github_org).await?;

    // Validate parent_task_id if provided
    if let Some(ref parent_id) = req.parent_task_id {
        let parent_exists = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM tasks WHERE id = $1")
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
    let created_by = user.0.sub.clone();

    let image_url_json = req
        .image_urls
        .as_ref()
        .filter(|v| !v.is_empty())
        .map(|v| serde_json::to_string(v).unwrap());

    sqlx::query(
        "INSERT INTO tasks (id, prompt, repo, base_branch, status, parent_task_id, created_by, image_url) \
         VALUES ($1, $2, $3, $4, 'pending', $5, $6, $7)",
    )
    .bind(&id)
    .bind(&req.prompt)
    .bind(&req.repo)
    .bind(base_branch)
    .bind(&req.parent_task_id)
    .bind(&created_by)
    .bind(&image_url_json)
    .execute(&state.db)
    .await?;

    // Record initial state transition
    record_state_transition(&state.db, &id, None, "pending").await;

    // Audit: task.created
    crate::audit::log_event(
        &state.db,
        "task.created",
        &created_by,
        Some(&id),
        serde_json::json!({ "repo": &req.repo, "created_by": &created_by }),
        None,
    )
    .await;

    let task = sqlx::query_as::<_, Task>(
        "SELECT id, prompt, repo, base_branch, branch_name, agent_name, status, \
         preview_slug, preview_url, error_message, \
         TO_CHAR(created_at, 'YYYY-MM-DD HH24:MI:SS') as created_at, \
         TO_CHAR(updated_at, 'YYYY-MM-DD HH24:MI:SS') as updated_at, \
         parent_task_id, created_by, screenshot_url, image_url, \
         total_input_tokens, total_output_tokens, total_cost_usd, name, pr_url, pr_number, compute_seconds \
         FROM tasks WHERE id = $1"
    )
    .bind(&id)
    .fetch_one(&state.db)
    .await?;

    // Get the user's git identity (GitHub token, name, email)
    let git_id = get_git_identity(&state.db, &created_by).await?;

    // Save preview_slug and preview_url to the DB right away so the frontend can show the link
    let preview_slug = format!("t-{short_id}");
    let preview_url = format!("https://{preview_slug}.{}", state.config.preview_domain);
    sqlx::query("UPDATE tasks SET preview_slug = $1, preview_url = $2 WHERE id = $3")
        .bind(&preview_slug)
        .bind(&preview_url)
        .bind(&id)
        .execute(&state.db)
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
    let custom_branch = req.custom_branch_name.clone();
    let channels = state.task_channels.clone();

    tokio::spawn(async move {
        let result = run_task_pipeline(
            &config, &db, &task_id, &short, &prompt, &repo, &base, image_url_json2.as_deref(), &git_id, custom_branch, &created_by, tx.clone(),
        )
        .await;

        if let Err(e) = &result {
            let _ = update_task_status(&db, &task_id, "failed", Some(&e.to_string())).await;
            let _ = tx.send(format!("[ERROR] Task failed: {e}"));
            // Clean up agent container on failure
            if let Ok(Some(name)) = sqlx::query_scalar::<_, Option<String>>(
                "SELECT agent_name FROM tasks WHERE id = $1"
            )
            .bind(&task_id)
            .fetch_one(&db)
            .await
            {
                let _ = shell::destroy_agent(&config, &name).await;
            }
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
    db: &PgPool,
    task_id: &str,
    short_id: &str,
    prompt: &str,
    repo: &str,
    base_branch: &str,
    image_url_json: Option<&str>,
    git_id: &GitIdentity,
    custom_branch_name: Option<String>,
    created_by: &str,
    tx: broadcast::Sender<String>,
) -> Result<(), AppError> {
    // Generate a short name for the task using Claude
    let task_name = match generate_task_name(db, &config.secrets_encryption_key, created_by, prompt).await {
        Ok(name) if !name.is_empty() => {
            tracing::info!("Generated task name for {task_id}: {name}");
            name
        }
        Ok(_) => {
            tracing::warn!("Empty task name generated for {task_id}, using fallback");
            format!("task-{short_id}")
        }
        Err(e) => {
            tracing::warn!("Failed to generate task name for {task_id}: {e}");
            format!("task-{short_id}")
        }
    };

    // Save the generated name to the DB
    let _ = sqlx::query("UPDATE tasks SET name = $1 WHERE id = $2")
        .bind(&task_name)
        .bind(task_id)
        .execute(db)
        .await;

    // Use custom branch name if provided, otherwise derive from the task name
    let branch_name = match custom_branch_name {
        Some(ref name) if !name.is_empty() => {
            slugify_for_branch(name)
        }
        _ => {
            let slug = slugify_for_branch(&task_name);
            format!("{slug}-{short_id}")
        }
    };

    // Load effective policy (org + repo merged) for branch protection and tool constraints
    let policy = policies::load_effective_policy(db, repo).await?;
    if let Some(ref pol) = policy {
        // Verify the feature branch name doesn't collide with a protected branch
        if pol.protected_branches.contains(&branch_name) {
            return Err(AppError::BadRequest(format!(
                "Branch '{}' is protected by repo policy. Choose a different branch name.",
                branch_name
            )));
        }
        log_and_send(
            db, task_id, &tx,
            &format!("[POLICY] Loaded policy for {repo}: {} protected branch(es)", pol.protected_branches.len()),
        );
    }

    // Record pipeline start time for compute_seconds tracking
    let pipeline_start = std::time::Instant::now();

    // Step 1: Create agent container
    let agent_name = format!("task-{short_id}");
    update_task_status(db, task_id, "creating_agent", None).await?;
    log_and_send(db, task_id, &tx, &format!("[STEP] Creating agent container '{agent_name}'..."));
    let create_start = std::time::Instant::now();
    shell::create_agent(config, &agent_name).await?;
    let create_ms = create_start.elapsed().as_millis();
    log_and_send(db, task_id, &tx, &format!("[STEP] Agent '{agent_name}' created in {create_ms}ms"));
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

    // Step 2a: Push the empty branch to GitHub, then start the preview on it.
    // The branch has the same commit as base_branch — no code changes yet.
    let base_sha = shell::agent_exec_capture(
        &agent_name,
        &format!("cd /home/agent/repo && git rev-parse 'origin/{base_branch}'"),
    )
    .await?
    .trim()
    .to_string();
    create_github_branch(&git_id.token, repo, &branch_name, &base_sha).await?;
    log_and_send(db, task_id, &tx, &format!("[STEP] Pushed branch '{branch_name}' to GitHub"));

    // Prewarm preview: create it on the feature branch so it's already on the right
    // branch when the agent finishes. Runs in background while Claude works.
    {
        let config2 = config.clone();
        let repo2 = repo.to_string();
        let branch2 = branch_name.clone();
        let slug = format!("t-{short_id}");
        let token = git_id.token.clone();
        tokio::spawn(async move {
            if let Err(e) = shell::create_preview(&config2, &repo2, &branch2, Some(&slug), &token).await {
                tracing::warn!("Preview prewarm failed for {slug}: {e}");
            }
        });
    }

    // Step 2b: Load and inject secrets
    let repo_secrets = secrets::load_secrets_for_repo(db, &config.secrets_encryption_key, repo).await?;
    if !repo_secrets.is_empty() {
        log_and_send(db, task_id, &tx, &format!("[STEP] Injecting {} secret(s)...", repo_secrets.len()));
        write_secrets_env_file(&agent_name, &repo_secrets, tx.clone()).await?;
    }

    // Step 2c: Apply network egress restrictions (if any)
    if let Some(ref pol) = policy {
        if let Some(ref egress) = pol.network_egress {
            log_and_send(db, task_id, &tx, "[POLICY] Applying network egress restrictions...");
            if let Err(e) = shell::apply_egress_rules(&agent_name, egress).await {
                log_and_send(db, task_id, &tx, &format!("[WARN] Failed to apply egress rules: {e}"));
            }
        }
    }

    // Step 3: Copy images and run Claude (streaming)
    let mut effective_prompt = augment_prompt_with_images(config, &agent_name, image_url_json, prompt, &tx).await?;

    // Inject policy constraints into the prompt so Claude knows its boundaries
    if let Some(ref pol) = policy {
        let mut constraints = Vec::new();
        if !pol.protected_branches.is_empty() {
            constraints.push(format!(
                "PROTECTED BRANCHES (do NOT push directly to these): {}",
                pol.protected_branches.join(", ")
            ));
        }
        if let Some(ref tools) = pol.allowed_tools {
            if let Some(deny) = tools.get("deny").and_then(|v| v.as_array()) {
                let names: Vec<&str> = deny.iter().filter_map(|v| v.as_str()).collect();
                if !names.is_empty() {
                    constraints.push(format!("DENIED TOOLS (do NOT use): {}", names.join(", ")));
                }
            }
            if let Some(allow) = tools.get("allow").and_then(|v| v.as_array()) {
                let names: Vec<&str> = allow.iter().filter_map(|v| v.as_str()).collect();
                if !names.is_empty() {
                    constraints.push(format!("ALLOWED TOOLS (use ONLY these): {}", names.join(", ")));
                }
            }
        }
        if !constraints.is_empty() {
            let policy_block = constraints.join("\n");
            effective_prompt = format!(
                "REPO POLICY CONSTRAINTS:\n{policy_block}\n\n{effective_prompt}"
            );
        }
    }

    update_task_status(db, task_id, "running_claude", None).await?;
    // Log which tools are blocked before running Claude so it's visible in the agent logs
    if let Some(ref pol) = policy {
        let denied = get_denied_tools(pol);
        if !denied.is_empty() {
            log_and_send(db, task_id, &tx, &format!(
                "[POLICY] Blocked tools: {}. Claude will not have access to these.",
                denied.join(", ")
            ));
        }
    }
    log_and_send(db, task_id, &tx, "[STEP] Running Claude...");
    let result = match run_claude_streaming(db, &config.secrets_encryption_key, created_by, &agent_name, &effective_prompt, tx.clone(), policy.as_ref()).await {
        Ok(r) => r,
        Err(e) => {
            // If a policy with denied tools is active, the CLI may exit non-zero because
            // it cannot fulfil the request. Don't fail the task — notify the user and
            // fall through to the follow-up loop so they can try a different prompt.
            if let Some(ref pol) = policy {
                let denied = get_denied_tools(pol);
                if !denied.is_empty() {
                    let msg = format!(
                        "Claude could not complete the request — the repo policy blocks these tools: {}.",
                        denied.join(", ")
                    );
                    log_and_send(db, task_id, &tx, &format!("[POLICY] {msg}"));
                    save_system_message(db, task_id, &msg).await?;
                    // Skip to commit/push/follow-up loop with an empty result
                    shell::ClaudeStreamResult {
                        text: String::new(),
                        actions: Vec::new(),
                        usage: shell::TokenUsage { input_tokens: 0, output_tokens: 0, cost_usd: 0.0 },
                    }
                } else {
                    return Err(e);
                }
            } else {
                return Err(e);
            }
        }
    };
    save_claude_response_as_message(db, task_id, &result.text).await?;
    persist_actions(db, task_id, &result.actions).await;
    if let Some(ref pol) = policy {
        check_policy_violations(db, task_id, &result.actions, pol, &tx).await;
    }
    increment_token_usage(db, task_id, &result.usage).await;

    // Check cost limit after initial run — just log a warning, follow-ups will be
    // individually rejected by the check inside the follow-up loop.
    if let Some(ref pol) = policy {
        if let Some(max_cost) = pol.max_cost_usd {
            if check_cost_limit(db, task_id, max_cost, &tx).await? {
                save_system_message(db, task_id, "Cost limit reached — further follow-ups will be rejected by policy.").await?;
            }
        }
    }

    // Step 3b: Commit any changes (if Claude asked a question and made none, that's fine —
    // the user will see the question in chat and respond via the follow-up loop)
    let _ = commit_changes_in_agent(&agent_name, prompt, tx.clone()).await?;

    // Step 3c: Push changes and update preview
    let mut preview_created = true;
    let mut branch_pushed = false;
    let pushed = push_and_preview(config, db, task_id, short_id, &agent_name, repo, &branch_name, base_branch, &mut branch_pushed, &mut preview_created, git_id, &tx).await?;
    if pushed {
        save_system_message(db, task_id, "Changes pushed and preview updated ✓").await?;
    }

    // Step 4: Follow-up loop
    follow_up_loop(config, db, task_id, short_id, &agent_name, repo, &branch_name, base_branch, &mut branch_pushed, &mut preview_created, git_id, created_by, &tx).await?;

    // Step 5: Destroy agent container
    log_and_send(db, task_id, &tx, "[STEP] Destroying agent container...");
    let _ = shell::destroy_agent(config, &agent_name).await;

    // Record compute time (wall-clock seconds from agent creation to destruction)
    let compute_secs = pipeline_start.elapsed().as_secs() as i32;
    let _ = sqlx::query("UPDATE tasks SET compute_seconds = $1 WHERE id = $2")
        .bind(compute_secs)
        .bind(task_id)
        .execute(db)
        .await;

    // Done
    let preview_url = format!("https://t-{short_id}.{}", config.preview_domain);
    update_task_status(db, task_id, "completed", None).await?;
    log_and_send(db, task_id, &tx, &format!("[DONE] Preview available at {preview_url}"));

    Ok(())
}

/// Build the shell export fragment that sets AI credentials for a user.
/// Returns (env_export_string, model_flag) where model_flag is always `""`.
/// Model selection uses ANTHROPIC_MODEL env var rather than --model flag to
/// avoid the Claude CLI's client-side validation against Anthropic-only names.
async fn build_claude_auth_env(
    db: &PgPool,
    encryption_key: &str,
    created_by: &str,
) -> Result<(String, String), AppError> {
    match settings::get_user_ai_config(db, encryption_key, created_by).await? {
        Some(cfg) if cfg.provider == "openrouter" => {
            let model = cfg.model.as_deref().unwrap_or("anthropic/claude-sonnet-4.6");
            Ok((
                format!(
                    "export ANTHROPIC_BASE_URL='https://openrouter.ai/api' ANTHROPIC_AUTH_TOKEN='{}' ANTHROPIC_API_KEY='' ANTHROPIC_MODEL='{}'",
                    cfg.api_key, model
                ),
                String::new(),
            ))
        }
        Some(cfg) => Ok((format!("export ANTHROPIC_API_KEY='{}'", cfg.api_key), String::new())),
        None => Err(AppError::Internal(
            "No AI provider configured. Go to Settings → AI Provider to connect your account."
                .into(),
        )),
    }
}

/// Generate a short task name from the prompt using Claude CLI.
/// Runs as a non-root user since claude --dangerously-skip-permissions refuses root.
/// Returns Err if the user has no AI config — the call site falls back gracefully.
async fn generate_task_name(
    db: &PgPool,
    encryption_key: &str,
    created_by: &str,
    prompt: &str,
) -> Result<String, AppError> {
    let cfg = settings::get_user_ai_config(db, encryption_key, created_by)
        .await?
        .ok_or_else(|| {
            AppError::Internal(
                "No AI provider configured for task name generation".into(),
            )
        })?;

    let naming_prompt = format!(
        "Generate a very short name (3-5 words, no quotes, no punctuation) that summarizes this coding task. \
         Reply with ONLY the name, nothing else.\n\nTask: {}", prompt
    );

    let mut env_args = vec![
        format!("ANTHROPIC_API_KEY={}", if cfg.provider == "openrouter" { "" } else { &cfg.api_key }),
        "HOME=/tmp".to_string(),
    ];
    if cfg.provider == "openrouter" {
        let model = cfg.model.as_deref().unwrap_or("anthropic/claude-sonnet-4.6");
        env_args.push(format!("ANTHROPIC_AUTH_TOKEN={}", cfg.api_key));
        env_args.push("ANTHROPIC_BASE_URL=https://openrouter.ai/api".to_string());
        env_args.push(format!("ANTHROPIC_MODEL={model}"));
    }

    let mut cmd_args = vec!["-u".to_string(), "nobody".to_string(), "env".to_string()];
    cmd_args.extend(env_args);
    let mut claude_args = vec![
        "claude".to_string(),
        "--dangerously-skip-permissions".to_string(),
    ];
    claude_args.extend(["-p".to_string(), naming_prompt]);
    cmd_args.extend(claude_args);

    let output = tokio::process::Command::new("sudo")
        .args(&cmd_args)
        .output()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to run claude for naming: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::Internal(format!(
            "Claude naming command failed (exit {}): {stderr}",
            output.status
        )));
    }

    let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
    // Take at most 60 chars
    let name = if name.len() > 60 { name[..60].to_string() } else { name };
    Ok(name)
}

/// Convert a task name into a slug suitable for git branch names.
fn slugify_for_branch(name: &str) -> String {
    let slug: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    // Collapse multiple dashes and trim
    let mut result = String::new();
    let mut prev_dash = false;
    for c in slug.chars() {
        if c == '-' {
            if !prev_dash && !result.is_empty() {
                result.push('-');
            }
            prev_dash = true;
        } else {
            result.push(c);
            prev_dash = false;
        }
    }
    result.trim_end_matches('-').to_string()
}

/// Extract the list of denied tool names from a policy.
fn get_denied_tools(policy: &crate::models::RepoPolicy) -> Vec<String> {
    let tools = match &policy.allowed_tools {
        Some(t) => t,
        None => return Vec::new(),
    };
    if let Some(deny) = tools.get("deny").and_then(|v| v.as_array()) {
        deny.iter().filter_map(|v| v.as_str()).map(String::from).collect()
    } else {
        Vec::new()
    }
}

/// Build a `--disallowedTools` CLI flag from the policy's deny list.
/// Returns an empty string if there is no policy or no denied tools.
fn build_disallowed_tools_flag(policy: Option<&crate::models::RepoPolicy>) -> String {
    let pol = match policy {
        Some(p) => p,
        None => return String::new(),
    };
    let tools = match &pol.allowed_tools {
        Some(t) => t,
        None => return String::new(),
    };

    let mut denied: Vec<&str> = Vec::new();

    // Explicit deny list
    if let Some(deny) = tools.get("deny").and_then(|v| v.as_array()) {
        denied.extend(deny.iter().filter_map(|v| v.as_str()));
    }

    if denied.is_empty() {
        return String::new();
    }

    // The Claude CLI accepts: --disallowedTools "Tool1,Tool2"
    format!("--disallowedTools '{}'", denied.join(","))
}

async fn run_claude_streaming(
    db: &PgPool,
    encryption_key: &str,
    created_by: &str,
    agent_name: &str,
    prompt: &str,
    tx: broadcast::Sender<String>,
    policy: Option<&crate::models::RepoPolicy>,
) -> Result<shell::ClaudeStreamResult, AppError> {
    let (auth_env, model_flag) = build_claude_auth_env(db, encryption_key, created_by).await?;
    let disallowed_flag = build_disallowed_tools_flag(policy);
    let escaped_prompt = prompt.replace('\'', "'\\''");
    let claude_cmd = format!(
        "source /home/agent/.env.sh 2>/dev/null ; {auth_env} && cd /home/agent/repo && claude --dangerously-skip-permissions --output-format stream-json --verbose {model_flag} {disallowed_flag} -p '{escaped_prompt}'"
    );
    shell::agent_exec_claude_streaming(agent_name, &claude_cmd, tx).await
}

/// Run Claude with --continue to maintain conversation context for follow-ups.
async fn run_claude_continue(
    db: &PgPool,
    encryption_key: &str,
    created_by: &str,
    agent_name: &str,
    prompt: &str,
    tx: broadcast::Sender<String>,
    policy: Option<&crate::models::RepoPolicy>,
) -> Result<shell::ClaudeStreamResult, AppError> {
    let (auth_env, model_flag) = build_claude_auth_env(db, encryption_key, created_by).await?;
    let disallowed_flag = build_disallowed_tools_flag(policy);
    let escaped_prompt = prompt.replace('\'', "'\\''");
    let claude_cmd = format!(
        "source /home/agent/.env.sh 2>/dev/null ; {auth_env} && cd /home/agent/repo && \
         claude --dangerously-skip-permissions --output-format stream-json --verbose \
         {model_flag} {disallowed_flag} --continue -p '{escaped_prompt}'"
    );
    shell::agent_exec_claude_streaming(agent_name, &claude_cmd, tx).await
}

/// Save Claude's text response as a chat message so it appears in the UI.
/// Full response is stored — no truncation.
async fn save_claude_response_as_message(
    db: &PgPool,
    task_id: &str,
    claude_text: &str,
) -> Result<(), AppError> {
    let text = claude_text.trim();
    if text.is_empty() {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO task_messages (task_id, sender, content) VALUES ($1, 'claude', $2)",
    )
    .bind(task_id)
    .bind(text)
    .execute(db)
    .await?;
    Ok(())
}

/// Save a system/status message to the chat so it appears inline in the UI.
async fn save_system_message(db: &PgPool, task_id: &str, content: &str) -> Result<(), AppError> {
    sqlx::query("INSERT INTO task_messages (task_id, sender, content) VALUES ($1, 'system', $2)")
        .bind(task_id)
        .bind(content)
        .execute(db)
        .await?;
    Ok(())
}

/// Batch-insert structured actions into the task_actions table.
async fn persist_actions(db: &PgPool, task_id: &str, actions: &[shell::RawAction]) {
    for action in actions {
        let _ = sqlx::query(
            "INSERT INTO task_actions (task_id, action_type, tool_name, tool_input, summary) \
             VALUES ($1, $2, $3, $4, $5)"
        )
        .bind(task_id)
        .bind(&action.action_type)
        .bind(&action.tool_name)
        .bind(&action.tool_input)
        .bind(&action.summary)
        .execute(db)
        .await;
    }
}

/// Check all tool_use actions against the policy and log violations only.
async fn check_policy_violations(
    db: &PgPool,
    task_id: &str,
    actions: &[shell::RawAction],
    policy: &crate::models::RepoPolicy,
    tx: &broadcast::Sender<String>,
) {
    for action in actions {
        if action.action_type != "tool_use" {
            continue;
        }
        let tool_name = match &action.tool_name {
            Some(n) => n,
            None => continue,
        };

        if let Some(reason) = policies::check_tool_denied(policy, tool_name) {
            let summary = format!("POLICY VIOLATION: {} — {}", tool_name, reason);
            let _ = sqlx::query(
                "INSERT INTO task_actions (task_id, action_type, tool_name, tool_input, summary) \
                 VALUES ($1, 'policy_violation', $2, $3, $4)",
            )
            .bind(task_id)
            .bind(tool_name)
            .bind(&action.tool_input)
            .bind(&summary)
            .execute(db)
            .await;
            let _ = tx.send(format!("[POLICY] {summary}"));
            tracing::warn!("Policy violation in task {task_id}: {summary}");
        }
    }
}

/// Increment the task's cumulative token usage counters and cost.
async fn increment_token_usage(db: &PgPool, task_id: &str, usage: &shell::TokenUsage) {
    if usage.input_tokens == 0 && usage.output_tokens == 0 && usage.cost_usd == 0.0 {
        return;
    }
    let _ = sqlx::query(
        "UPDATE tasks SET \
         total_input_tokens = COALESCE(total_input_tokens, 0) + $1, \
         total_output_tokens = COALESCE(total_output_tokens, 0) + $2, \
         total_cost_usd = COALESCE(total_cost_usd, 0) + $3, \
         updated_at = NOW() \
         WHERE id = $4"
    )
    .bind(usage.input_tokens)
    .bind(usage.output_tokens)
    .bind(usage.cost_usd)
    .bind(task_id)
    .execute(db)
    .await;
}

/// Check whether the task has exceeded its cost limit.
/// Uses the real cost reported by the Claude CLI (stored in total_cost_usd).
/// Returns `true` if the limit has been reached or exceeded.
async fn check_cost_limit(
    db: &PgPool,
    task_id: &str,
    max_cost_usd: f64,
    tx: &broadcast::Sender<String>,
) -> Result<bool, AppError> {
    let cost: f64 = sqlx::query_scalar(
        "SELECT COALESCE(total_cost_usd, 0) FROM tasks WHERE id = $1",
    )
    .bind(task_id)
    .fetch_one(db)
    .await?;

    if cost >= max_cost_usd {
        let msg = format!(
            "[POLICY] Cost limit reached: ${:.4} spent >= ${:.4} limit",
            cost, max_cost_usd,
        );
        tracing::warn!("Task {task_id}: {msg}");
        let _ = tx.send(msg);
        return Ok(true);
    }

    Ok(false)
}

/// Record a state transition in the task_state_transitions table.
async fn record_state_transition(db: &PgPool, task_id: &str, from: Option<&str>, to: &str) {
    let _ = sqlx::query(
        "INSERT INTO task_state_transitions (task_id, from_status, to_status) VALUES ($1, $2, $3)"
    )
    .bind(task_id)
    .bind(from)
    .bind(to)
    .execute(db)
    .await;
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
    db: &PgPool,
    task_id: &str,
    short_id: &str,
    agent_name: &str,
    repo: &str,
    branch_name: &str,
    base_branch: &str,
    branch_pushed: &mut bool,
    preview_created: &mut bool,
    git_id: &GitIdentity,
    created_by: &str,
    tx: &broadcast::Sender<String>,
) -> Result<FollowUpOutcome, AppError> {
    let mut last_seen_id: i64 = 0;

    // Track conversation history for context (used as fallback if --continue fails)
    let mut conversation_history: Vec<String> = Vec::new();

    // If the task has a preview, inject a preview-logs helper script into the agent container
    // so Claude can fetch live preview output on demand during follow-ups.
    let preview_slug: Option<String> = sqlx::query_scalar(
        "SELECT preview_slug FROM tasks WHERE id = $1",
    )
    .bind(task_id)
    .fetch_optional(db)
    .await?
    .flatten();

    let has_preview_tool = if let Some(ref slug) = preview_slug {
        match shell::agent_host_ip(agent_name) {
            Ok(host_ip) => {
                let script = format!(
                    "#!/bin/sh\ncurl -sf \"http://{host_ip}:3200/internal/preview-logs/{slug}\" || echo \"Preview logs unavailable\"\n"
                );
                let escaped = script.replace('\'', "'\\''");
                let write_cmd = format!(
                    "mkdir -p /home/agent/bin && printf '%s' '{escaped}' > /home/agent/bin/preview-logs && chmod +x /home/agent/bin/preview-logs"
                );
                if let Err(e) = shell::agent_exec_capture(agent_name, &write_cmd).await {
                    tracing::warn!("Failed to inject preview-logs script: {e}");
                    false
                } else {
                    true
                }
            }
            Err(e) => {
                tracing::warn!("Could not get agent host IP for preview-logs tool: {e}");
                false
            }
        }
    } else {
        false
    };

    update_task_status(db, task_id, "awaiting_followup", None).await?;
    let _ = tx.send("[STATUS] Waiting for follow-up messages (click 'Mark Done' to finish)...".to_string());

    loop {
        // Check for new messages IMMEDIATELY (no sleep before first check)
        // Filter out claude's own messages so we only process user messages
        let new_messages: Vec<TaskMessage> = sqlx::query_as::<_, TaskMessage>(
            "SELECT id, task_id, sender, content, \
             TO_CHAR(created_at, 'YYYY-MM-DD HH24:MI:SS') as created_at, image_url \
             FROM task_messages \
             WHERE task_id = $1 AND id > $2 AND sender NOT IN ('claude', 'system') \
             ORDER BY id ASC",
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

        // Reload policy from DB each iteration so changes (add/remove/edit) take effect immediately
        let policy = policies::load_effective_policy(db, repo).await?;

        // Batch all pending messages into one prompt
        let mut combined_parts: Vec<String> = Vec::new();
        for msg in &new_messages {
            let effective = augment_prompt_with_images(
                config, agent_name, msg.image_url.as_deref(), &msg.content, tx,
            ).await?;
            combined_parts.push(effective);
            conversation_history.push(format!("{}: {}", msg.sender, msg.content));
        }
        let mut combined_prompt = combined_parts.join("\n\n---\n\n");
        if has_preview_tool {
            combined_prompt = format!(
                "Note: you have a `~/bin/preview-logs` command that prints the last 100 lines \
                 of the live preview app output. Use it when the user reports an error or \
                 something not working as expected.\n\n{combined_prompt}"
            );
        }

        // Check cost limit before running follow-up — reject this message but keep the task open
        if let Some(ref pol) = policy {
            if let Some(max_cost) = pol.max_cost_usd {
                if check_cost_limit(db, task_id, max_cost, tx).await? {
                    save_system_message(db, task_id, "Cost limit reached — this follow-up was blocked by policy.").await?;
                    update_task_status(db, task_id, "awaiting_followup", None).await?;
                    continue;
                }
            }
        }

        // Run Claude with --continue to maintain conversation context
        save_system_message(db, task_id, "Running Claude with your follow-up...").await?;
        update_task_status(db, task_id, "running_claude", None).await?;
        let _ = tx.send("[FOLLOWUP] Running Claude with follow-up...".to_string());

        let result = match run_claude_continue(db, &config.secrets_encryption_key, created_by, agent_name, &combined_prompt, tx.clone(), policy.as_ref()).await {
            Ok(r) => r,
            Err(e) => {
                // Fallback: if --continue fails, use fresh session with full context
                let _ = tx.send(format!("[WARN] --continue failed ({e}), falling back to fresh session..."));
                let original_prompt = sqlx::query_scalar::<_, String>("SELECT prompt FROM tasks WHERE id = $1")
                    .bind(task_id)
                    .fetch_one(db)
                    .await?;
                let context_prompt = build_followup_prompt(&original_prompt, &conversation_history, &combined_prompt);
                run_claude_streaming(db, &config.secrets_encryption_key, created_by, agent_name, &context_prompt, tx.clone(), policy.as_ref()).await?
            }
        };
        save_claude_response_as_message(db, task_id, &result.text).await?;
        persist_actions(db, task_id, &result.actions).await;
        if let Some(ref pol) = policy {
            check_policy_violations(db, task_id, &result.actions, pol, tx).await;
        }
        increment_token_usage(db, task_id, &result.usage).await;

        let _ = commit_changes_in_agent(agent_name, &combined_parts[0], tx.clone()).await?;

        // Push and update preview
        let pushed = push_and_preview(config, db, task_id, short_id, agent_name, repo, branch_name, base_branch, branch_pushed, preview_created, git_id, tx).await?;
        if pushed {
            save_system_message(db, task_id, "Changes pushed and preview updated ✓").await?;
        }

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

/// Send an HTTP request with retries and exponential backoff.
/// Retries on network errors and 5xx server errors up to `max_retries` times.
/// Returns the response on success or the last error after all retries are exhausted.
async fn send_with_retries(
    max_retries: u32,
    operation: &str,
    build_request: impl Fn() -> reqwest::RequestBuilder,
) -> Result<reqwest::Response, AppError> {
    let mut last_err = None;

    for attempt in 0..=max_retries {
        match build_request().send().await {
            Ok(resp) if resp.status().is_server_error() => {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                last_err = Some(format!("{operation} failed ({status}): {body}"));
            }
            Ok(resp) => return Ok(resp),
            Err(e) => {
                last_err = Some(format!("{operation} failed: {e}"));
            }
        }

        if attempt < max_retries {
            let delay = std::time::Duration::from_secs(1 << attempt);
            tracing::warn!(
                "{operation} (attempt {}/{}), retrying in {delay:?}: {}",
                attempt + 1,
                max_retries + 1,
                last_err.as_deref().unwrap_or("unknown"),
            );
            tokio::time::sleep(delay).await;
        }
    }

    Err(AppError::Internal(
        last_err.unwrap_or_else(|| format!("{operation} failed after retries")),
    ))
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

    let resp = send_with_retries(10, "GitHub create branch", || {
        client
            .post(&url)
            .header("Authorization", format!("Bearer {token}"))
            .header("User-Agent", "dashboard")
            .header("Accept", "application/vnd.github+json")
            .json(&body)
    })
    .await?;

    let status = resp.status();
    if !status.is_success() {
        // 422 = branch already exists, which is fine (e.g. we created it early for preview prewarm)
        if status.as_u16() == 422 {
            return Ok(());
        }
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

    let resp = send_with_retries(10, "GitHub GraphQL request", || {
        client
            .post("https://api.github.com/graphql")
            .header("Authorization", format!("Bearer {token}"))
            .header("User-Agent", "dashboard")
            .json(&body)
    })
    .await?;

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
    db: &PgPool,
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
) -> Result<bool, AppError> {
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
        return Ok(false);
    }

    save_system_message(db, task_id, "Deploying changes to preview...").await?;

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
    let commit_msg = sqlx::query_scalar::<_, String>("SELECT prompt FROM tasks WHERE id = $1")
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

        shell::create_preview(config, repo, branch_name, Some(&preview_slug), &git_id.token).await?;

        let preview_url = format!("https://{preview_slug}.{}", config.preview_domain);
        update_task_field(db, task_id, "preview_slug", &preview_slug).await?;
        update_task_field(db, task_id, "preview_url", &preview_url).await?;
        *preview_created = true;
    } else {
        log_and_send(db, task_id, tx, &format!("[STEP] Updating preview '{preview_slug}'..."));
        let _ = shell::update_preview(config, &preview_slug, &git_id.token).await;
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

    Ok(true)
}

async fn take_screenshot(
    config: &Config,
    db: &PgPool,
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
            "--no-sandbox",
            &format!("--screenshot={screenshot_path}"),
            "--window-size=1280,720",
            preview_url,
        ])
        .output()
        .await;

    match output {
        Ok(o) if o.status.success() => {
            let _ = sqlx::query(
                "UPDATE tasks SET screenshot_url = $1, updated_at = NOW() WHERE id = $2",
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

// ── Repos ──

pub async fn list_repos(
    _user: AuthUser,
    State(state): State<crate::AppState>,
) -> Result<Json<Vec<String>>, AppError> {
    let rows = sqlx::query_scalar::<_, String>(
        "SELECT DISTINCT repo FROM tasks ORDER BY repo"
    )
    .fetch_all(&state.db)
    .await?;

    Ok(Json(rows))
}

// ── Branches ──

pub async fn list_branches(
    user: AuthUser,
    State(state): State<crate::AppState>,
    Path((owner, repo)): Path<(String, String)>,
) -> Result<Json<Vec<String>>, AppError> {
    let full_repo = format!("{owner}/{repo}");
    check_repo_permission(&state.db, &user.0.sub, &full_repo, &user.0.role, &state.config.github_org).await?;
    let git_id = get_git_identity(&state.db, &user.0.sub).await?;

    let client = reqwest::Client::new();
    let mut branches = Vec::new();
    let mut page = 1u32;

    loop {
        let resp = client
            .get(format!(
                "https://api.github.com/repos/{owner}/{repo}/branches?per_page=100&page={page}"
            ))
            .header("Authorization", format!("token {}", git_id.token))
            .header("User-Agent", "tekton-dashboard")
            .header("Accept", "application/vnd.github+json")
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

        let items: Vec<serde_json::Value> = resp.json().await
            .map_err(|e| AppError::Internal(format!("Failed to parse branches response: {e}")))?;

        if items.is_empty() {
            break;
        }

        for item in &items {
            if let Some(name) = item["name"].as_str() {
                branches.push(name.to_string());
            }
        }

        if items.len() < 100 {
            break;
        }
        page += 1;
    }

    Ok(Json(branches))
}

// ── Messages ──

pub async fn list_messages(
    user: AuthUser,
    State(state): State<crate::AppState>,
    Path(id): Path<String>,
    Query(params): Query<ListMessagesQuery>,
) -> Result<Json<Vec<TaskMessage>>, AppError> {
    check_task_ownership(&state.db, &id, &user.0.sub, &user.0.role).await?;

    let messages = if let Some(before_id) = params.before_id {
        let limit = params.limit.unwrap_or(100);
        sqlx::query_as::<_, TaskMessage>(
            "SELECT id, task_id, sender, content, \
             TO_CHAR(created_at, 'YYYY-MM-DD HH24:MI:SS') as created_at, image_url \
             FROM task_messages WHERE task_id = $1 AND id < $2 \
             ORDER BY created_at ASC LIMIT $3",
        )
        .bind(&id)
        .bind(before_id)
        .bind(limit)
        .fetch_all(&state.db)
        .await?
    } else if let Some(limit) = params.limit {
        sqlx::query_as::<_, TaskMessage>(
            "SELECT id, task_id, sender, content, \
             TO_CHAR(created_at, 'YYYY-MM-DD HH24:MI:SS') as created_at, image_url \
             FROM task_messages WHERE task_id = $1 \
             ORDER BY created_at ASC LIMIT $2",
        )
        .bind(&id)
        .bind(limit)
        .fetch_all(&state.db)
        .await?
    } else {
        sqlx::query_as::<_, TaskMessage>(
            "SELECT id, task_id, sender, content, \
             TO_CHAR(created_at, 'YYYY-MM-DD HH24:MI:SS') as created_at, image_url \
             FROM task_messages WHERE task_id = $1 \
             ORDER BY created_at ASC",
        )
        .bind(&id)
        .fetch_all(&state.db)
        .await?
    };

    Ok(Json(messages))
}

pub async fn send_message(
    user: MemberUser,
    State(state): State<crate::AppState>,
    Path(id): Path<String>,
    Json(req): Json<SendMessageRequest>,
) -> Result<Json<TaskMessage>, AppError> {
    check_task_ownership(&state.db, &id, &user.0.sub, &user.0.role).await?;

    // Verify task exists
    let task_exists = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM tasks WHERE id = $1")
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
        "INSERT INTO task_messages (task_id, sender, content, image_url) VALUES ($1, $2, $3, $4)",
    )
    .bind(&id)
    .bind(sender)
    .bind(&req.content)
    .bind(&image_url_json)
    .execute(&state.db)
    .await?;

    let message = sqlx::query_as::<_, TaskMessage>(
        "SELECT id, task_id, sender, content, \
         TO_CHAR(created_at, 'YYYY-MM-DD HH24:MI:SS') as created_at, image_url \
         FROM task_messages WHERE task_id = $1 ORDER BY id DESC LIMIT 1",
    )
    .bind(&id)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(message))
}

// ── Actions endpoint ──

pub async fn list_actions(
    user: AuthUser,
    State(state): State<crate::AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<TaskAction>>, AppError> {
    check_task_ownership(&state.db, &id, &user.0.sub, &user.0.role).await?;
    let actions = sqlx::query_as::<_, TaskAction>(
        "SELECT id, task_id, action_type, tool_name, tool_input, summary, \
         TO_CHAR(created_at, 'YYYY-MM-DD HH24:MI:SS') as created_at \
         FROM task_actions WHERE task_id = $1 ORDER BY id ASC",
    )
    .bind(&id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(actions))
}

// ── Helpers ──

/// Write a .env.sh script with exported secrets into the agent container.
async fn write_secrets_env_file(
    agent_name: &str,
    repo_secrets: &[(String, String)],
    tx: broadcast::Sender<String>,
) -> Result<(), AppError> {
    let env_lines: Vec<String> = repo_secrets
        .iter()
        .map(|(name, value)| format!("export {}='{}'", name, value.replace('\'', "'\\''")))
        .collect();
    let env_script = env_lines.join("\n");
    let escaped_script = env_script.replace('\'', "'\\''");
    shell::agent_exec(
        agent_name,
        &format!("printf '%s\\n' '{escaped_script}' > /home/agent/.env.sh"),
        tx,
    )
    .await
}

/// Replace any occurrence of secret values in a log message with [REDACTED].
#[allow(dead_code)]
pub fn scrub_secrets(msg: &str, repo_secrets: &[(String, String)]) -> String {
    let mut result = msg.to_string();
    for (_, value) in repo_secrets {
        if !value.is_empty() {
            result = result.replace(value.as_str(), "[REDACTED]");
        }
    }
    result
}

fn log_and_send(
    db: &PgPool,
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
        let _ = sqlx::query("INSERT INTO task_logs (task_id, line) VALUES ($1, $2)")
            .bind(&task_id)
            .bind(&msg)
            .execute(&db)
            .await;
    });
}

async fn update_task_status(
    db: &PgPool,
    task_id: &str,
    status: &str,
    error: Option<&str>,
) -> Result<(), AppError> {
    // Get the current status for the state transition record
    let current_status = sqlx::query_scalar::<_, String>(
        "SELECT status FROM tasks WHERE id = $1"
    )
    .bind(task_id)
    .fetch_optional(db)
    .await?;

    sqlx::query(
        "UPDATE tasks SET status = $1, error_message = $2, updated_at = NOW() WHERE id = $3",
    )
    .bind(status)
    .bind(error)
    .bind(task_id)
    .execute(db)
    .await?;

    // Record state transition
    record_state_transition(db, task_id, current_status.as_deref(), status).await;

    Ok(())
}

async fn update_task_field(
    db: &PgPool,
    task_id: &str,
    field: &str,
    value: &str,
) -> Result<(), AppError> {
    // Safe because field is always a hardcoded string from our code
    let query = format!(
        "UPDATE tasks SET {field} = $1, updated_at = NOW() WHERE id = $2"
    );
    sqlx::query(&query)
        .bind(value)
        .bind(task_id)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn reopen_task(
    user: MemberUser,
    State(state): State<crate::AppState>,
    Path(id): Path<String>,
) -> Result<Json<Task>, AppError> {
    check_task_ownership(&state.db, &id, &user.0.sub, &user.0.role).await?;

    let task = sqlx::query_as::<_, Task>(
        "SELECT id, prompt, repo, base_branch, branch_name, agent_name, status, \
         preview_slug, preview_url, error_message, \
         TO_CHAR(created_at, 'YYYY-MM-DD HH24:MI:SS') as created_at, \
         TO_CHAR(updated_at, 'YYYY-MM-DD HH24:MI:SS') as updated_at, \
         parent_task_id, created_by, screenshot_url, image_url, \
         total_input_tokens, total_output_tokens, total_cost_usd, name, pr_url, pr_number, compute_seconds \
         FROM tasks WHERE id = $1"
    )
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
    let created_by = user.0.sub.clone();

    tokio::spawn(async move {
        let result = run_reopen_pipeline(
            &config, &db, &task_id, &short, &repo, &branch, &base, has_preview, &git_id, &created_by, tx.clone(),
        )
        .await;

        if let Err(e) = &result {
            let _ = update_task_status(&db, &task_id, "failed", Some(&e.to_string())).await;
            let _ = tx.send(format!("[ERROR] Reopen failed: {e}"));
            if let Ok(Some(name)) = sqlx::query_scalar::<_, Option<String>>(
                "SELECT agent_name FROM tasks WHERE id = $1"
            )
            .bind(&task_id)
            .fetch_one(&db)
            .await
            {
                let _ = shell::destroy_agent(&config, &name).await;
            }
        }

        let channels2 = channels;
        let tid = task_id.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            channels2.remove(&tid);
        });
    });

    let task = sqlx::query_as::<_, Task>(
        "SELECT id, prompt, repo, base_branch, branch_name, agent_name, status, \
         preview_slug, preview_url, error_message, \
         TO_CHAR(created_at, 'YYYY-MM-DD HH24:MI:SS') as created_at, \
         TO_CHAR(updated_at, 'YYYY-MM-DD HH24:MI:SS') as updated_at, \
         parent_task_id, created_by, screenshot_url, image_url, \
         total_input_tokens, total_output_tokens, total_cost_usd, name, pr_url, pr_number, compute_seconds \
         FROM tasks WHERE id = $1"
    )
    .bind(&id)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(task))
}

pub async fn get_task_diff(
    user: AuthUser,
    State(state): State<crate::AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    check_task_ownership(&state.db, &id, &user.0.sub, &user.0.role).await?;

    let task = sqlx::query_as::<_, Task>(
        "SELECT id, prompt, repo, base_branch, branch_name, agent_name, status, \
         preview_slug, preview_url, error_message, \
         TO_CHAR(created_at, 'YYYY-MM-DD HH24:MI:SS') as created_at, \
         TO_CHAR(updated_at, 'YYYY-MM-DD HH24:MI:SS') as updated_at, \
         parent_task_id, created_by, screenshot_url, image_url, \
         total_input_tokens, total_output_tokens, total_cost_usd, name, pr_url, pr_number, compute_seconds \
         FROM tasks WHERE id = $1",
    )
    .bind(&id)
    .fetch_one(&state.db)
    .await?;

    let Some(branch_name) = task.branch_name.as_deref().filter(|b| !b.is_empty()) else {
        return Ok(Json(serde_json::json!({ "diff": "" })));
    };

    let git_id = get_git_identity(&state.db, &user.0.sub).await?;
    let client = reqwest::Client::new();
    let diff = match client
        .get(format!(
            "https://api.github.com/repos/{}/compare/{}...{}",
            task.repo, task.base_branch, branch_name
        ))
        .header("Authorization", format!("token {}", git_id.token))
        .header("User-Agent", "tekton-dashboard")
        .header("Accept", "application/vnd.github.v3.diff")
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => resp.text().await.unwrap_or_default(),
        _ => String::new(),
    };

    Ok(Json(serde_json::json!({ "diff": diff })))
}

pub async fn update_task_name(
    user: MemberUser,
    State(state): State<crate::AppState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateTaskNameRequest>,
) -> Result<Json<Task>, AppError> {
    check_task_ownership(&state.db, &id, &user.0.sub, &user.0.role).await?;
    let _ = sqlx::query_as::<_, Task>(
        "SELECT id, prompt, repo, base_branch, branch_name, agent_name, status, \
         preview_slug, preview_url, error_message, \
         TO_CHAR(created_at, 'YYYY-MM-DD HH24:MI:SS') as created_at, \
         TO_CHAR(updated_at, 'YYYY-MM-DD HH24:MI:SS') as updated_at, \
         parent_task_id, created_by, screenshot_url, image_url, \
         total_input_tokens, total_output_tokens, total_cost_usd, name, pr_url, pr_number, compute_seconds \
         FROM tasks WHERE id = $1"
    )
    .bind(&id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("Task not found".into()))?;

    sqlx::query("UPDATE tasks SET name = $1, updated_at = NOW() WHERE id = $2")
        .bind(&req.name)
        .bind(&id)
        .execute(&state.db)
        .await?;

    let task = sqlx::query_as::<_, Task>(
        "SELECT id, prompt, repo, base_branch, branch_name, agent_name, status, \
         preview_slug, preview_url, error_message, \
         TO_CHAR(created_at, 'YYYY-MM-DD HH24:MI:SS') as created_at, \
         TO_CHAR(updated_at, 'YYYY-MM-DD HH24:MI:SS') as updated_at, \
         parent_task_id, created_by, screenshot_url, image_url, \
         total_input_tokens, total_output_tokens, total_cost_usd, name, pr_url, pr_number, compute_seconds \
         FROM tasks WHERE id = $1"
    )
    .bind(&id)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(task))
}

async fn run_reopen_pipeline(
    config: &Config,
    db: &PgPool,
    task_id: &str,
    short_id: &str,
    repo: &str,
    branch_name: &str,
    base_branch: &str,
    had_preview: bool,
    git_id: &GitIdentity,
    created_by: &str,
    tx: broadcast::Sender<String>,
) -> Result<(), AppError> {
    // Check if the previous agent container is still alive (e.g. server crashed mid-run).
    // Look up the last-known agent_name from the DB.
    let prev_agent = sqlx::query_scalar::<_, Option<String>>(
        "SELECT agent_name FROM tasks WHERE id = $1"
    )
    .bind(task_id)
    .fetch_one(db)
    .await
    .ok()
    .flatten();

    let container_exists = prev_agent
        .as_deref()
        .map(|n| shell::agent_ip_public(n).is_ok())
        .unwrap_or(false);

    let agent_name = if container_exists {
        let name = prev_agent.unwrap();
        log_and_send(db, task_id, &tx, &format!("[STEP] Reusing existing agent container '{name}'..."));
        update_task_field(db, task_id, "agent_name", &name).await?;
        name
    } else {
        // Step 1: Create agent container
        let name = format!("task-{short_id}");
        log_and_send(db, task_id, &tx, &format!("[STEP] Creating agent container '{name}' (reopen)..."));
        let create_start = std::time::Instant::now();
        shell::create_agent(config, &name).await?;
        let create_ms = create_start.elapsed().as_millis();
        log_and_send(db, task_id, &tx, &format!("[STEP] Agent '{name}' created in {create_ms}ms"));
        update_task_field(db, task_id, "agent_name", &name).await?;

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
        shell::agent_exec(&name, &clone_cmd, tx.clone()).await?;

        // Step 2b: Load and inject secrets
        let repo_secrets = secrets::load_secrets_for_repo(db, &config.secrets_encryption_key, repo).await?;
        if !repo_secrets.is_empty() {
            log_and_send(db, task_id, &tx, &format!("[STEP] Injecting {} secret(s)...", repo_secrets.len()));
            write_secrets_env_file(&name, &repo_secrets, tx.clone()).await?;
        }
        name
    };

    // Load effective policy for enforcement during follow-ups
    let policy = policies::load_effective_policy(db, repo).await?;

    // Apply network egress restrictions (if any)
    if let Some(ref pol) = policy {
        if let Some(ref egress) = pol.network_egress {
            log_and_send(db, task_id, &tx, "[POLICY] Applying network egress restrictions...");
            if let Err(e) = shell::apply_egress_rules(&agent_name, egress).await {
                log_and_send(db, task_id, &tx, &format!("[WARN] Failed to apply egress rules: {e}"));
            }
        }
    }

    // Step 3: Go straight into follow-up loop
    // branch_pushed starts as true since the branch already exists on GitHub
    let mut branch_pushed = true;
    let mut preview_created = had_preview;
    follow_up_loop(config, db, task_id, short_id, &agent_name, repo, branch_name, base_branch, &mut branch_pushed, &mut preview_created, git_id, created_by, &tx).await?;

    // Step 4: Destroy agent container
    log_and_send(db, task_id, &tx, "[STEP] Destroying agent container...");
    let _ = shell::destroy_agent(config, &agent_name).await;

    update_task_status(db, task_id, "completed", None).await?;
    log_and_send(db, task_id, &tx, "[DONE] Task completed.");

    Ok(())
}

pub async fn recover_interrupted_tasks(
    config: Arc<Config>,
    db: PgPool,
    task_channels: TaskChannels,
) {
    let tasks = match sqlx::query_as::<_, Task>(
        "SELECT id, prompt, repo, base_branch, branch_name, agent_name, status, \
         preview_slug, preview_url, error_message, \
         TO_CHAR(created_at, 'YYYY-MM-DD HH24:MI:SS') as created_at, \
         TO_CHAR(updated_at, 'YYYY-MM-DD HH24:MI:SS') as updated_at, \
         parent_task_id, created_by, screenshot_url, image_url, \
         total_input_tokens, total_output_tokens, total_cost_usd, name, pr_url, pr_number, compute_seconds \
         FROM tasks WHERE status NOT IN ('completed', 'failed')",
    )
    .fetch_all(&db)
    .await
    {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("Failed to query interrupted tasks: {e}");
            return;
        }
    };

    if tasks.is_empty() {
        return;
    }
    tracing::info!("Recovering {} interrupted task(s)...", tasks.len());

    for task in tasks {
        match task.status.as_str() {
            "awaiting_followup" => {
                let Some(branch_name) = task.branch_name.clone() else {
                    let _ = update_task_status(
                        &db,
                        &task.id,
                        "failed",
                        Some("Interrupted by server restart: missing branch name"),
                    )
                    .await;
                    continue;
                };
                let Some(ref created_by) = task.created_by else {
                    let _ = update_task_status(
                        &db,
                        &task.id,
                        "failed",
                        Some("Interrupted by server restart: missing created_by"),
                    )
                    .await;
                    continue;
                };
                let git_id = match get_git_identity(&db, created_by).await {
                    Ok(g) => g,
                    Err(e) => {
                        let _ = update_task_status(
                            &db,
                            &task.id,
                            "failed",
                            Some(&format!("Interrupted by server restart: {e}")),
                        )
                        .await;
                        continue;
                    }
                };
                let short_id = task.id[..6].to_string();
                let had_preview = task.preview_url.is_some();
                let (tx, _) = broadcast::channel(1024);
                task_channels.insert(task.id.clone(), tx.clone());

                let (cfg, db2, channels) = (config.clone(), db.clone(), task_channels.clone());
                let (task_id, repo, base, created_by) = (
                    task.id.clone(),
                    task.repo.clone(),
                    task.base_branch.clone(),
                    created_by.clone(),
                );
                tokio::spawn(async move {
                    tracing::info!("Recovering awaiting_followup task {task_id}");
                    let result = run_reopen_pipeline(
                        &cfg,
                        &db2,
                        &task_id,
                        &short_id,
                        &repo,
                        &branch_name,
                        &base,
                        had_preview,
                        &git_id,
                        &created_by,
                        tx.clone(),
                    )
                    .await;
                    if let Err(e) = result {
                        let _ = update_task_status(
                            &db2,
                            &task_id,
                            "failed",
                            Some(&format!("Recovery failed: {e}")),
                        )
                        .await;
                        let _ = tx.send(format!("[ERROR] Recovery failed: {e}"));
                        if let Ok(Some(name)) = sqlx::query_scalar::<_, Option<String>>(
                            "SELECT agent_name FROM tasks WHERE id = $1"
                        )
                        .bind(&task_id)
                        .fetch_one(&db2)
                        .await
                        {
                            let _ = shell::destroy_agent(&cfg, &name).await;
                        }
                    }
                    tokio::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                        channels.remove(&task_id);
                    });
                });
            }
            "pending" => {
                let Some(ref created_by) = task.created_by else {
                    let _ = update_task_status(
                        &db,
                        &task.id,
                        "failed",
                        Some("Interrupted by server restart: missing created_by"),
                    )
                    .await;
                    continue;
                };
                let git_id = match get_git_identity(&db, created_by).await {
                    Ok(g) => g,
                    Err(e) => {
                        let _ = update_task_status(
                            &db,
                            &task.id,
                            "failed",
                            Some(&format!("Interrupted by server restart: {e}")),
                        )
                        .await;
                        continue;
                    }
                };
                let short_id = task.id[..6].to_string();
                let (tx, _) = broadcast::channel(1024);
                task_channels.insert(task.id.clone(), tx.clone());

                let (cfg, db2, channels) = (config.clone(), db.clone(), task_channels.clone());
                let (task_id, prompt, repo, base, image_url, created_by) = (
                    task.id.clone(),
                    task.prompt.clone(),
                    task.repo.clone(),
                    task.base_branch.clone(),
                    task.image_url.clone(),
                    created_by.clone(),
                );
                tokio::spawn(async move {
                    tracing::info!("Recovering pending task {task_id}");
                    let result = run_task_pipeline(
                        &cfg,
                        &db2,
                        &task_id,
                        &short_id,
                        &prompt,
                        &repo,
                        &base,
                        image_url.as_deref(),
                        &git_id,
                        None,
                        &created_by,
                        tx.clone(),
                    )
                    .await;
                    if let Err(e) = result {
                        let _ = update_task_status(
                            &db2,
                            &task_id,
                            "failed",
                            Some(&format!("Recovery failed: {e}")),
                        )
                        .await;
                        let _ = tx.send(format!("[ERROR] Recovery failed: {e}"));
                        if let Ok(Some(name)) = sqlx::query_scalar::<_, Option<String>>(
                            "SELECT agent_name FROM tasks WHERE id = $1"
                        )
                        .bind(&task_id)
                        .fetch_one(&db2)
                        .await
                        {
                            let _ = shell::destroy_agent(&cfg, &name).await;
                        }
                    }
                    tokio::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                        channels.remove(&task_id);
                    });
                });
            }
            other => {
                tracing::info!(
                    "Task {} was interrupted in state '{other}', marking failed",
                    task.id
                );
                let _ = update_task_status(
                    &db,
                    &task.id,
                    "failed",
                    Some("Interrupted by server restart. Use Reopen to continue."),
                )
                .await;
            }
        }
    }
}

// ── PR Creation ──

/// Generate a rich PR description using Claude, based on conversation history and diff.
async fn generate_pr_body(
    db: &PgPool,
    encryption_key: &str,
    github_token: &str,
    task: &Task,
    created_by: &str,
) -> Result<String, AppError> {
    // 1. Fetch conversation messages
    let messages = sqlx::query_as::<_, TaskMessage>(
        "SELECT id, task_id, sender, content, \
         TO_CHAR(created_at, 'YYYY-MM-DD HH24:MI:SS') as created_at, image_url \
         FROM task_messages WHERE task_id = $1 ORDER BY id"
    )
    .bind(&task.id)
    .fetch_all(db)
    .await
    .unwrap_or_default();

    let conversation = messages.iter()
        .filter(|m| m.sender == "claude" || m.sender == "user")
        .map(|m| format!("[{}]: {}", m.sender, m.content))
        .collect::<Vec<_>>()
        .join("\n\n");

    // 2. Fetch diff from GitHub API (file list + stats)
    let branch_name = task.branch_name.as_deref().unwrap_or("");
    let client = reqwest::Client::new();
    let diff_summary = match client
        .get(format!(
            "https://api.github.com/repos/{}/compare/{}...{}",
            task.repo, task.base_branch, branch_name
        ))
        .header("Authorization", format!("token {github_token}"))
        .header("User-Agent", "tekton-dashboard")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            let data: serde_json::Value = resp.json().await.unwrap_or_default();
            let files = data["files"].as_array();
            match files {
                Some(files) => files.iter()
                    .map(|f| {
                        let name = f["filename"].as_str().unwrap_or("?");
                        let adds = f["additions"].as_i64().unwrap_or(0);
                        let dels = f["deletions"].as_i64().unwrap_or(0);
                        let status = f["status"].as_str().unwrap_or("modified");
                        format!("- `{name}` ({status}, +{adds} -{dels})")
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
                None => String::from("(no file changes found)"),
            }
        }
        _ => String::from("(could not fetch diff)"),
    };

    // 3. Build prompt for Claude
    let context = format!(
        "You are writing a GitHub Pull Request description. Write a clear, well-structured PR description in markdown.\n\n\
         Include these sections:\n\
         - **Summary**: A concise description of what this PR does (2-4 bullet points)\n\
         - **Changes**: Brief description of the key changes made\n\
         - **Test plan**: How to verify the changes work\n\n\
         Do NOT include a title line — just the body content.\n\n\
         Here is the context:\n\n\
         **Task prompt**: {}\n\n\
         **Conversation between user and Claude**:\n{}\n\n\
         **Files changed**:\n{}\n",
        task.prompt,
        if conversation.is_empty() { "(no conversation)" } else { &conversation },
        diff_summary
    );

    // 4. Call Claude to generate the description
    let cfg = settings::get_user_ai_config(db, encryption_key, created_by)
        .await?
        .ok_or_else(|| AppError::Internal("No AI provider configured".into()))?;

    let mut env_args = vec![
        format!("ANTHROPIC_API_KEY={}", if cfg.provider == "openrouter" { "" } else { &cfg.api_key }),
        "HOME=/tmp".to_string(),
    ];
    if cfg.provider == "openrouter" {
        let model = cfg.model.as_deref().unwrap_or("anthropic/claude-sonnet-4.6");
        env_args.push(format!("ANTHROPIC_AUTH_TOKEN={}", cfg.api_key));
        env_args.push("ANTHROPIC_BASE_URL=https://openrouter.ai/api".to_string());
        env_args.push(format!("ANTHROPIC_MODEL={model}"));
    }

    let mut cmd_args = vec!["-u".to_string(), "nobody".to_string(), "env".to_string()];
    cmd_args.extend(env_args);
    let mut claude_args = vec![
        "claude".to_string(),
        "--dangerously-skip-permissions".to_string(),
    ];
    claude_args.extend(["-p".to_string(), context.clone()]);
    cmd_args.extend(claude_args);

    let output = tokio::process::Command::new("sudo")
        .args(&cmd_args)
        .output()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to run claude for PR body: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::Internal(format!(
            "Claude PR body generation failed (exit {}): {stderr}",
            output.status
        )));
    }

    let body = String::from_utf8_lossy(&output.stdout).trim().to_string();

    Ok(body)
}

pub async fn create_pr(
    user: MemberUser,
    State(state): State<crate::AppState>,
    Path(id): Path<String>,
) -> Result<Json<Task>, AppError> {
    check_task_ownership(&state.db, &id, &user.0.sub, &user.0.role).await?;

    let task = sqlx::query_as::<_, Task>(
        "SELECT id, prompt, repo, base_branch, branch_name, agent_name, status, \
         preview_slug, preview_url, error_message, \
         TO_CHAR(created_at, 'YYYY-MM-DD HH24:MI:SS') as created_at, \
         TO_CHAR(updated_at, 'YYYY-MM-DD HH24:MI:SS') as updated_at, \
         parent_task_id, created_by, screenshot_url, image_url, \
         total_input_tokens, total_output_tokens, total_cost_usd, name, pr_url, pr_number, compute_seconds \
         FROM tasks WHERE id = $1"
    )
    .bind(&id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("Task not found".into()))?;

    if task.pr_url.is_some() {
        return Err(AppError::BadRequest("PR already exists for this task".into()));
    }

    let branch_name = task.branch_name.as_deref()
        .ok_or_else(|| AppError::BadRequest("Task has no branch".into()))?;

    let git_id = get_git_identity(&state.db, &user.0.sub).await?;

    // Build PR title from task name or prompt
    let title = task.name.as_deref()
        .unwrap_or_else(|| &task.prompt)
        .chars()
        .take(72)
        .collect::<String>();

    // Build PR body: gather context and ask Claude to write the description
    let body = generate_pr_body(&state.db, &state.config.secrets_encryption_key, &git_id.token, &task, &user.0.sub).await
        .unwrap_or_else(|e| {
            tracing::warn!("Failed to generate PR body via Claude: {e}, using fallback");
            format!("## Task\n\n{}", task.prompt)
        });

    // Create PR via GitHub API
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("https://api.github.com/repos/{}/pulls", task.repo))
        .header("Authorization", format!("token {}", git_id.token))
        .header("User-Agent", "tekton-dashboard")
        .header("Accept", "application/vnd.github+json")
        .json(&serde_json::json!({
            "title": title,
            "body": body,
            "head": branch_name,
            "base": task.base_branch,
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

    let pr_data: serde_json::Value = resp.json().await
        .map_err(|e| AppError::Internal(format!("Failed to parse PR response: {e}")))?;

    let pr_url = pr_data["html_url"].as_str().unwrap_or("").to_string();
    let pr_number = pr_data["number"].as_i64().unwrap_or(0) as i32;

    sqlx::query("UPDATE tasks SET pr_url = $1, pr_number = $2, updated_at = NOW() WHERE id = $3")
        .bind(&pr_url)
        .bind(pr_number)
        .bind(&id)
        .execute(&state.db)
        .await?;

    let updated_task = sqlx::query_as::<_, Task>(
        "SELECT id, prompt, repo, base_branch, branch_name, agent_name, status, \
         preview_slug, preview_url, error_message, \
         TO_CHAR(created_at, 'YYYY-MM-DD HH24:MI:SS') as created_at, \
         TO_CHAR(updated_at, 'YYYY-MM-DD HH24:MI:SS') as updated_at, \
         parent_task_id, created_by, screenshot_url, image_url, \
         total_input_tokens, total_output_tokens, total_cost_usd, name, pr_url, pr_number, compute_seconds \
         FROM tasks WHERE id = $1"
    )
    .bind(&id)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(updated_task))
}

pub async fn link_pr(
    user: MemberUser,
    State(state): State<crate::AppState>,
    Path(id): Path<String>,
    Json(req): Json<crate::models::LinkPrRequest>,
) -> Result<Json<Task>, AppError> {
    check_task_ownership(&state.db, &id, &user.0.sub, &user.0.role).await?;
    // Extract PR number from URL (e.g. https://github.com/owner/repo/pull/123)
    let pr_number: Option<i32> = req.pr_url
        .rsplit('/')
        .next()
        .and_then(|s| s.parse().ok());

    sqlx::query("UPDATE tasks SET pr_url = $1, pr_number = $2, updated_at = NOW() WHERE id = $3")
        .bind(&req.pr_url)
        .bind(pr_number)
        .bind(&id)
        .execute(&state.db)
        .await?;

    let task = sqlx::query_as::<_, Task>(
        "SELECT id, prompt, repo, base_branch, branch_name, agent_name, status, \
         preview_slug, preview_url, error_message, \
         TO_CHAR(created_at, 'YYYY-MM-DD HH24:MI:SS') as created_at, \
         TO_CHAR(updated_at, 'YYYY-MM-DD HH24:MI:SS') as updated_at, \
         parent_task_id, created_by, screenshot_url, image_url, \
         total_input_tokens, total_output_tokens, total_cost_usd, name, pr_url, pr_number, compute_seconds \
         FROM tasks WHERE id = $1"
    )
    .bind(&id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("Task not found".into()))?;

    Ok(Json(task))
}

pub async fn get_task_logs(
    user: AuthUser,
    State(state): State<crate::AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<TaskLog>>, AppError> {
    check_task_ownership(&state.db, &id, &user.0.sub, &user.0.role).await?;
    let logs = sqlx::query_as::<_, TaskLog>(
        "SELECT id, task_id, TO_CHAR(timestamp, 'YYYY-MM-DD HH24:MI:SS') as timestamp, line \
         FROM task_logs WHERE task_id = $1 ORDER BY id ASC",
    )
    .bind(&id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(logs))
}
