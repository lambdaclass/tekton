use std::sync::Arc;

use sqlx::PgPool;

use crate::config::Config;
use crate::error::AppError;
use crate::models::IntakeSource;
use crate::tasks::TaskChannels;

// ── External issue representation ──

#[derive(Debug, Clone)]
pub struct ExternalIssue {
    pub id: String,
    pub url: String,
    pub title: String,
    pub body: String,
    pub labels: Vec<String>,
    pub updated_at: Option<String>,
}

// ── GitHub provider ──

async fn fetch_github_issues(
    source: &IntakeSource,
    api_token: &str,
) -> Result<Vec<ExternalIssue>, AppError> {
    let (owner, repo) = parse_owner_repo(&source.target_repo)?;

    let labels_param = if source.label_filter.is_empty() {
        String::new()
    } else {
        format!("&labels={}", source.label_filter.join(","))
    };

    let url = format!(
        "https://api.github.com/repos/{owner}/{repo}/issues?state=open&per_page=100{labels_param}"
    );

    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {api_token}"))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "tekton-intake")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("GitHub API request failed: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(AppError::Internal(format!(
            "GitHub API returned {status}: {body}"
        )));
    }

    let items: Vec<serde_json::Value> = resp
        .json()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to parse GitHub response: {e}")))?;

    let mut issues = Vec::new();
    for item in items {
        // Skip pull requests (GitHub returns PRs in the issues endpoint)
        if item.get("pull_request").is_some() {
            continue;
        }

        let id = item["number"].as_u64().unwrap_or(0).to_string();
        let url = item["html_url"].as_str().unwrap_or("").to_string();
        let title = item["title"].as_str().unwrap_or("").to_string();
        let body = item["body"].as_str().unwrap_or("").to_string();
        let labels: Vec<String> = item["labels"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|l| l["name"].as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let updated_at = item["updated_at"].as_str().map(String::from);

        issues.push(ExternalIssue {
            id,
            url,
            title,
            body,
            labels,
            updated_at,
        });
    }

    Ok(issues)
}

fn parse_owner_repo(target_repo: &str) -> Result<(String, String), AppError> {
    let parts: Vec<&str> = target_repo.split('/').collect();
    if parts.len() != 2 {
        return Err(AppError::BadRequest(format!(
            "Invalid repo format '{}', expected 'owner/repo'",
            target_repo
        )));
    }
    Ok((parts[0].to_string(), parts[1].to_string()))
}

// ── Prompt building ──

fn build_prompt(source: &IntakeSource, issue: &ExternalIssue) -> String {
    if let Some(ref template) = source.prompt_template {
        template
            .replace("{{number}}", &issue.id)
            .replace("{{title}}", &issue.title)
            .replace("{{body}}", &issue.body)
            .replace("{{url}}", &issue.url)
            .replace("{{labels}}", &issue.labels.join(", "))
            .replace("{{repo}}", &source.target_repo)
    } else {
        format!(
            "Implement the following based on this issue:\n\n\
             ## {title}\n\n\
             {body}\n\n\
             Source: {url}\n\n\
             Please implement the changes, write tests if appropriate, and ensure the code compiles.",
            title = issue.title,
            body = issue.body,
            url = issue.url,
        )
    }
}

// ── Polling daemon ──

pub async fn start_intake_daemon(config: Arc<Config>, db: PgPool, task_channels: TaskChannels) {
    if !config.intake_enabled {
        tracing::info!("Intake daemon disabled (INTAKE_ENABLED != true)");
        return;
    }
    tracing::info!(
        "Starting intake daemon (max global concurrent: {})",
        config.intake_max_global_concurrent
    );

    tokio::spawn(async move {
        loop {
            if let Err(e) = sync_intake_statuses(&db).await {
                tracing::error!("Intake status sync failed: {e}");
            }
            if let Err(e) = process_pending_issues(&config, &db, &task_channels).await {
                tracing::error!("Intake process-pending cycle failed: {e}");
            }
            if let Err(e) = poll_all_sources(&config, &db).await {
                tracing::error!("Intake poll cycle failed: {e}");
            }
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        }
    });
}

/// Sync intake issue statuses based on their linked task statuses.
/// - `task_created` → `review` when the task reaches `awaiting_followup` or `completed`
/// - `task_created` → `failed` when the task fails
/// - `review` → `done` when the user completes the task from the task UI
async fn sync_intake_statuses(db: &PgPool) -> Result<(), AppError> {
    let moved_to_review = sqlx::query(
        "UPDATE intake_issues SET status = 'review', updated_at = NOW() \
         WHERE status = 'task_created' \
         AND task_id IN (SELECT id FROM tasks WHERE status IN ('awaiting_followup', 'completed'))",
    )
    .execute(db)
    .await?;

    if moved_to_review.rows_affected() > 0 {
        tracing::info!(
            "Intake: moved {} issue(s) to review",
            moved_to_review.rows_affected()
        );
    }

    // When a task is completed from the task UI, auto-close the intake issue
    let moved_to_done = sqlx::query(
        "UPDATE intake_issues SET status = 'done', updated_at = NOW() \
         WHERE status = 'review' \
         AND task_id IN (SELECT id FROM tasks WHERE status = 'completed')",
    )
    .execute(db)
    .await?;

    if moved_to_done.rows_affected() > 0 {
        tracing::info!(
            "Intake: moved {} issue(s) to done (task completed)",
            moved_to_done.rows_affected()
        );
    }

    let moved_to_failed = sqlx::query(
        "UPDATE intake_issues SET status = 'failed', \
         error_message = 'Linked task failed', updated_at = NOW() \
         WHERE status = 'task_created' \
         AND task_id IN (SELECT id FROM tasks WHERE status = 'failed')",
    )
    .execute(db)
    .await?;

    if moved_to_failed.rows_affected() > 0 {
        tracing::info!(
            "Intake: moved {} issue(s) to failed",
            moved_to_failed.rows_affected()
        );
    }

    Ok(())
}

// ── Process pending issues (user-promoted) into tasks ──

/// Pick up `pending` intake issues and spawn tasks when concurrency allows.
/// Called each daemon cycle between sync and poll.
async fn process_pending_issues(
    config: &Arc<Config>,
    db: &PgPool,
    task_channels: &TaskChannels,
) -> Result<(), AppError> {
    // Check global concurrency: slots held by task_created + review
    let global_active: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM intake_issues WHERE status IN ('task_created', 'review')",
    )
    .fetch_one(db)
    .await?;

    if global_active.0 >= config.intake_max_global_concurrent as i64 {
        tracing::debug!(
            "Intake: global concurrency limit reached ({}/{}), skipping pending processing",
            global_active.0,
            config.intake_max_global_concurrent
        );
        return Ok(());
    }

    let mut global_remaining =
        config.intake_max_global_concurrent as i64 - global_active.0;

    // Fetch pending issues FIFO, joined with source config
    let pending_issues: Vec<PendingIssueRow> = sqlx::query_as(
        "SELECT i.id, i.source_id, i.prompt, \
         s.provider, s.target_repo, s.target_base_branch, s.run_as_user, \
         s.max_concurrent_tasks, s.name as source_name \
         FROM intake_issues i \
         JOIN intake_sources s ON s.id = i.source_id \
         WHERE i.status = 'pending' AND s.enabled = true \
         ORDER BY i.updated_at ASC",
    )
    .fetch_all(db)
    .await?;

    if pending_issues.is_empty() {
        return Ok(());
    }

    tracing::info!(
        "Intake: processing {} pending issue(s), global slots remaining: {}",
        pending_issues.len(),
        global_remaining
    );

    for issue in &pending_issues {
        if global_remaining <= 0 {
            break;
        }

        // Per-source concurrency check
        let source_active: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM intake_issues WHERE source_id = $1 \
             AND status IN ('task_created', 'review')",
        )
        .bind(issue.source_id)
        .fetch_one(db)
        .await?;

        if source_active.0 >= issue.max_concurrent_tasks as i64 {
            tracing::debug!(
                "Intake: source '{}' at per-source limit ({}/{}), skipping issue {}",
                issue.source_name,
                source_active.0,
                issue.max_concurrent_tasks,
                issue.id
            );
            continue;
        }

        // Decrypt API token for task spawning
        let token_row: (String,) =
            sqlx::query_as("SELECT api_token_encrypted FROM intake_sources WHERE id = $1")
                .bind(issue.source_id)
                .fetch_one(db)
                .await?;
        let _api_token =
            crate::secrets::decrypt_secret(&config.secrets_encryption_key, &token_row.0)?;

        let prompt = match &issue.prompt {
            Some(p) => p.clone(),
            None => {
                tracing::warn!(
                    "Intake: issue {} has no stored prompt, marking failed",
                    issue.id
                );
                sqlx::query(
                    "UPDATE intake_issues SET status = 'failed', \
                     error_message = 'No prompt stored', updated_at = NOW() WHERE id = $1",
                )
                .bind(issue.id)
                .execute(db)
                .await?;
                continue;
            }
        };

        let source_type = format!("intake_{}", issue.provider);
        match crate::tasks::spawn_task_internal(
            config.clone(),
            db.clone(),
            task_channels.clone(),
            crate::tasks::SpawnTaskParams {
                prompt,
                repo: issue.target_repo.clone(),
                base_branch: issue.target_base_branch.clone(),
                created_by: issue.run_as_user.clone(),
                source_type: Some(source_type),
                intake_issue_id: Some(issue.id),
            },
        )
        .await
        {
            Ok(task) => {
                sqlx::query(
                    "UPDATE intake_issues SET task_id = $1, status = 'task_created', \
                     updated_at = NOW() WHERE id = $2",
                )
                .bind(&task.id)
                .bind(issue.id)
                .execute(db)
                .await?;

                tracing::info!(
                    "Intake: created task {} for pending issue {} from source '{}'",
                    task.id,
                    issue.id,
                    issue.source_name
                );
                global_remaining -= 1;
            }
            Err(e) => {
                sqlx::query(
                    "UPDATE intake_issues SET status = 'failed', error_message = $1, \
                     updated_at = NOW() WHERE id = $2",
                )
                .bind(e.to_string())
                .bind(issue.id)
                .execute(db)
                .await?;
                tracing::error!(
                    "Intake: failed to create task for pending issue {}: {e}",
                    issue.id
                );
            }
        }
    }

    Ok(())
}

#[derive(sqlx::FromRow)]
struct PendingIssueRow {
    id: i64,
    source_id: i64,
    prompt: Option<String>,
    provider: String,
    target_repo: String,
    target_base_branch: String,
    run_as_user: String,
    max_concurrent_tasks: i32,
    source_name: String,
}

// ── Polling: fetch external issues → backlog ──

async fn poll_all_sources(config: &Arc<Config>, db: &PgPool) -> Result<(), AppError> {
    let sources = sqlx::query_as::<_, IntakeSource>(
        "SELECT id, name, provider, enabled, config, target_repo, target_base_branch, \
         label_filter, prompt_template, run_as_user, poll_interval_secs, \
         max_concurrent_tasks, max_tasks_per_poll, auto_create_pr, \
         created_by, TO_CHAR(created_at, 'YYYY-MM-DD HH24:MI:SS') as created_at, \
         TO_CHAR(updated_at, 'YYYY-MM-DD HH24:MI:SS') as updated_at \
         FROM intake_sources WHERE enabled = true",
    )
    .fetch_all(db)
    .await?;

    for source in sources {
        // Check if it's time to poll this source (compute elapsed seconds in SQL)
        let elapsed_row: Option<(i64,)> = sqlx::query_as(
            "SELECT EXTRACT(EPOCH FROM NOW() - polled_at)::bigint \
             FROM intake_poll_log \
             WHERE source_id = $1 ORDER BY polled_at DESC LIMIT 1",
        )
        .bind(source.id)
        .fetch_optional(db)
        .await?;

        let should_poll = match elapsed_row {
            Some((elapsed_secs,)) => elapsed_secs >= source.poll_interval_secs as i64,
            None => true,
        };

        if !should_poll {
            let remaining =
                source.poll_interval_secs as i64 - elapsed_row.map(|(s,)| s).unwrap_or(0);
            tracing::debug!(
                "Intake: skipping source '{}' (id={}), next poll in {}s",
                source.name,
                source.id,
                remaining
            );
            continue;
        }

        let config = config.clone();
        let db = db.clone();
        tokio::spawn(async move {
            if let Err(e) = poll_source(&config, &db, &source).await {
                tracing::error!(
                    "Failed to poll intake source '{}' (id={}): {e}",
                    source.name,
                    source.id
                );
                let _ = sqlx::query(
                    "INSERT INTO intake_poll_log (source_id, error_message, duration_ms) \
                     VALUES ($1, $2, 0)",
                )
                .bind(source.id)
                .bind(e.to_string())
                .execute(&db)
                .await;
            }
        });
    }

    Ok(())
}

/// Poll a single source: fetch external issues, deduplicate, insert new ones as `backlog`.
/// No tasks are spawned here — that happens in `process_pending_issues()`.
async fn poll_source(
    config: &Config,
    db: &PgPool,
    source: &IntakeSource,
) -> Result<(), AppError> {
    let poll_start = std::time::Instant::now();

    // Fetch the encrypted API token
    let token_row: (String,) =
        sqlx::query_as("SELECT api_token_encrypted FROM intake_sources WHERE id = $1")
            .bind(source.id)
            .fetch_one(db)
            .await?;
    let api_token = crate::secrets::decrypt_secret(&config.secrets_encryption_key, &token_row.0)?;

    // Fetch issues from provider
    let issues = match source.provider.as_str() {
        "github" => fetch_github_issues(source, &api_token).await?,
        other => {
            return Err(AppError::Internal(format!("Unknown provider: {other}")));
        }
    };

    let issues_found = issues.len() as i32;
    let mut issues_created = 0i32;

    // Batch dedup: fetch all existing external IDs for this source
    let existing_ids: std::collections::HashSet<String> = sqlx::query_as::<_, (String,)>(
        "SELECT external_id FROM intake_issues WHERE source_id = $1",
    )
    .bind(source.id)
    .fetch_all(db)
    .await?
    .into_iter()
    .map(|(id,)| id)
    .collect();

    let new_issues: Vec<_> = issues
        .iter()
        .filter(|issue| !existing_ids.contains(&issue.id))
        .take(source.max_tasks_per_poll as usize)
        .collect();

    let issues_skipped = issues_found - new_issues.len() as i32;

    for issue in &new_issues {
        let prompt = build_prompt(source, issue);

        // Insert intake_issue as backlog with pre-built prompt
        sqlx::query(
            "INSERT INTO intake_issues \
             (source_id, external_id, external_url, external_title, \
              external_body, external_labels, external_updated_at, status, prompt) \
             VALUES ($1, $2, $3, $4, $5, $6, $7::timestamptz, 'backlog', $8)",
        )
        .bind(source.id)
        .bind(&issue.id)
        .bind(&issue.url)
        .bind(&issue.title)
        .bind(&issue.body)
        .bind(&issue.labels)
        .bind(&issue.updated_at)
        .bind(&prompt)
        .execute(db)
        .await?;

        issues_created += 1;
    }

    let duration_ms = poll_start.elapsed().as_millis() as i32;
    tracing::info!(
        "Intake: polled source '{}' (id={}) in {}ms — found {}, created {}, skipped {}",
        source.name,
        source.id,
        duration_ms,
        issues_found,
        issues_created,
        issues_skipped
    );
    sqlx::query(
        "INSERT INTO intake_poll_log \
         (source_id, issues_found, issues_created, issues_skipped, duration_ms) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(source.id)
    .bind(issues_found)
    .bind(issues_created)
    .bind(issues_skipped)
    .bind(duration_ms)
    .execute(db)
    .await?;

    Ok(())
}
