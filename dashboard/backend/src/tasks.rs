use axum::extract::{Multipart, Path, Query, State};
use axum::Json;
use dashmap::DashMap;
use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::agents;
use crate::auth::{self, AuthUser, MemberUser};
use crate::config::Config;
use crate::error::AppError;
use crate::models::{
    CreateTaskRequest, ListMessagesQuery, ListTasksQuery, PaginatedTasks, SendMessageRequest, Task,
    TaskAction, TaskLog, TaskMessage, UpdateTaskNameRequest,
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
    github_org: &str,
) -> Result<(), AppError> {
    auth::check_task_ownership(db, task_id, github_login, role, github_org).await
}

/// Shared context threaded through the pipeline functions.
struct PipelineCtx<'a> {
    config: &'a Config,
    db: &'a PgPool,
    task_id: &'a str,
    short_id: &'a str,
    repo: &'a str,
    base_branch: &'a str,
    git_id: &'a GitIdentity,
}

#[derive(Clone)]
struct GitIdentity {
    token: String,
    name: String,
    email: String,
}

async fn get_git_identity(db: &PgPool, github_login: &str) -> Result<GitIdentity, AppError> {
    let row = sqlx::query_as::<_, (String, String, Option<String>)>(
        "SELECT github_token, name, email FROM users WHERE github_login = $1",
    )
    .bind(github_login)
    .fetch_optional(db)
    .await?
    .ok_or_else(|| AppError::Auth(format!("User '{github_login}' not found in database")))?;

    let email = row
        .2
        .unwrap_or_else(|| format!("{github_login}@users.noreply.github.com"));

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
    let per_page = params.per_page.unwrap_or(50).clamp(1, 200);
    let offset = ((page - 1) * per_page) as i64;
    let limit = per_page as i64;

    // Build dynamic WHERE clause
    let mut conditions: Vec<String> = Vec::new();
    let mut bind_idx = 1u32;

    // We'll collect bind values as strings for the dynamic query
    let mut bind_values: Vec<String> = Vec::new();

    // Non-admin users see tasks they created OR tasks from repos they have access to
    if user.0.role != "admin" {
        let org_prefix = format!("{}/%", state.config.github_org);
        if user.0.role == "member" {
            // Members can see tasks from any repo in the org, plus repos explicitly granted
            conditions.push(format!(
                "(created_by = ${bind_idx} OR repo LIKE ${} OR repo IN \
                 (SELECT repo FROM user_repo_permissions WHERE github_login = ${}))",
                bind_idx + 1,
                bind_idx + 2,
            ));
            bind_values.push(user.0.sub.clone());
            bind_values.push(org_prefix);
            bind_values.push(user.0.sub.clone());
            bind_idx += 3;
        } else {
            // Viewers only see tasks from repos explicitly granted
            conditions.push(format!(
                "(created_by = ${bind_idx} OR repo IN \
                 (SELECT repo FROM user_repo_permissions WHERE github_login = ${}))",
                bind_idx + 1,
            ));
            bind_values.push(user.0.sub.clone());
            bind_values.push(user.0.sub.clone());
            bind_idx += 2;
        }
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
         TO_CHAR(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as created_at, \
         TO_CHAR(updated_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as updated_at, \
         parent_task_id, created_by, screenshot_url, image_url, \
         total_input_tokens, total_output_tokens, total_cost_usd, name, pr_url, pr_number, compute_seconds, \
         intake_issue_id, source_type, \
         TO_CHAR(agent_retained_until, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as agent_retained_until \
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
    check_task_ownership(
        &state.db,
        &id,
        &user.0.sub,
        &user.0.role,
        &state.config.github_org,
    )
    .await?;
    let task = sqlx::query_as::<_, Task>(
        "SELECT id, prompt, repo, base_branch, branch_name, agent_name, status, \
         preview_slug, preview_url, error_message, \
         TO_CHAR(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as created_at, \
         TO_CHAR(updated_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as updated_at, \
         parent_task_id, created_by, screenshot_url, image_url, \
         total_input_tokens, total_output_tokens, total_cost_usd, name, pr_url, pr_number, compute_seconds, \
         intake_issue_id, source_type, \
         TO_CHAR(agent_retained_until, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as agent_retained_until \
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
    check_task_ownership(
        &state.db,
        &id,
        &user.0.sub,
        &user.0.role,
        &state.config.github_org,
    )
    .await?;

    // Verify parent exists
    let _ = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM tasks WHERE id = $1")
        .bind(&id)
        .fetch_one(&state.db)
        .await?;

    let subtasks = sqlx::query_as::<_, Task>(
        "SELECT id, prompt, repo, base_branch, branch_name, agent_name, status, \
         preview_slug, preview_url, error_message, \
         TO_CHAR(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as created_at, \
         TO_CHAR(updated_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as updated_at, \
         parent_task_id, created_by, screenshot_url, image_url, \
         total_input_tokens, total_output_tokens, total_cost_usd, name, pr_url, pr_number, compute_seconds, \
         intake_issue_id, source_type, \
         TO_CHAR(agent_retained_until, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as agent_retained_until \
         FROM tasks WHERE parent_task_id = $1 ORDER BY created_at ASC"
    )
    .bind(&id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(subtasks))
}

pub(crate) struct SpawnTaskParams {
    pub prompt: String,
    pub repo: String,
    pub base_branch: String,
    pub created_by: String,
    pub source_type: Option<String>,
    pub intake_issue_id: Option<i64>,
}

/// Internal task spawning for both HTTP handler and intake daemon.
/// Returns the created Task.
pub(crate) async fn spawn_task_internal(
    config: Arc<Config>,
    db: PgPool,
    task_channels: TaskChannels,
    params: SpawnTaskParams,
) -> Result<Task, AppError> {
    let SpawnTaskParams {
        prompt,
        repo,
        base_branch,
        created_by,
        source_type,
        intake_issue_id,
    } = params;
    let id = Uuid::new_v4().to_string();
    let short_id = id[..6].to_string();

    sqlx::query(
        "INSERT INTO tasks (id, prompt, repo, base_branch, status, created_by, source_type, intake_issue_id) \
         VALUES ($1, $2, $3, $4, 'pending', $5, $6, $7)",
    )
    .bind(&id)
    .bind(&prompt)
    .bind(&repo)
    .bind(&base_branch)
    .bind(&created_by)
    .bind(&source_type)
    .bind(intake_issue_id)
    .execute(&db)
    .await?;

    record_state_transition(&db, &id, None, "pending").await;

    crate::audit::log_event(
        &db,
        "task.created",
        &created_by,
        Some(&id),
        serde_json::json!({ "repo": &repo, "created_by": &created_by, "source_type": &source_type }),
        None,
    )
    .await;

    let task = sqlx::query_as::<_, Task>(
        "SELECT id, prompt, repo, base_branch, branch_name, agent_name, status, \
         preview_slug, preview_url, error_message, \
         TO_CHAR(created_at, 'YYYY-MM-DD HH24:MI:SS') as created_at, \
         TO_CHAR(updated_at, 'YYYY-MM-DD HH24:MI:SS') as updated_at, \
         parent_task_id, created_by, screenshot_url, image_url, \
         total_input_tokens, total_output_tokens, total_cost_usd, name, pr_url, pr_number, compute_seconds, \
         intake_issue_id, source_type, \
         TO_CHAR(agent_retained_until, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as agent_retained_until \
         FROM tasks WHERE id = $1"
    )
    .bind(&id)
    .fetch_one(&db)
    .await?;

    let git_id = get_git_identity(&db, &created_by).await?;

    // Intake sources with auto_create_pr off never get a preview, so don't advertise one.
    if intake_auto_create_pr(&db, &id).await? != Some(false) {
        let preview_slug = format!("t-{short_id}");
        let preview_url = format!("https://{preview_slug}.{}", config.preview_domain);
        sqlx::query("UPDATE tasks SET preview_slug = $1, preview_url = $2 WHERE id = $3")
            .bind(&preview_slug)
            .bind(&preview_url)
            .bind(&id)
            .execute(&db)
            .await?;
    }

    let (tx, _) = broadcast::channel(1024);
    task_channels.insert(id.clone(), tx.clone());

    let task_id = id.clone();
    let channels = task_channels.clone();

    tokio::spawn(async move {
        let ctx = PipelineCtx {
            config: &config,
            db: &db,
            task_id: &task_id,
            short_id: &short_id,
            repo: &repo,
            base_branch: &base_branch,
            git_id: &git_id,
        };
        let result = run_task_pipeline(&ctx, &prompt, None, None, &created_by, tx.clone()).await;

        if let Err(e) = &result {
            agents::fail_task(&config, &db, &task_id, &e.to_string()).await;
            let _ = tx.send(format!("[ERROR] Task failed: {e}"));
        }

        let channels2 = channels;
        let tid = task_id.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            channels2.remove(&tid);
        });
    });

    Ok(task)
}

pub async fn create_task(
    user: MemberUser,
    State(state): State<crate::AppState>,
    Json(req): Json<CreateTaskRequest>,
) -> Result<Json<Task>, AppError> {
    // Validate repo is allowed
    if !state.config.allowed_repos.is_empty() && !state.config.allowed_repos.contains(&req.repo) {
        return Err(AppError::BadRequest(format!(
            "Repo '{}' is not in the allowed list",
            req.repo
        )));
    }

    // Check per-user repo permission
    check_repo_permission(
        &state.db,
        &user.0.sub,
        &req.repo,
        &user.0.role,
        &state.config.github_org,
    )
    .await?;

    // Check budget limits before creating the task
    crate::cost::check_budget(&state.db, &user.0.sub, &state.config.github_org).await?;

    // Validate parent_task_id if provided
    if let Some(ref parent_id) = req.parent_task_id {
        let parent_exists =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM tasks WHERE id = $1")
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
         TO_CHAR(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as created_at, \
         TO_CHAR(updated_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as updated_at, \
         parent_task_id, created_by, screenshot_url, image_url, \
         total_input_tokens, total_output_tokens, total_cost_usd, name, pr_url, pr_number, compute_seconds, \
         intake_issue_id, source_type, \
         TO_CHAR(agent_retained_until, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as agent_retained_until \
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
        let ctx = PipelineCtx {
            config: &config,
            db: &db,
            task_id: &task_id,
            short_id: &short,
            repo: &repo,
            base_branch: &base,
            git_id: &git_id,
        };
        let result = run_task_pipeline(
            &ctx,
            &prompt,
            image_url_json2.as_deref(),
            custom_branch,
            &created_by,
            tx.clone(),
        )
        .await;

        if let Err(e) = &result {
            agents::fail_task(&config, &db, &task_id, &e.to_string()).await;
            let _ = tx.send(format!("[ERROR] Task failed: {e}"));
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
    ctx: &PipelineCtx<'_>,
    prompt: &str,
    image_url_json: Option<&str>,
    custom_branch_name: Option<String>,
    created_by: &str,
    tx: broadcast::Sender<String>,
) -> Result<(), AppError> {
    let &PipelineCtx {
        config,
        db,
        task_id,
        short_id,
        repo,
        base_branch,
        git_id,
    } = ctx;
    // Generate a short name for the task using Claude
    let task_name =
        match generate_task_name(db, &config.secrets_encryption_key, created_by, prompt).await {
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
        Some(ref name) if !name.is_empty() => slugify_for_branch(name),
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
            db,
            task_id,
            &tx,
            &format!(
                "[POLICY] Loaded policy for {repo}: {} protected branch(es)",
                pol.protected_branches.len()
            ),
        );
    }

    // Record pipeline start time for compute_seconds tracking
    let pipeline_start = std::time::Instant::now();

    // Step 1: Create agent container
    let agent_name = format!("task-{short_id}");
    update_task_status(db, task_id, "creating_agent", None).await?;
    log_and_send(
        db,
        task_id,
        &tx,
        &format!("[STEP] Creating agent container '{agent_name}'..."),
    );
    let create_start = std::time::Instant::now();
    shell::create_agent(config, &agent_name).await?;
    let create_ms = create_start.elapsed().as_millis();
    log_and_send(
        db,
        task_id,
        &tx,
        &format!("[STEP] Agent '{agent_name}' created in {create_ms}ms"),
    );
    update_task_field(db, task_id, "agent_name", &agent_name).await?;

    // Step 2: Clone repo and create branch
    update_task_status(db, task_id, "cloning", None).await?;
    log_and_send(
        db,
        task_id,
        &tx,
        &format!("[STEP] Cloning {repo} and creating branch {branch_name}..."),
    );

    let clone_url = format!(
        "https://x-access-token:{}@github.com/{repo}.git",
        git_id.token
    );

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
    log_and_send(
        db,
        task_id,
        &tx,
        &format!("[STEP] Pushed branch '{branch_name}' to GitHub"),
    );

    let auto_create_pr = intake_auto_create_pr(db, task_id).await?;
    let deliver = auto_create_pr != Some(false);

    // Prewarm preview: create it on the feature branch so it's already on the right
    // branch when the agent finishes. Runs in background while Claude works.
    if deliver {
        let config2 = config.clone();
        let repo2 = repo.to_string();
        let branch2 = branch_name.clone();
        let slug = format!("t-{short_id}");
        let token = git_id.token.clone();
        tokio::spawn(async move {
            if let Err(e) =
                shell::create_preview(&config2, &repo2, &branch2, Some(&slug), &token).await
            {
                tracing::warn!("Preview prewarm failed for {slug}: {e}");
            }
        });
    }

    // Step 2b: Load and inject secrets
    let repo_secrets =
        secrets::load_secrets_for_repo(db, &config.secrets_encryption_key, repo).await?;
    if !repo_secrets.is_empty() {
        log_and_send(
            db,
            task_id,
            &tx,
            &format!("[STEP] Injecting {} secret(s)...", repo_secrets.len()),
        );
        write_secrets_env_file(&agent_name, &repo_secrets, tx.clone()).await?;
    }

    // Step 2c: Apply network egress restrictions (if any)
    if let Some(ref pol) = policy {
        if let Some(ref egress) = pol.network_egress {
            log_and_send(
                db,
                task_id,
                &tx,
                "[POLICY] Applying network egress restrictions...",
            );
            if let Err(e) = shell::apply_egress_rules(&agent_name, egress).await {
                log_and_send(
                    db,
                    task_id,
                    &tx,
                    &format!("[WARN] Failed to apply egress rules: {e}"),
                );
            }
        }
    }

    // Step 3: Copy images and run Claude (streaming)
    let mut effective_prompt =
        augment_prompt_with_images(config, &agent_name, image_url_json, prompt, &tx).await?;

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
                    constraints.push(format!(
                        "ALLOWED TOOLS (use ONLY these): {}",
                        names.join(", ")
                    ));
                }
            }
        }
        if !constraints.is_empty() {
            let policy_block = constraints.join("\n");
            effective_prompt =
                format!("REPO POLICY CONSTRAINTS:\n{policy_block}\n\n{effective_prompt}");
        }
    }

    update_task_status(db, task_id, "running_claude", None).await?;
    // Log which tools are blocked before running Claude so it's visible in the agent logs
    if let Some(ref pol) = policy {
        let denied = get_denied_tools(pol);
        if !denied.is_empty() {
            log_and_send(
                db,
                task_id,
                &tx,
                &format!(
                    "[POLICY] Blocked tools: {}. Claude will not have access to these.",
                    denied.join(", ")
                ),
            );
        }
    }
    log_and_send(db, task_id, &tx, "[STEP] Running Claude...");
    let run = run_claude_streaming(
        db,
        &config.secrets_encryption_key,
        created_by,
        &agent_name,
        &effective_prompt,
        tx.clone(),
        policy.as_ref(),
    )
    .await;
    if let Err(e) = record_claude_run(db, task_id, run, policy.as_ref(), &tx).await {
        // If a policy with denied tools is active, the CLI may exit non-zero because
        // it cannot fulfil the request. Don't fail the task — notify the user and
        // fall through to the follow-up loop so they can try a different prompt.
        let denied = policy.as_ref().map(get_denied_tools).unwrap_or_default();
        if denied.is_empty() {
            return Err(e);
        }
        let msg = format!(
            "Claude could not complete the request — the repo policy blocks these tools: {}.",
            denied.join(", ")
        );
        log_and_send(db, task_id, &tx, &format!("[POLICY] {msg}"));
        save_system_message(db, task_id, &msg).await?;
    }

    // Check cost limit after initial run — just log a warning, follow-ups will be
    // individually rejected by the check inside the follow-up loop.
    if let Some(ref pol) = policy {
        if let Some(max_cost) = pol.max_cost_usd {
            if check_cost_limit(db, task_id, max_cost, &tx).await? {
                save_system_message(
                    db,
                    task_id,
                    "Cost limit reached — further follow-ups will be rejected by policy.",
                )
                .await?;
            }
        }
    }

    // Step 3b: Commit any changes (if Claude asked a question and made none, that's fine —
    // the user will see the question in chat and respond via the follow-up loop)
    if deliver {
        let _ = commit_changes_in_agent(&agent_name, prompt, tx.clone()).await?;
    }

    // Step 3c: Push changes and update preview
    let mut preview_created = deliver;
    let mut branch_pushed = false;
    push_or_hold(
        ctx,
        &agent_name,
        &branch_name,
        &mut branch_pushed,
        &mut preview_created,
        created_by,
        &tx,
        None,
    )
    .await?;

    // Step 4: Follow-up loop
    let outcome = follow_up_loop(
        ctx,
        &agent_name,
        &branch_name,
        &mut branch_pushed,
        &mut preview_created,
        created_by,
        &tx,
        0,
    )
    .await?;

    // Step 5: Destroy agent container
    let completed = finish_task(ctx, &agent_name, outcome, &tx).await?;

    // Record compute time (wall-clock seconds from agent creation to destruction)
    let compute_secs = pipeline_start.elapsed().as_secs() as i32;
    let _ = sqlx::query("UPDATE tasks SET compute_seconds = $1 WHERE id = $2")
        .bind(compute_secs)
        .bind(task_id)
        .execute(db)
        .await;

    if !completed {
        return Ok(());
    }
    if preview_created {
        let preview_url = format!("https://t-{short_id}.{}", config.preview_domain);
        log_and_send(
            db,
            task_id,
            &tx,
            &format!("[DONE] Preview available at {preview_url}"),
        );
    } else {
        log_and_send(db, task_id, &tx, "[DONE] Task completed.");
    }

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
            let model = cfg
                .model
                .as_deref()
                .unwrap_or("anthropic/claude-sonnet-4.6");
            Ok((
                format!(
                    "export ANTHROPIC_BASE_URL='https://openrouter.ai/api' ANTHROPIC_AUTH_TOKEN='{}' ANTHROPIC_API_KEY='' ANTHROPIC_MODEL='{}'",
                    cfg.api_key, model
                ),
                String::new(),
            ))
        }
        Some(cfg) if cfg.provider == "anthropic-oauth" => Ok((
            format!(
                "export CLAUDE_CODE_OAUTH_TOKEN='{}' ANTHROPIC_API_KEY=''",
                cfg.api_key
            ),
            String::new(),
        )),
        Some(cfg) => Ok((
            format!("export ANTHROPIC_API_KEY='{}'", cfg.api_key),
            String::new(),
        )),
        None => {
            // Fallback to the global admin-configured key
            match settings::get_global_ai_config(db, encryption_key).await? {
                Some(cfg) if cfg.provider == "openrouter" => {
                    let model = cfg
                        .model
                        .as_deref()
                        .unwrap_or("anthropic/claude-sonnet-4.6");
                    Ok((
                        format!(
                            "export ANTHROPIC_BASE_URL='https://openrouter.ai/api' ANTHROPIC_AUTH_TOKEN='{}' ANTHROPIC_API_KEY='' ANTHROPIC_MODEL='{}'",
                            cfg.api_key, model
                        ),
                        String::new(),
                    ))
                }
                Some(cfg) if cfg.provider == "anthropic-oauth" => Ok((
                    format!(
                "export CLAUDE_CODE_OAUTH_TOKEN='{}' ANTHROPIC_API_KEY=''",
                cfg.api_key
            ),
                    String::new(),
                )),
                Some(cfg) => Ok((
                    format!("export ANTHROPIC_API_KEY='{}'", cfg.api_key),
                    String::new(),
                )),
                None => Err(AppError::Internal(
                    "No AI provider configured. Go to Settings → AI Provider to connect your account, or ask an admin to set up a global API key."
                        .into(),
                )),
            }
        }
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
            AppError::Internal("No AI provider configured for task name generation".into())
        })?;

    let naming_prompt = format!(
        "Generate a very short name (3-5 words, no quotes, no punctuation) that summarizes this coding task. \
         Reply with ONLY the name, nothing else.\n\nTask: {}", prompt
    );

    let mut env_args = vec![
        format!(
            "ANTHROPIC_API_KEY={}",
            if cfg.provider == "openrouter" {
                ""
            } else {
                &cfg.api_key
            }
        ),
        "HOME=/tmp".to_string(),
    ];
    if cfg.provider == "openrouter" {
        let model = cfg
            .model
            .as_deref()
            .unwrap_or("anthropic/claude-sonnet-4.6");
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
    let name = if name.len() > 60 {
        name[..60].to_string()
    } else {
        name
    };
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
        deny.iter()
            .filter_map(|v| v.as_str())
            .map(String::from)
            .collect()
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
) -> shell::ClaudeRun {
    let (auth_env, model_flag) = build_claude_auth_env(db, encryption_key, created_by)
        .await
        .map_err(shell::ClaudeStreamError::before_start)?;
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
) -> shell::ClaudeRun {
    let (auth_env, model_flag) = build_claude_auth_env(db, encryption_key, created_by)
        .await
        .map_err(shell::ClaudeStreamError::before_start)?;
    let disallowed_flag = build_disallowed_tools_flag(policy);
    let escaped_prompt = prompt.replace('\'', "'\\''");
    let claude_cmd = format!(
        "source /home/agent/.env.sh 2>/dev/null ; {auth_env} && cd /home/agent/repo && \
         claude --dangerously-skip-permissions --output-format stream-json --verbose \
         {model_flag} {disallowed_flag} --continue -p '{escaped_prompt}'"
    );
    shell::agent_exec_claude_streaming(agent_name, &claude_cmd, tx).await
}

/// Record one Claude invocation — its actions, policy violations, token usage and chat
/// text — whether it succeeded or failed, so usage streamed before a failure isn't lost.
/// Call exactly once per invocation. Returns the invocation's error, if any.
async fn record_claude_run(
    db: &PgPool,
    task_id: &str,
    run: shell::ClaudeRun,
    policy: Option<&crate::models::RepoPolicy>,
    tx: &broadcast::Sender<String>,
) -> Result<(), AppError> {
    let (output, error) = match run {
        Ok(output) => (output, None),
        Err(e) => (e.partial, Some(e.error)),
    };
    persist_actions(db, task_id, &output.actions).await;
    if let Some(pol) = policy {
        check_policy_violations(db, task_id, &output.actions, pol, tx).await;
    }
    increment_token_usage(db, task_id, &output.usage).await;
    let saved = save_claude_response_as_message(db, task_id, &output.text).await;
    match error {
        None => saved,
        Some(e) => {
            if let Err(save_err) = saved {
                tracing::warn!("Failed to save partial Claude response for {task_id}: {save_err}");
            }
            Err(e)
        }
    }
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
    sqlx::query("INSERT INTO task_messages (task_id, sender, content) VALUES ($1, 'claude', $2)")
        .bind(task_id)
        .bind(text)
        .execute(db)
        .await?;
    Ok(())
}

/// Save a system/status message to the chat so it appears inline in the UI.
pub(crate) async fn save_system_message(
    db: &PgPool,
    task_id: &str,
    content: &str,
) -> Result<(), AppError> {
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
             VALUES ($1, $2, $3, $4, $5)",
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
         WHERE id = $4",
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
    let cost: f64 =
        sqlx::query_scalar("SELECT COALESCE(total_cost_usd, 0) FROM tasks WHERE id = $1")
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
        "INSERT INTO task_state_transitions (task_id, from_status, to_status) VALUES ($1, $2, $3)",
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

    let _ = tx.send(format!(
        "[STEP] Copying {} image(s) to agent container...",
        urls.len()
    ));

    // Create uploads dir in agent
    if let Err(e) = shell::agent_exec(agent_name, "mkdir -p /home/agent/uploads", tx.clone()).await
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
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            "UserKnownHostsFile=/dev/null",
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
    let commit_cmd =
        format!("cd /home/agent/repo && git add -A && git commit -m 'Claude: {escaped_prompt}'");
    shell::agent_exec(agent_name, &commit_cmd, tx).await?;
    Ok(true)
}

/// Sent as a chat message to push the agent's current changes again without running Claude.
const RETRY_PUSH: &str = "__retry_push__";

#[derive(PartialEq)]
enum FollowUpOutcome {
    Done,
    /// The task ended while its last push was still failing, so the work never reached GitHub.
    Undelivered(String),
}

#[allow(clippy::too_many_arguments)]
async fn follow_up_loop(
    ctx: &PipelineCtx<'_>,
    agent_name: &str,
    branch_name: &str,
    branch_pushed: &mut bool,
    preview_created: &mut bool,
    created_by: &str,
    tx: &broadcast::Sender<String>,
    initial_last_seen_id: i64,
) -> Result<FollowUpOutcome, AppError> {
    let &PipelineCtx {
        config,
        db,
        task_id,
        short_id: _,
        repo,
        base_branch: _,
        git_id: _,
    } = ctx;
    let mut last_seen_id: i64 = initial_last_seen_id;

    // Track conversation history for context (used as fallback if --continue fails)
    let mut conversation_history: Vec<String> = Vec::new();

    // If the task has a preview, inject a preview-logs helper script into the agent container
    // so Claude can fetch live preview output on demand during follow-ups.
    let preview_slug: Option<String> =
        sqlx::query_scalar("SELECT preview_slug FROM tasks WHERE id = $1")
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

    // While the last push is failing the task stays push_failed, until a push succeeds or the
    // container's retention runs out.
    let mut push_failed_deadline = load_push_failed_deadline(db, task_id).await?;
    if push_failed_deadline.is_none() {
        update_task_status(db, task_id, "awaiting_followup", None).await?;
    }
    let _ = tx.send(
        "[STATUS] Waiting for follow-up messages (click 'Mark Done' to finish)...".to_string(),
    );

    loop {
        // Check for new messages IMMEDIATELY (no sleep before first check)
        // Filter out claude's own messages so we only process user messages
        let new_messages: Vec<TaskMessage> = sqlx::query_as::<_, TaskMessage>(
            "SELECT id, task_id, sender, content, \
             TO_CHAR(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as created_at, image_url \
             FROM task_messages \
             WHERE task_id = $1 AND id > $2 AND sender NOT IN ('claude', 'system') \
             ORDER BY id ASC",
        )
        .bind(task_id)
        .bind(last_seen_id)
        .fetch_all(db)
        .await?;

        if new_messages.is_empty() {
            if push_failed_deadline.is_some_and(|d| std::time::Instant::now() >= d) {
                return Ok(FollowUpOutcome::Undelivered(
                    "The push kept failing until the container's retention ran out, so the \
                     changes were not delivered."
                        .into(),
                ));
            }
            // Sleep 3s before checking again (reduced from 10s)
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            continue;
        }

        // Update last seen to latest message id
        last_seen_id = new_messages.last().unwrap().id;

        // Check for __done__ in any message
        if new_messages.iter().any(|m| m.content.trim() == "__done__") {
            let _ = tx.send("[STATUS] Received '__done__', finishing up.".to_string());
            if push_failed_deadline.is_some() {
                return Ok(FollowUpOutcome::Undelivered(
                    "Marked done while the last push was still failing, so the changes were \
                     not delivered."
                        .into(),
                ));
            }
            return Ok(FollowUpOutcome::Done);
        }

        // A follow-up message pushes anyway, so retry requests only matter on their own.
        let new_messages: Vec<TaskMessage> = new_messages
            .into_iter()
            .filter(|m| m.content.trim() != RETRY_PUSH)
            .collect();
        if new_messages.is_empty() {
            let _ = tx.send("[STATUS] Retrying push...".to_string());
            push_failed_deadline = push_or_hold(
                ctx,
                agent_name,
                branch_name,
                branch_pushed,
                preview_created,
                created_by,
                tx,
                push_failed_deadline,
            )
            .await?;
            continue;
        }

        // Reload policy from DB each iteration so changes (add/remove/edit) take effect immediately
        let policy = policies::load_effective_policy(db, repo).await?;

        // Batch all pending messages into one prompt
        let mut combined_parts: Vec<String> = Vec::new();
        for msg in &new_messages {
            let effective = augment_prompt_with_images(
                config,
                agent_name,
                msg.image_url.as_deref(),
                &msg.content,
                tx,
            )
            .await?;
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
                    save_system_message(
                        db,
                        task_id,
                        "Cost limit reached — this follow-up was blocked by policy.",
                    )
                    .await?;
                    continue;
                }
            }
        }

        // Run Claude with --continue to maintain conversation context
        save_system_message(db, task_id, "Running Claude with your follow-up...").await?;
        update_task_status(db, task_id, "running_claude", None).await?;
        let _ = tx.send("[FOLLOWUP] Running Claude with follow-up...".to_string());

        let run = run_claude_continue(
            db,
            &config.secrets_encryption_key,
            created_by,
            agent_name,
            &combined_prompt,
            tx.clone(),
            policy.as_ref(),
        )
        .await;
        if let Err(e) = record_claude_run(db, task_id, run, policy.as_ref(), tx).await {
            // Fallback: if --continue fails, use fresh session with full context.
            // The failed attempt's usage was already recorded above.
            let _ = tx.send(format!(
                "[WARN] --continue failed ({e}), falling back to fresh session..."
            ));
            let original_prompt =
                sqlx::query_scalar::<_, String>("SELECT prompt FROM tasks WHERE id = $1")
                    .bind(task_id)
                    .fetch_one(db)
                    .await?;
            let context_prompt =
                build_followup_prompt(&original_prompt, &conversation_history, &combined_prompt);
            let run = run_claude_streaming(
                db,
                &config.secrets_encryption_key,
                created_by,
                agent_name,
                &context_prompt,
                tx.clone(),
                policy.as_ref(),
            )
            .await;
            record_claude_run(db, task_id, run, policy.as_ref(), tx).await?;
        }

        if should_deliver(db, task_id).await? {
            let _ = commit_changes_in_agent(agent_name, &combined_parts[0], tx.clone()).await?;
        }

        // Push and update preview
        push_failed_deadline = push_or_hold(
            ctx,
            agent_name,
            branch_name,
            branch_pushed,
            preview_created,
            created_by,
            tx,
            push_failed_deadline,
        )
        .await?;
        let _ = tx.send("[STATUS] Waiting for more follow-up messages...".to_string());
        // Loop immediately — check for more messages right away (no sleep before next check)
    }
}

/// When the task is push_failed, the instant its container's retention runs out.
async fn load_push_failed_deadline(
    db: &PgPool,
    task_id: &str,
) -> Result<Option<std::time::Instant>, AppError> {
    let remaining: Option<Option<f64>> = sqlx::query_scalar(
        "SELECT EXTRACT(EPOCH FROM agent_retained_until - NOW())::FLOAT8 \
         FROM tasks WHERE id = $1 AND status = 'push_failed'",
    )
    .bind(task_id)
    .fetch_optional(db)
    .await?;
    Ok(remaining.map(|secs| {
        std::time::Instant::now() + std::time::Duration::from_secs_f64(secs.unwrap_or(0.0).max(0.0))
    }))
}

/// Push the agent's changes. A failed push doesn't end the task: the error goes to the chat,
/// the task moves to push_failed and its container is kept, so a retry or a follow-up can
/// deliver the work. Returns the push_failed deadline, which is None once a push succeeds.
#[allow(clippy::too_many_arguments)]
async fn push_or_hold(
    ctx: &PipelineCtx<'_>,
    agent_name: &str,
    branch_name: &str,
    branch_pushed: &mut bool,
    preview_created: &mut bool,
    created_by: &str,
    tx: &broadcast::Sender<String>,
    push_failed_deadline: Option<std::time::Instant>,
) -> Result<Option<std::time::Instant>, AppError> {
    let &PipelineCtx {
        config,
        db,
        task_id,
        ..
    } = ctx;
    match push_and_preview(
        ctx,
        agent_name,
        branch_name,
        branch_pushed,
        preview_created,
        tx,
    )
    .await
    {
        Ok(outcome) => {
            if outcome.branch_has_commits() {
                save_system_message(db, task_id, "Changes pushed and preview updated ✓").await?;
                maybe_auto_open_pr(ctx, created_by, tx).await;
            }
            if push_failed_deadline.is_some() {
                agents::clear_retention(db, task_id).await;
            }
            update_task_status(db, task_id, "awaiting_followup", None).await?;
            Ok(None)
        }
        Err(e) => {
            log_and_send(db, task_id, tx, &format!("[ERROR] Push failed: {e}"));
            let hours = config.agent_retention_push_failed_hours;
            save_system_message(
                db,
                task_id,
                &format!(
                    "Push failed: {e}\n\nThe agent's changes are still in its container, kept \
                     for {hours}h. Click Retry push or send a follow-up to try again."
                ),
            )
            .await?;
            update_task_status(db, task_id, "push_failed", Some(&e.to_string())).await?;
            // The retention window counts from the first failure, not the latest retry.
            if let Some(deadline) = push_failed_deadline {
                return Ok(Some(deadline));
            }
            agents::retain(db, task_id, hours).await;
            Ok(Some(
                std::time::Instant::now() + std::time::Duration::from_secs(hours as u64 * 3600),
            ))
        }
    }
}

/// Tear down after the follow-up loop: snapshot and destroy the container, then record the
/// final status. Returns whether the task completed.
async fn finish_task(
    ctx: &PipelineCtx<'_>,
    agent_name: &str,
    outcome: FollowUpOutcome,
    tx: &broadcast::Sender<String>,
) -> Result<bool, AppError> {
    let &PipelineCtx {
        config,
        db,
        task_id,
        base_branch,
        ..
    } = ctx;
    log_and_send(db, task_id, tx, "[STEP] Destroying agent container...");
    agents::snapshot_and_destroy(config, db, task_id, agent_name, base_branch).await;
    match outcome {
        FollowUpOutcome::Done => {
            update_task_status(db, task_id, "completed", None).await?;
            Ok(true)
        }
        FollowUpOutcome::Undelivered(reason) => {
            let msg = format!("{reason} A snapshot of the agent's work was saved on the server.");
            save_system_message(db, task_id, &msg).await?;
            update_task_status(db, task_id, "failed", Some(&msg)).await?;
            log_and_send(db, task_id, tx, &format!("[DONE] {msg}"));
            Ok(false)
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

/// Collect all file changes in the agent repo relative to a base ref.
///
/// Why Python: This function needs to run a multi-step operation (stage files, commit,
/// diff, read file contents, base64-encode them, produce JSON) inside a *remote* agent
/// container over SSH. We can't use Rust libraries like `git2` because the git repo
/// lives on the remote machine, not locally. We need to send a self-contained script
/// over SSH that does everything in one shot. Python is a good fit here because it has
/// `subprocess`, `base64`, `json`, and file I/O in its stdlib, making the script
/// compact and readable compared to an equivalent bash version. The script source lives
/// in `scripts/collect_file_changes.py` and is embedded at compile time.
async fn collect_file_changes(agent_name: &str, base_ref: &str) -> Result<FileChanges, AppError> {
    // Sanitize base_ref (should be like "origin/main" — never contains quotes)
    let safe_base_ref = base_ref.replace(['\'', '"'], "");
    let python_script =
        include_str!("../scripts/collect_file_changes.py").replace("__BASE_REF__", &safe_base_ref);

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

    Err(AppError::Internal(last_err.unwrap_or_else(|| {
        format!("{operation} failed after retries")
    })))
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
        let hint = graphql_error_hint(errors, file_changes);
        return Err(AppError::Internal(format!(
            "GraphQL errors: {errors}{hint}"
        )));
    }

    let oid = resp_json
        .pointer("/data/createCommitOnBranch/commit/oid")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::Internal("Missing commit OID in GraphQL response".into()))?
        .to_string();

    Ok(oid)
}

/// GitHub reports a FORBIDDEN createCommitOnBranch without saying which permission is missing;
/// name the usual suspects.
fn graphql_error_hint(errors: &serde_json::Value, file_changes: &FileChanges) -> &'static str {
    let forbidden = errors.as_array().is_some_and(|errs| {
        errs.iter()
            .any(|e| e.get("type").and_then(|t| t.as_str()) == Some("FORBIDDEN"))
    });
    if !forbidden {
        return "";
    }
    let touches_workflows = file_changes
        .additions
        .iter()
        .map(|a| &a.path)
        .chain(file_changes.deletions.iter().map(|d| &d.path))
        .any(|p| p.starts_with(".github/workflows/"));
    if touches_workflows {
        "\nHint: this push changes files under .github/workflows/, which needs a token with the \
         `workflow` scope (or Workflows write permission)."
    } else {
        "\nHint: FORBIDDEN usually means the token lacks Contents write permission on this \
         repository."
    }
}

/// `None` for manual tasks; `Some(auto_create_pr)` for intake tasks. An intake task whose
/// source was deleted resolves to `Some(false)` so it never starts pushing.
fn resolve_auto_create_pr(source_type: Option<String>, source_flag: Option<bool>) -> Option<bool> {
    match source_type {
        Some(t) if t.starts_with("intake_") => Some(source_flag.unwrap_or(false)),
        _ => None,
    }
}

async fn intake_auto_create_pr(db: &PgPool, task_id: &str) -> Result<Option<bool>, AppError> {
    Ok(sqlx::query_as::<_, (Option<String>, Option<bool>)>(
        "SELECT t.source_type, s.auto_create_pr FROM tasks t \
         LEFT JOIN intake_issues i ON i.id = t.intake_issue_id \
         LEFT JOIN intake_sources s ON s.id = i.source_id \
         WHERE t.id = $1",
    )
    .bind(task_id)
    .fetch_optional(db)
    .await?
    .and_then(|(source_type, flag)| resolve_auto_create_pr(source_type, flag)))
}

/// Whether the pipeline commits, pushes and previews Claude's changes for this task.
async fn should_deliver(db: &PgPool, task_id: &str) -> Result<bool, AppError> {
    Ok(intake_auto_create_pr(db, task_id).await? != Some(false))
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

/// Fetch the task branch from GitHub into the agent repo and return its head commit,
/// or `None` if it couldn't be fetched (e.g. the branch doesn't exist yet).
async fn fetch_remote_branch(agent_name: &str, branch_name: &str) -> Option<String> {
    let escaped = branch_name.replace('\'', "'\\''");
    let cmd = format!(
        "cd /home/agent/repo && git fetch -q origin '+{escaped}:refs/remotes/origin/{escaped}' && git rev-parse 'refs/remotes/origin/{escaped}'"
    );
    let output = shell::agent_exec_capture(agent_name, &cmd).await.ok()?;
    let oid = output.trim();
    (!oid.is_empty()).then(|| oid.to_string())
}

/// Rebase the agent's local branch onto `remote_head` unless it already contains it, and
/// return whether a rebase happened. Pushes upload every file that differs from the remote
/// branch, so commits someone else added there (a reviewer's fix, GitHub's "Update branch")
/// would otherwise be silently reverted.
async fn rebase_onto_remote_head(agent_name: &str, remote_head: &str) -> Result<bool, AppError> {
    let ancestry = shell::agent_exec_capture(
        agent_name,
        &format!(
            "cd /home/agent/repo && if git merge-base --is-ancestor {remote_head} HEAD; then echo contained; else echo diverged; fi"
        ),
    )
    .await?;
    if ancestry.trim_end().ends_with("contained") {
        return Ok(false);
    }

    shell::agent_exec_capture(
        agent_name,
        &format!(
            "cd /home/agent/repo && git rebase --autostash {remote_head} || {{ git rebase --abort; exit 1; }}"
        ),
    )
    .await
    .map_err(|e| {
        AppError::Internal(format!(
            "The task branch has commits that conflict with Claude's changes; resolve them on the branch and ask Claude to continue. ({e})"
        ))
    })?;
    Ok(true)
}

/// Whether the fetched task branch has commits that aren't on the base branch.
async fn remote_branch_ahead_of_base(
    agent_name: &str,
    branch_name: &str,
    base_branch: &str,
) -> bool {
    let cmd = format!(
        "cd /home/agent/repo && git rev-list --count 'origin/{base_branch}..refs/remotes/origin/{branch_name}'"
    );
    shell::agent_exec_capture(agent_name, &cmd)
        .await
        .ok()
        .and_then(|out| out.trim().parse::<u64>().ok())
        .is_some_and(|n| n > 0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PushOutcome {
    /// The task's intake source has auto_create_pr off; delivery is left to the agent.
    Disabled,
    NoChanges,
    Pushed,
    /// Nothing left to push because the agent already pushed its commits to the branch.
    AgentPushed,
}

impl PushOutcome {
    fn branch_has_commits(self) -> bool {
        matches!(self, PushOutcome::Pushed | PushOutcome::AgentPushed)
    }
}

async fn push_and_preview(
    ctx: &PipelineCtx<'_>,
    agent_name: &str,
    branch_name: &str,
    branch_pushed: &mut bool,
    preview_created: &mut bool,
    tx: &broadcast::Sender<String>,
) -> Result<PushOutcome, AppError> {
    let &PipelineCtx {
        db,
        task_id,
        repo,
        base_branch,
        git_id,
        ..
    } = ctx;

    if !should_deliver(db, task_id).await? {
        log_and_send(
            db,
            task_id,
            tx,
            "[STEP] Auto-push disabled for this intake source; leaving changes to the agent.",
        );
        return Ok(PushOutcome::Disabled);
    }

    // Diff against the branch as it is on GitHub right now: the agent may have pushed to it
    // itself, and a stale ref makes createCommitOnBranch fail with STALE_DATA.
    let remote_head = fetch_remote_branch(agent_name, branch_name).await;
    if let Some(oid) = &remote_head {
        if rebase_onto_remote_head(agent_name, oid).await? {
            log_and_send(
                db,
                task_id,
                tx,
                "[STEP] Rebased onto new commits on the task branch.",
            );
        }
    }
    let base_ref = if remote_head.is_some() || *branch_pushed {
        format!("origin/{branch_name}")
    } else {
        format!("origin/{base_branch}")
    };

    // Collect file changes via Python script in the agent container
    update_task_status(db, task_id, "pushing", None).await?;
    log_and_send(db, task_id, tx, "[STEP] Collecting file changes...");
    let file_changes = collect_file_changes(agent_name, &base_ref).await?;

    if file_changes.additions.is_empty() && file_changes.deletions.is_empty() {
        if !*branch_pushed
            && remote_head.is_some()
            && remote_branch_ahead_of_base(agent_name, branch_name, base_branch).await
        {
            log_and_send(
                db,
                task_id,
                tx,
                "[STEP] The agent already pushed its changes to the branch.",
            );
            *branch_pushed = true;
            update_task_preview(ctx, branch_name, preview_created, tx).await?;
            return Ok(PushOutcome::AgentPushed);
        }
        log_and_send(db, task_id, tx, "[WARN] No file changes to push.");
        return Ok(PushOutcome::NoChanges);
    }

    save_system_message(db, task_id, "Deploying changes to preview...").await?;

    log_and_send(
        db,
        task_id,
        tx,
        &format!(
            "[STEP] Pushing {} addition(s) and {} deletion(s) via GitHub API...",
            file_changes.additions.len(),
            file_changes.deletions.len()
        ),
    );

    // On first push: get base SHA and create the branch on GitHub
    let expected_oid = if let Some(oid) = remote_head {
        oid
    } else if !*branch_pushed {
        let base_sha = shell::agent_exec_capture(
            agent_name,
            &format!("cd /home/agent/repo && git rev-parse '{base_ref}'"),
        )
        .await?
        .trim()
        .to_string();

        create_github_branch(&git_id.token, repo, branch_name, &base_sha).await?;
        log_and_send(
            db,
            task_id,
            tx,
            &format!("[STEP] Created branch {branch_name} on GitHub."),
        );
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
        &git_id.token,
        repo,
        branch_name,
        &expected_oid,
        &file_changes,
        &commit_msg,
    )
    .await?;
    log_and_send(
        db,
        task_id,
        tx,
        &format!("[STEP] Created verified commit {oid}"),
    );

    // Sync local repo to match the API-created commit
    sync_agent_to_remote(agent_name, branch_name).await?;
    *branch_pushed = true;

    update_task_preview(ctx, branch_name, preview_created, tx).await?;
    Ok(PushOutcome::Pushed)
}

/// Create the task's preview on its branch, or update it if it already exists, then take a
/// screenshot in the background.
async fn update_task_preview(
    ctx: &PipelineCtx<'_>,
    branch_name: &str,
    preview_created: &mut bool,
    tx: &broadcast::Sender<String>,
) -> Result<(), AppError> {
    let &PipelineCtx {
        config,
        db,
        task_id,
        short_id,
        repo,
        git_id,
        ..
    } = ctx;
    let preview_slug = format!("t-{short_id}");

    if !*preview_created {
        update_task_status(db, task_id, "creating_preview", None).await?;
        log_and_send(
            db,
            task_id,
            tx,
            &format!("[STEP] Creating preview '{preview_slug}'..."),
        );

        shell::create_preview(
            config,
            repo,
            branch_name,
            Some(&preview_slug),
            &git_id.token,
        )
        .await?;

        let preview_url = format!("https://{preview_slug}.{}", config.preview_domain);
        update_task_field(db, task_id, "preview_slug", &preview_slug).await?;
        update_task_field(db, task_id, "preview_url", &preview_url).await?;
        *preview_created = true;
    } else {
        log_and_send(
            db,
            task_id,
            tx,
            &format!("[STEP] Updating preview '{preview_slug}'..."),
        );
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
        take_screenshot(
            &config2,
            &db2,
            &task_id2,
            &preview_slug2,
            &preview_url2,
            &tx2,
        )
        .await;
    });

    Ok(())
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
    let rows = sqlx::query_scalar::<_, String>("SELECT DISTINCT repo FROM tasks ORDER BY repo")
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
    check_repo_permission(
        &state.db,
        &user.0.sub,
        &full_repo,
        &user.0.role,
        &state.config.github_org,
    )
    .await?;
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

        let items: Vec<serde_json::Value> = resp
            .json()
            .await
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
    check_task_ownership(
        &state.db,
        &id,
        &user.0.sub,
        &user.0.role,
        &state.config.github_org,
    )
    .await?;

    let messages = if let Some(before_id) = params.before_id {
        let limit = params.limit.unwrap_or(100);
        sqlx::query_as::<_, TaskMessage>(
            "SELECT id, task_id, sender, content, \
             TO_CHAR(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as created_at, image_url \
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
             TO_CHAR(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as created_at, image_url \
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
             TO_CHAR(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as created_at, image_url \
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
    check_task_ownership(
        &state.db,
        &id,
        &user.0.sub,
        &user.0.role,
        &state.config.github_org,
    )
    .await?;

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
        return Err(AppError::BadRequest(
            "Message content cannot be empty".into(),
        ));
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
         TO_CHAR(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as created_at, image_url \
         FROM task_messages WHERE task_id = $1 ORDER BY id DESC LIMIT 1",
    )
    .bind(&id)
    .fetch_one(&state.db)
    .await?;

    // Audit: task.message_sent
    crate::audit::log_event(
        &state.db,
        "task.message_sent",
        &user.0.sub,
        Some(&id),
        serde_json::json!({ "message_id": message.id }),
        None,
    )
    .await;

    Ok(Json(message))
}

// ── Actions endpoint ──

pub async fn list_actions(
    user: AuthUser,
    State(state): State<crate::AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<TaskAction>>, AppError> {
    check_task_ownership(
        &state.db,
        &id,
        &user.0.sub,
        &user.0.role,
        &state.config.github_org,
    )
    .await?;
    let actions = sqlx::query_as::<_, TaskAction>(
        "SELECT id, task_id, action_type, tool_name, tool_input, summary, \
         TO_CHAR(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as created_at \
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

fn log_and_send(db: &PgPool, task_id: &str, tx: &broadcast::Sender<String>, msg: &str) {
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

pub(crate) async fn update_task_status(
    db: &PgPool,
    task_id: &str,
    status: &str,
    error: Option<&str>,
) -> Result<(), AppError> {
    // Get the current status and created_by for the state transition record / audit
    let prev: Option<(String, Option<String>)> =
        sqlx::query_as("SELECT status, created_by FROM tasks WHERE id = $1")
            .bind(task_id)
            .fetch_optional(db)
            .await?;
    let (current_status, created_by) = match prev {
        Some((s, c)) => (Some(s), c),
        None => (None, None),
    };

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

    // Audit: task.completed / task.failed
    if status == "completed" || status == "failed" {
        let actor = created_by.as_deref().unwrap_or("system");
        let event_type = if status == "completed" {
            "task.completed"
        } else {
            "task.failed"
        };
        let mut detail = serde_json::json!({ "status": status });
        if let Some(err) = error {
            detail["error"] = serde_json::Value::String(err.to_string());
        }
        crate::audit::log_event(db, event_type, actor, Some(task_id), detail, None).await;
    }

    Ok(())
}

async fn update_task_field(
    db: &PgPool,
    task_id: &str,
    field: &str,
    value: &str,
) -> Result<(), AppError> {
    // Safe because field is always a hardcoded string from our code
    let query = format!("UPDATE tasks SET {field} = $1, updated_at = NOW() WHERE id = $2");
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
    check_task_ownership(
        &state.db,
        &id,
        &user.0.sub,
        &user.0.role,
        &state.config.github_org,
    )
    .await?;

    let task = sqlx::query_as::<_, Task>(
        "SELECT id, prompt, repo, base_branch, branch_name, agent_name, status, \
         preview_slug, preview_url, error_message, \
         TO_CHAR(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as created_at, \
         TO_CHAR(updated_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as updated_at, \
         parent_task_id, created_by, screenshot_url, image_url, \
         total_input_tokens, total_output_tokens, total_cost_usd, name, pr_url, pr_number, compute_seconds, \
         intake_issue_id, source_type, \
         TO_CHAR(agent_retained_until, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as agent_retained_until \
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

    let branch_name = task
        .branch_name
        .as_deref()
        .ok_or_else(|| AppError::BadRequest("Task has no branch — cannot reopen".into()))?;

    let short_id = &id[..6];

    // Get the reopening user's git identity
    let git_id = get_git_identity(&state.db, &user.0.sub).await?;

    // Reset status and error. A kept container is live again, so stop its retention countdown.
    update_task_status(&state.db, &id, "creating_agent", None).await?;
    agents::clear_retention(&state.db, &id).await;

    // Audit: task.reopened
    crate::audit::log_event(
        &state.db,
        "task.reopened",
        &user.0.sub,
        Some(&id),
        serde_json::json!({ "previous_status": &task.status }),
        None,
    )
    .await;

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
        let ctx = PipelineCtx {
            config: &config,
            db: &db,
            task_id: &task_id,
            short_id: &short,
            repo: &repo,
            base_branch: &base,
            git_id: &git_id,
        };
        let result = run_reopen_pipeline(&ctx, &branch, has_preview, &created_by, tx.clone()).await;

        if let Err(e) = &result {
            agents::fail_task(&config, &db, &task_id, &e.to_string()).await;
            let _ = tx.send(format!("[ERROR] Reopen failed: {e}"));
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
         TO_CHAR(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as created_at, \
         TO_CHAR(updated_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as updated_at, \
         parent_task_id, created_by, screenshot_url, image_url, \
         total_input_tokens, total_output_tokens, total_cost_usd, name, pr_url, pr_number, compute_seconds, \
         intake_issue_id, source_type, \
         TO_CHAR(agent_retained_until, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as agent_retained_until \
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
    check_task_ownership(
        &state.db,
        &id,
        &user.0.sub,
        &user.0.role,
        &state.config.github_org,
    )
    .await?;

    let task = sqlx::query_as::<_, Task>(
        "SELECT id, prompt, repo, base_branch, branch_name, agent_name, status, \
         preview_slug, preview_url, error_message, \
         TO_CHAR(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as created_at, \
         TO_CHAR(updated_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as updated_at, \
         parent_task_id, created_by, screenshot_url, image_url, \
         total_input_tokens, total_output_tokens, total_cost_usd, name, pr_url, pr_number, compute_seconds, \
         intake_issue_id, source_type, \
         TO_CHAR(agent_retained_until, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as agent_retained_until \
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
    check_task_ownership(
        &state.db,
        &id,
        &user.0.sub,
        &user.0.role,
        &state.config.github_org,
    )
    .await?;
    let _ = sqlx::query_as::<_, Task>(
        "SELECT id, prompt, repo, base_branch, branch_name, agent_name, status, \
         preview_slug, preview_url, error_message, \
         TO_CHAR(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as created_at, \
         TO_CHAR(updated_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as updated_at, \
         parent_task_id, created_by, screenshot_url, image_url, \
         total_input_tokens, total_output_tokens, total_cost_usd, name, pr_url, pr_number, compute_seconds, \
         intake_issue_id, source_type, \
         TO_CHAR(agent_retained_until, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as agent_retained_until \
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

    // Audit: task.renamed
    crate::audit::log_event(
        &state.db,
        "task.renamed",
        &user.0.sub,
        Some(&id),
        serde_json::json!({ "new_name": &req.name }),
        None,
    )
    .await;

    let task = sqlx::query_as::<_, Task>(
        "SELECT id, prompt, repo, base_branch, branch_name, agent_name, status, \
         preview_slug, preview_url, error_message, \
         TO_CHAR(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as created_at, \
         TO_CHAR(updated_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as updated_at, \
         parent_task_id, created_by, screenshot_url, image_url, \
         total_input_tokens, total_output_tokens, total_cost_usd, name, pr_url, pr_number, compute_seconds, \
         intake_issue_id, source_type, \
         TO_CHAR(agent_retained_until, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as agent_retained_until \
         FROM tasks WHERE id = $1"
    )
    .bind(&id)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(task))
}

async fn run_reopen_pipeline(
    ctx: &PipelineCtx<'_>,
    branch_name: &str,
    had_preview: bool,
    created_by: &str,
    tx: broadcast::Sender<String>,
) -> Result<(), AppError> {
    let &PipelineCtx {
        config,
        db,
        task_id,
        short_id,
        repo,
        base_branch: _,
        git_id,
    } = ctx;
    // Check if the previous agent container is still alive (e.g. server crashed mid-run).
    // Look up the last-known agent_name from the DB.
    let prev_agent =
        sqlx::query_scalar::<_, Option<String>>("SELECT agent_name FROM tasks WHERE id = $1")
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
        log_and_send(
            db,
            task_id,
            &tx,
            &format!("[STEP] Reusing existing agent container '{name}'..."),
        );
        update_task_field(db, task_id, "agent_name", &name).await?;
        // It holds the previous work: a failure from here on must keep it, not destroy it.
        update_task_status(db, task_id, "awaiting_followup", None).await?;
        name
    } else {
        // Step 1: Create agent container
        let name = format!("task-{short_id}");
        log_and_send(
            db,
            task_id,
            &tx,
            &format!("[STEP] Creating agent container '{name}' (reopen)..."),
        );
        let create_start = std::time::Instant::now();
        shell::create_agent(config, &name).await?;
        let create_ms = create_start.elapsed().as_millis();
        log_and_send(
            db,
            task_id,
            &tx,
            &format!("[STEP] Agent '{name}' created in {create_ms}ms"),
        );
        update_task_field(db, task_id, "agent_name", &name).await?;

        // Step 2: Clone repo and checkout existing branch
        update_task_status(db, task_id, "cloning", None).await?;
        log_and_send(
            db,
            task_id,
            &tx,
            &format!("[STEP] Cloning {repo} and checking out existing branch {branch_name}..."),
        );

        let clone_url = format!(
            "https://x-access-token:{}@github.com/{repo}.git",
            git_id.token
        );

        let escaped_name = git_id.name.replace('\'', "'\\''");
        let escaped_email = git_id.email.replace('\'', "'\\''");
        let clone_cmd = format!(
            "cd /home/agent && git clone '{clone_url}' repo && \
             cd repo && git checkout '{branch_name}' && \
             git config user.name '{escaped_name}' && git config user.email '{escaped_email}'"
        );
        shell::agent_exec(&name, &clone_cmd, tx.clone()).await?;

        // Step 2b: Load and inject secrets
        let repo_secrets =
            secrets::load_secrets_for_repo(db, &config.secrets_encryption_key, repo).await?;
        if !repo_secrets.is_empty() {
            log_and_send(
                db,
                task_id,
                &tx,
                &format!("[STEP] Injecting {} secret(s)...", repo_secrets.len()),
            );
            write_secrets_env_file(&name, &repo_secrets, tx.clone()).await?;
        }
        name
    };

    // Load effective policy for enforcement during follow-ups
    let policy = policies::load_effective_policy(db, repo).await?;

    // Apply network egress restrictions (if any)
    if let Some(ref pol) = policy {
        if let Some(ref egress) = pol.network_egress {
            log_and_send(
                db,
                task_id,
                &tx,
                "[POLICY] Applying network egress restrictions...",
            );
            if let Err(e) = shell::apply_egress_rules(&agent_name, egress).await {
                log_and_send(
                    db,
                    task_id,
                    &tx,
                    &format!("[WARN] Failed to apply egress rules: {e}"),
                );
            }
        }
    }

    // Step 3: Go straight into follow-up loop
    // branch_pushed starts as true since the branch already exists on GitHub
    let mut branch_pushed = true;
    let mut preview_created = had_preview;
    let outcome = follow_up_loop(
        ctx,
        &agent_name,
        branch_name,
        &mut branch_pushed,
        &mut preview_created,
        created_by,
        &tx,
        0,
    )
    .await?;

    // Step 4: Destroy agent container
    if finish_task(ctx, &agent_name, outcome, &tx).await? {
        log_and_send(db, task_id, &tx, "[DONE] Task completed.");
    }

    Ok(())
}

/// Reconnect to an existing live container and re-enter the follow-up loop.
///
/// Used during dashboard restart recovery for tasks that already have a container
/// with the repo checked out. Does NOT create containers or invoke Claude — zero
/// AI tokens are consumed until the user sends a follow-up message.
async fn recover_to_followup(
    ctx: &PipelineCtx<'_>,
    branch_name: &str,
    had_preview: bool,
    created_by: &str,
    tx: broadcast::Sender<String>,
) -> Result<(), AppError> {
    let &PipelineCtx {
        config: _,
        db,
        task_id,
        short_id: _,
        repo,
        base_branch: _,
        git_id: _,
    } = ctx;

    // Look up agent_name from DB
    let agent_name =
        sqlx::query_scalar::<_, Option<String>>("SELECT agent_name FROM tasks WHERE id = $1")
            .bind(task_id)
            .fetch_one(db)
            .await?
            .ok_or_else(|| AppError::Internal("No agent_name for recovered task".into()))?;

    // Check container liveness
    if shell::agent_ip_public(&agent_name).is_err() {
        return Err(AppError::Internal(format!(
            "Agent container '{agent_name}' is no longer reachable"
        )));
    }

    log_and_send(
        db,
        task_id,
        &tx,
        &format!("[STEP] Reconnected to existing agent container '{agent_name}'"),
    );

    // Refresh the git remote URL with a fresh token so pushes/fetches work
    let clone_url = format!(
        "https://x-access-token:{}@github.com/{repo}.git",
        ctx.git_id.token
    );
    let remote_cmd = format!(
        "cd /home/agent/repo && git remote set-url origin '{}'",
        clone_url.replace('\'', "'\\''")
    );
    if let Err(e) = shell::agent_exec_capture(&agent_name, &remote_cmd).await {
        tracing::warn!("Failed to refresh git remote URL: {e}");
    }

    // Load effective policy for enforcement during follow-ups
    let policy = policies::load_effective_policy(db, repo).await?;

    // Apply network egress restrictions (if any)
    if let Some(ref pol) = policy {
        if let Some(ref egress) = pol.network_egress {
            log_and_send(
                db,
                task_id,
                &tx,
                "[POLICY] Applying network egress restrictions...",
            );
            if let Err(e) = shell::apply_egress_rules(&agent_name, egress).await {
                log_and_send(
                    db,
                    task_id,
                    &tx,
                    &format!("[WARN] Failed to apply egress rules: {e}"),
                );
            }
        }
    }

    // Skip all messages that existed before recovery so we don't replay them
    let max_msg_id: i64 =
        sqlx::query_scalar("SELECT COALESCE(MAX(id), 0) FROM task_messages WHERE task_id = $1")
            .bind(task_id)
            .fetch_one(db)
            .await?;

    // Enter follow-up loop — branch already pushed, preview may already exist
    let mut branch_pushed = true;
    let mut preview_created = had_preview;
    let outcome = follow_up_loop(
        ctx,
        &agent_name,
        branch_name,
        &mut branch_pushed,
        &mut preview_created,
        created_by,
        &tx,
        max_msg_id,
    )
    .await?;

    // Destroy agent container after follow-up loop completes
    if finish_task(ctx, &agent_name, outcome, &tx).await? {
        log_and_send(db, task_id, &tx, "[DONE] Task completed.");
    }

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
         TO_CHAR(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as created_at, \
         TO_CHAR(updated_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as updated_at, \
         parent_task_id, created_by, screenshot_url, image_url, \
         total_input_tokens, total_output_tokens, total_cost_usd, name, pr_url, pr_number, compute_seconds, \
         intake_issue_id, source_type, \
         TO_CHAR(agent_retained_until, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as agent_retained_until \
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
            "awaiting_followup" | "push_failed" => {
                let Some(branch_name) = task.branch_name.clone() else {
                    let _ = update_task_status(
                        &db,
                        &task.id,
                        "failed",
                        Some("Interrupted by dashboard restart: missing branch name"),
                    )
                    .await;
                    continue;
                };
                let Some(ref created_by) = task.created_by else {
                    let _ = update_task_status(
                        &db,
                        &task.id,
                        "failed",
                        Some("Interrupted by dashboard restart: missing created_by"),
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
                            Some(&format!("Interrupted by dashboard restart: {e}")),
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
                    tracing::info!("Recovering idle task {task_id}");
                    let ctx = PipelineCtx {
                        config: &cfg,
                        db: &db2,
                        task_id: &task_id,
                        short_id: &short_id,
                        repo: &repo,
                        base_branch: &base,
                        git_id: &git_id,
                    };
                    let result = recover_to_followup(
                        &ctx,
                        &branch_name,
                        had_preview,
                        &created_by,
                        tx.clone(),
                    )
                    .await;
                    if let Err(e) = result {
                        agents::fail_task(&cfg, &db2, &task_id, &format!("Recovery failed: {e}"))
                            .await;
                        let _ = tx.send(format!("[ERROR] Recovery failed: {e}"));
                    }
                    tokio::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                        channels.remove(&task_id);
                    });
                });
            }
            "running_claude" => {
                let Some(branch_name) = task.branch_name.clone() else {
                    let _ = update_task_status(
                        &db,
                        &task.id,
                        "failed",
                        Some("Interrupted by dashboard restart: missing branch name"),
                    )
                    .await;
                    continue;
                };
                let Some(ref created_by) = task.created_by else {
                    let _ = update_task_status(
                        &db,
                        &task.id,
                        "failed",
                        Some("Interrupted by dashboard restart: missing created_by"),
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
                            Some(&format!("Interrupted by dashboard restart: {e}")),
                        )
                        .await;
                        continue;
                    }
                };

                // Insert system message explaining the interruption
                let _ = save_system_message(
                    &db,
                    &task.id,
                    "Dashboard restarted while Claude was working. Send a follow-up message to continue the conversation.",
                ).await;

                // Transition to awaiting_followup so the user can resume
                let _ = update_task_status(&db, &task.id, "awaiting_followup", None).await;

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
                    tracing::info!(
                        "Recovering running_claude task {task_id} (→ awaiting_followup)"
                    );
                    let ctx = PipelineCtx {
                        config: &cfg,
                        db: &db2,
                        task_id: &task_id,
                        short_id: &short_id,
                        repo: &repo,
                        base_branch: &base,
                        git_id: &git_id,
                    };
                    let result = recover_to_followup(
                        &ctx,
                        &branch_name,
                        had_preview,
                        &created_by,
                        tx.clone(),
                    )
                    .await;
                    if let Err(e) = result {
                        agents::fail_task(&cfg, &db2, &task_id, &format!("Recovery failed: {e}"))
                            .await;
                        let _ = tx.send(format!("[ERROR] Recovery failed: {e}"));
                    }
                    tokio::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                        channels.remove(&task_id);
                    });
                });
            }
            "pending" => {
                tracing::info!("Task {} was pending at restart, marking failed", task.id);
                let _ = update_task_status(
                    &db,
                    &task.id,
                    "failed",
                    Some("Interrupted by dashboard restart. Please create a new task."),
                )
                .await;
            }
            "pushing" | "creating_preview" => {
                tracing::info!(
                    "Task {} was interrupted during '{}', marking failed",
                    task.id,
                    task.status
                );
                agents::fail_task(
                    &config,
                    &db,
                    &task.id,
                    &format!(
                        "Interrupted by dashboard restart during {}. Use Reopen to continue.",
                        task.status
                    ),
                )
                .await;
            }
            "creating_agent" | "cloning" => {
                tracing::info!(
                    "Task {} was interrupted during '{}', marking failed",
                    task.id,
                    task.status
                );
                let _ = update_task_status(
                    &db,
                    &task.id,
                    "failed",
                    Some(&format!(
                        "Interrupted by dashboard restart during {}. Use Reopen to continue.",
                        task.status
                    )),
                )
                .await;
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
                    Some("Interrupted by dashboard restart. Use Reopen to continue."),
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
         TO_CHAR(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as created_at, image_url \
         FROM task_messages WHERE task_id = $1 ORDER BY id",
    )
    .bind(&task.id)
    .fetch_all(db)
    .await
    .unwrap_or_default();

    let conversation = messages
        .iter()
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
                Some(files) => files
                    .iter()
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
        format!(
            "ANTHROPIC_API_KEY={}",
            if cfg.provider == "openrouter" {
                ""
            } else {
                &cfg.api_key
            }
        ),
        "HOME=/tmp".to_string(),
    ];
    if cfg.provider == "openrouter" {
        let model = cfg
            .model
            .as_deref()
            .unwrap_or("anthropic/claude-sonnet-4.6");
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
    check_task_ownership(
        &state.db,
        &id,
        &user.0.sub,
        &user.0.role,
        &state.config.github_org,
    )
    .await?;

    let task = fetch_task(&state.db, &id)
        .await?
        .ok_or_else(|| AppError::NotFound("Task not found".into()))?;

    if task.pr_url.is_some() {
        return Err(AppError::BadRequest(
            "PR already exists for this task".into(),
        ));
    }

    let git_id = get_git_identity(&state.db, &user.0.sub).await?;
    open_pr_for_task(
        &state.db,
        &state.config.secrets_encryption_key,
        &git_id,
        &task,
        &user.0.sub,
    )
    .await?;

    let updated_task = fetch_task(&state.db, &id)
        .await?
        .ok_or_else(|| AppError::NotFound("Task not found".into()))?;

    Ok(Json(updated_task))
}

async fn fetch_task(db: &PgPool, id: &str) -> Result<Option<Task>, AppError> {
    Ok(sqlx::query_as::<_, Task>(
        "SELECT id, prompt, repo, base_branch, branch_name, agent_name, status, \
         preview_slug, preview_url, error_message, \
         TO_CHAR(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as created_at, \
         TO_CHAR(updated_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as updated_at, \
         parent_task_id, created_by, screenshot_url, image_url, \
         total_input_tokens, total_output_tokens, total_cost_usd, name, pr_url, pr_number, compute_seconds, \
         intake_issue_id, source_type, \
         TO_CHAR(agent_retained_until, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as agent_retained_until \
         FROM tasks WHERE id = $1"
    )
    .bind(id)
    .fetch_optional(db)
    .await?)
}

#[derive(Debug, PartialEq)]
enum PrCreateRejection {
    AlreadyExists,
    NoCommits,
    Other,
}

fn classify_pr_create_error(status: u16, body: &str) -> PrCreateRejection {
    if status != 422 {
        return PrCreateRejection::Other;
    }
    if body.contains("A pull request already exists") {
        PrCreateRejection::AlreadyExists
    } else if body.contains("No commits between") {
        PrCreateRejection::NoCommits
    } else {
        PrCreateRejection::Other
    }
}

/// Find the open PR whose head is `branch_name` in `repo`.
async fn find_open_pr_for_branch(
    token: &str,
    repo: &str,
    branch_name: &str,
) -> Result<Option<serde_json::Value>, AppError> {
    let owner = repo.split('/').next().unwrap_or(repo);
    let resp = reqwest::Client::new()
        .get(format!("https://api.github.com/repos/{repo}/pulls"))
        .query(&[
            ("head", format!("{owner}:{branch_name}")),
            ("state", "open".to_string()),
        ])
        .header("Authorization", format!("token {token}"))
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
    let prs: Vec<serde_json::Value> = resp
        .json()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to parse PR list: {e}")))?;
    Ok(prs.into_iter().next())
}

/// Open a PR for the task's branch as `acting_user` and record it on the task. If GitHub
/// reports that the branch already has a PR (e.g. the agent opened one), that PR is recorded.
async fn open_pr_for_task(
    db: &PgPool,
    encryption_key: &str,
    git_id: &GitIdentity,
    task: &Task,
    acting_user: &str,
) -> Result<(String, i32), AppError> {
    let branch_name = task
        .branch_name
        .as_deref()
        .ok_or_else(|| AppError::BadRequest("Task has no branch".into()))?;

    // Build PR title from task name or prompt
    let title = task
        .name
        .as_deref()
        .unwrap_or(&task.prompt)
        .chars()
        .take(72)
        .collect::<String>();

    // Build PR body: gather context and ask Claude to write the description
    let mut body = generate_pr_body(db, encryption_key, &git_id.token, task, acting_user)
        .await
        .unwrap_or_else(|e| {
            tracing::warn!("Failed to generate PR body via Claude: {e}, using fallback");
            format!("## Task\n\n{}", task.prompt)
        });

    // Append preview URL if one exists for this task
    if let Some(ref url) = task.preview_url {
        body.push_str(&format!("\n\n---\n\n🔗 **Preview:** {url}"));
    }

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

    let (pr_data, existing): (serde_json::Value, bool) = if resp.status().is_success() {
        let data = resp
            .json()
            .await
            .map_err(|e| AppError::Internal(format!("Failed to parse PR response: {e}")))?;
        (data, false)
    } else {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        match classify_pr_create_error(status.as_u16(), &text) {
            PrCreateRejection::AlreadyExists => {
                let pr = find_open_pr_for_branch(&git_id.token, &task.repo, branch_name)
                    .await?
                    .ok_or_else(|| {
                        AppError::Internal(format!("GitHub API returned {status}: {text}"))
                    })?;
                (pr, true)
            }
            PrCreateRejection::NoCommits => {
                return Err(AppError::BadRequest("Branch has no changes yet".into()));
            }
            PrCreateRejection::Other => {
                return Err(AppError::Internal(format!(
                    "GitHub API returned {status}: {text}"
                )));
            }
        }
    };

    let pr_url = pr_data["html_url"].as_str().unwrap_or("").to_string();
    let pr_number = pr_data["number"].as_i64().unwrap_or(0) as i32;

    // Only the first writer records the PR (the Create PR button can race the auto-PR).
    let updated = sqlx::query(
        "UPDATE tasks SET pr_url = $1, pr_number = $2, updated_at = NOW() \
         WHERE id = $3 AND pr_url IS NULL",
    )
    .bind(&pr_url)
    .bind(pr_number)
    .bind(&task.id)
    .execute(db)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(AppError::BadRequest(
            "PR already exists for this task".into(),
        ));
    }

    // Audit: task.pr_created
    crate::audit::log_event(
        db,
        "task.pr_created",
        acting_user,
        Some(&task.id),
        serde_json::json!({ "pr_url": &pr_url, "pr_number": pr_number, "existing": existing }),
        None,
    )
    .await;

    Ok((pr_url, pr_number))
}

/// For intake sources with auto_create_pr on, open the task's PR in the background once its
/// branch has commits. Failures are reported on the task but never fail it.
async fn maybe_auto_open_pr(
    ctx: &PipelineCtx<'_>,
    acting_user: &str,
    tx: &broadcast::Sender<String>,
) {
    let &PipelineCtx {
        config,
        db,
        task_id,
        git_id,
        ..
    } = ctx;
    match intake_auto_create_pr(db, task_id).await {
        Ok(Some(true)) => {}
        Ok(_) => return,
        Err(e) => {
            tracing::warn!("Could not resolve auto_create_pr for task {task_id}: {e}");
            return;
        }
    }
    let task = match fetch_task(db, task_id).await {
        Ok(Some(task)) if task.pr_url.is_none() => task,
        Ok(_) => return,
        Err(e) => {
            tracing::warn!("Could not load task {task_id} for auto PR: {e}");
            return;
        }
    };

    let db = db.clone();
    let encryption_key = config.secrets_encryption_key.clone();
    let git_id = git_id.clone();
    let acting_user = acting_user.to_string();
    let tx = tx.clone();
    tokio::spawn(async move {
        log_and_send(&db, &task.id, &tx, "[STEP] Opening pull request...");
        match open_pr_for_task(&db, &encryption_key, &git_id, &task, &acting_user).await {
            Ok((pr_url, _)) => {
                log_and_send(
                    &db,
                    &task.id,
                    &tx,
                    &format!("[STEP] Pull request: {pr_url}"),
                );
                let _ =
                    save_system_message(&db, &task.id, &format!("Pull request: {pr_url}")).await;
            }
            // No commits yet, or a PR was recorded in the meantime: nothing to do.
            Err(AppError::BadRequest(msg)) => {
                log_and_send(
                    &db,
                    &task.id,
                    &tx,
                    &format!("[STEP] Skipped auto PR: {msg}"),
                );
            }
            Err(e) => {
                log_and_send(
                    &db,
                    &task.id,
                    &tx,
                    &format!("[WARN] Auto PR creation failed: {e}"),
                );
                let _ = save_system_message(
                    &db,
                    &task.id,
                    &format!("Automatic PR creation failed: {e}. Use Create PR to retry."),
                )
                .await;
            }
        }
    });
}

pub async fn link_pr(
    user: MemberUser,
    State(state): State<crate::AppState>,
    Path(id): Path<String>,
    Json(req): Json<crate::models::LinkPrRequest>,
) -> Result<Json<Task>, AppError> {
    check_task_ownership(
        &state.db,
        &id,
        &user.0.sub,
        &user.0.role,
        &state.config.github_org,
    )
    .await?;
    // Extract PR number from URL (e.g. https://github.com/owner/repo/pull/123)
    let pr_number: Option<i32> = req.pr_url.rsplit('/').next().and_then(|s| s.parse().ok());

    sqlx::query("UPDATE tasks SET pr_url = $1, pr_number = $2, updated_at = NOW() WHERE id = $3")
        .bind(&req.pr_url)
        .bind(pr_number)
        .bind(&id)
        .execute(&state.db)
        .await?;

    // Audit: task.pr_linked
    crate::audit::log_event(
        &state.db,
        "task.pr_linked",
        &user.0.sub,
        Some(&id),
        serde_json::json!({ "pr_url": &req.pr_url }),
        None,
    )
    .await;

    let task = sqlx::query_as::<_, Task>(
        "SELECT id, prompt, repo, base_branch, branch_name, agent_name, status, \
         preview_slug, preview_url, error_message, \
         TO_CHAR(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as created_at, \
         TO_CHAR(updated_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as updated_at, \
         parent_task_id, created_by, screenshot_url, image_url, \
         total_input_tokens, total_output_tokens, total_cost_usd, name, pr_url, pr_number, compute_seconds, \
         intake_issue_id, source_type, \
         TO_CHAR(agent_retained_until, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as agent_retained_until \
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
    check_task_ownership(
        &state.db,
        &id,
        &user.0.sub,
        &user.0.role,
        &state.config.github_org,
    )
    .await?;
    let logs = sqlx::query_as::<_, TaskLog>(
        "SELECT id, task_id, TO_CHAR(timestamp, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as timestamp, line \
         FROM task_logs WHERE task_id = $1 ORDER BY id ASC",
    )
    .bind(&id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(logs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_create_pr_resolution() {
        assert_eq!(resolve_auto_create_pr(None, None), None);
        assert_eq!(
            resolve_auto_create_pr(Some("manual".into()), Some(true)),
            None
        );
        assert_eq!(
            resolve_auto_create_pr(Some("intake_github".into()), Some(true)),
            Some(true)
        );
        assert_eq!(
            resolve_auto_create_pr(Some("intake_github".into()), Some(false)),
            Some(false)
        );
        // Source deleted: never start pushing
        assert_eq!(
            resolve_auto_create_pr(Some("intake_github".into()), None),
            Some(false)
        );
    }

    #[test]
    fn pr_create_error_classification() {
        let exists = r#"{"message":"Validation Failed","errors":[{"resource":"PullRequest","code":"custom","message":"A pull request already exists for lambdaclass:task-1."}]}"#;
        let no_commits = r#"{"message":"Validation Failed","errors":[{"resource":"PullRequest","code":"custom","message":"No commits between main and task-1"}]}"#;
        assert_eq!(
            classify_pr_create_error(422, exists),
            PrCreateRejection::AlreadyExists
        );
        assert_eq!(
            classify_pr_create_error(422, no_commits),
            PrCreateRejection::NoCommits
        );
        assert_eq!(
            classify_pr_create_error(422, "{}"),
            PrCreateRejection::Other
        );
        assert_eq!(
            classify_pr_create_error(403, exists),
            PrCreateRejection::Other
        );
    }

    #[test]
    fn graphql_forbidden_hints() {
        let forbidden = serde_json::json!([{ "type": "FORBIDDEN", "message": "no" }]);
        let changes = |path: &str| FileChanges {
            additions: vec![FileAddition {
                path: path.into(),
                contents: String::new(),
            }],
            deletions: vec![],
        };
        assert!(
            graphql_error_hint(&forbidden, &changes(".github/workflows/ci.yml"))
                .contains("`workflow` scope")
        );
        assert!(graphql_error_hint(&forbidden, &changes("src/main.rs")).contains("Contents write"));
        let stale = serde_json::json!([{ "type": "STALE_DATA", "message": "no" }]);
        assert_eq!(graphql_error_hint(&stale, &changes("src/main.rs")), "");
    }
}
