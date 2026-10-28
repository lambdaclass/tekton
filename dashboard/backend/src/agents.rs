//! Agent container lifecycle once the agent has started working: keeping containers whose work
//! hasn't reached GitHub, snapshotting that work before every destroy, and reaping leftovers.

use axum::extract::{Path, State};
use axum::Json;
use serde::Serialize;
use sqlx::PgPool;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::auth::{AdminUser, MemberUser};
use crate::config::Config;
use crate::error::AppError;
use crate::shell;
use crate::tasks::{save_system_message, update_task_status};

const REAP_INTERVAL: Duration = Duration::from_secs(15 * 60);

/// Statuses in which the container may hold agent work that isn't on GitHub yet.
pub fn holds_agent_work(status: &str) -> bool {
    matches!(
        status,
        "running_claude" | "pushing" | "creating_preview" | "awaiting_followup" | "push_failed"
    )
}

fn is_final(status: &str) -> bool {
    status == "completed" || status == "failed"
}

/// Keep the task's container for `hours` from now.
pub async fn retain(db: &PgPool, task_id: &str, hours: i64) {
    if let Err(e) = sqlx::query(
        "UPDATE tasks SET agent_retained_until = NOW() + ($1::FLOAT8 * INTERVAL '1 hour') \
         WHERE id = $2",
    )
    .bind(hours as f64)
    .bind(task_id)
    .execute(db)
    .await
    {
        tracing::warn!("Failed to set container retention for task {task_id}: {e}");
    }
}

pub async fn clear_retention(db: &PgPool, task_id: &str) {
    if let Err(e) = sqlx::query("UPDATE tasks SET agent_retained_until = NULL WHERE id = $1")
        .bind(task_id)
        .execute(db)
        .await
    {
        tracing::warn!("Failed to clear container retention for task {task_id}: {e}");
    }
}

/// Copy the agent's work to `<snapshot_dir>/<task_id>/<unix time>/`: the commits ahead of the
/// base branch as a git bundle, and uncommitted plus untracked changes as a binary diff.
pub async fn snapshot_agent(
    config: &Config,
    task_id: &str,
    agent_name: &str,
    base_branch: &str,
) -> Result<PathBuf, AppError> {
    let base = base_branch.replace('\'', "'\\''");
    let cmd = format!(
        "set -e; rm -rf /tmp/snapshot /tmp/snapshot.index && mkdir /tmp/snapshot && \
         cd /home/agent/repo && \
         (git log --oneline 'origin/{base}..HEAD' > /tmp/snapshot/commits.txt 2>&1 || true) && \
         (git bundle create /tmp/snapshot/commits.bundle HEAD '^origin/{base}' 2>/dev/null || true) && \
         cp .git/index /tmp/snapshot.index && \
         GIT_INDEX_FILE=/tmp/snapshot.index git add -A && \
         GIT_INDEX_FILE=/tmp/snapshot.index git diff --cached --binary HEAD > /tmp/snapshot/uncommitted.diff && \
         rm -f /tmp/snapshot.index"
    );
    shell::agent_exec_capture(agent_name, &cmd).await?;

    let task_dir = PathBuf::from(&config.snapshot_dir).join(task_id);
    tokio::fs::create_dir_all(&task_dir)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to create {}: {e}", task_dir.display())))?;
    let dest = task_dir.join(chrono::Utc::now().timestamp().to_string());
    shell::scp_from_agent(agent_name, "/tmp/snapshot", &dest.to_string_lossy()).await?;
    Ok(dest)
}

/// Snapshot the agent's work, then destroy its container. Every destroy of a container that may
/// hold work goes through here. Failures are logged rather than returned: a failed snapshot must
/// not keep a container alive forever, and a failed destroy is left for the reaper.
pub async fn snapshot_and_destroy(
    config: &Config,
    db: &PgPool,
    task_id: &str,
    agent_name: &str,
    base_branch: &str,
) {
    match snapshot_agent(config, task_id, agent_name, base_branch).await {
        Ok(dest) => tracing::info!("Saved snapshot of {agent_name} to {}", dest.display()),
        Err(e) => tracing::warn!("Failed to snapshot {agent_name} for task {task_id}: {e}"),
    }
    destroy(config, db, task_id, agent_name).await;
}

/// Destroy without a snapshot, for containers the agent never worked in.
pub async fn destroy(config: &Config, db: &PgPool, task_id: &str, agent_name: &str) {
    if let Err(e) = shell::destroy_agent(config, agent_name).await {
        tracing::warn!("Failed to destroy agent {agent_name} for task {task_id}: {e}");
    }
    clear_retention(db, task_id).await;
}

/// Mark a task failed after its pipeline errored. If the agent had started working, keep the
/// container for the retention window so the work can be recovered (Reopen reuses it);
/// otherwise there's nothing to save, so destroy it now.
pub async fn fail_task(config: &Config, db: &PgPool, task_id: &str, error: &str) {
    let row: Option<(String, Option<String>)> =
        sqlx::query_as("SELECT status, agent_name FROM tasks WHERE id = $1")
            .bind(task_id)
            .fetch_optional(db)
            .await
            .unwrap_or_else(|e| {
                tracing::warn!("Failed to load task {task_id} while failing it: {e}");
                None
            });
    if let Err(e) = update_task_status(db, task_id, "failed", Some(error)).await {
        tracing::warn!("Failed to mark task {task_id} failed: {e}");
    }
    let Some((status, Some(agent_name))) = row else {
        return;
    };
    if holds_agent_work(&status) {
        let hours = config.agent_retention_failed_hours;
        retain(db, task_id, hours).await;
        let _ = save_system_message(
            db,
            task_id,
            &format!(
                "The agent's container is kept for {hours}h so its work isn't lost. Reopen the \
                 task to continue where it stopped, or discard the container to free its space."
            ),
        )
        .await;
    } else {
        destroy(config, db, task_id, &agent_name).await;
    }
}

// ── Reaper ──

pub fn spawn_reaper(config: Arc<Config>, db: PgPool) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(REAP_INTERVAL);
        loop {
            interval.tick().await;
            reap(&config, &db).await;
        }
    });
}

async fn reap(config: &Config, db: &PgPool) {
    // Claim expired containers by clearing their retention first, so a concurrent discard
    // doesn't destroy the same container twice.
    let expired: Vec<(String, String, String)> = match sqlx::query_as(
        "UPDATE tasks SET agent_retained_until = NULL \
         WHERE status IN ('completed', 'failed') AND agent_retained_until < NOW() \
         AND agent_name IS NOT NULL \
         RETURNING id, agent_name, base_branch",
    )
    .fetch_all(db)
    .await
    {
        Ok(rows) => rows,
        Err(e) => {
            tracing::warn!("Reaper: failed to claim expired containers: {e}");
            Vec::new()
        }
    };
    for (task_id, agent_name, base_branch) in expired {
        tracing::info!("Reaper: retention expired for {agent_name} (task {task_id})");
        snapshot_and_destroy(config, db, &task_id, &agent_name, &base_branch).await;
    }

    reap_leftover_containers(config, db).await;

    let dir = PathBuf::from(&config.snapshot_dir);
    let max_age = Duration::from_secs(config.snapshot_retention_days * 24 * 3600);
    if let Err(e) = tokio::task::spawn_blocking(move || prune_snapshots(&dir, max_age)).await {
        tracing::warn!("Reaper: snapshot pruning panicked: {e}");
    }
}

#[derive(sqlx::FromRow)]
struct ContainerOwner {
    id: String,
    status: String,
    base_branch: String,
    /// Finished, not kept on purpose, and untouched for an hour.
    stale: bool,
}

/// The tasks a `task-*` container may belong to: the ones that last used it, or the one its
/// name was derived from, which covers the moment between creating it and saving agent_name.
async fn container_owners(db: &PgPool, agent_name: &str) -> Result<Vec<ContainerOwner>, AppError> {
    let short_id = agent_name.strip_prefix("task-").unwrap_or(agent_name);
    Ok(sqlx::query_as(
        "SELECT id, status, base_branch, \
         COALESCE(status IN ('completed', 'failed') AND agent_retained_until IS NULL \
          AND updated_at < NOW() - INTERVAL '1 hour', false) AS stale \
         FROM tasks WHERE agent_name = $1 OR id LIKE $2 \
         ORDER BY updated_at DESC",
    )
    .bind(agent_name)
    .bind(format!("{short_id}%"))
    .fetch_all(db)
    .await?)
}

/// Destroy `task-*` containers no live or kept task owns: leftovers from failed destroys,
/// crashes, or containers whose task row is gone.
async fn reap_leftover_containers(config: &Config, db: &PgPool) {
    let names = match shell::list_agents(config).await {
        Ok(names) => names,
        Err(e) => {
            tracing::warn!("Reaper: failed to list agents: {e}");
            return;
        }
    };
    for name in names.iter().filter(|n| n.starts_with("task-")) {
        let owners = match container_owners(db, name).await {
            Ok(owners) => owners,
            Err(e) => {
                tracing::warn!("Reaper: failed to look up owner of {name}: {e}");
                continue;
            }
        };
        if !owners.iter().all(|o| o.stale) {
            continue;
        }
        tracing::info!("Reaper: destroying leftover container {name}");
        match owners.first() {
            Some(owner) => {
                snapshot_and_destroy(config, db, &owner.id, name, &owner.base_branch).await
            }
            None => {
                if let Err(e) = shell::destroy_agent(config, name).await {
                    tracing::warn!("Reaper: failed to destroy {name}: {e}");
                }
            }
        }
    }
}

/// Delete `<dir>/<task_id>/<snapshot>` directories older than `max_age`, then empty task dirs.
fn prune_snapshots(dir: &std::path::Path, max_age: Duration) {
    let Ok(tasks) = std::fs::read_dir(dir) else {
        return;
    };
    for task_dir in tasks.flatten().map(|e| e.path()).filter(|p| p.is_dir()) {
        let Ok(snapshots) = std::fs::read_dir(&task_dir) else {
            continue;
        };
        for snapshot in snapshots.flatten().map(|e| e.path()) {
            let expired = std::fs::metadata(&snapshot)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age > max_age);
            if expired {
                if let Err(e) = std::fs::remove_dir_all(&snapshot) {
                    tracing::warn!("Failed to prune snapshot {}: {e}", snapshot.display());
                }
            }
        }
        // Only succeeds once the directory is empty.
        let _ = std::fs::remove_dir(&task_dir);
    }
}

// ── Manual discard ──

/// POST /api/tasks/{id}/discard-agent — snapshot and destroy a finished task's kept container
/// now, to free its space before the retention window runs out.
pub async fn discard_agent(
    user: MemberUser,
    State(state): State<crate::AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    crate::auth::check_task_ownership(
        &state.db,
        &id,
        &user.0.sub,
        &user.0.role,
        &state.config.github_org,
    )
    .await?;

    let claimed: Option<(String, String)> = sqlx::query_as(
        "UPDATE tasks SET agent_retained_until = NULL \
         WHERE id = $1 AND status IN ('completed', 'failed') \
         AND agent_retained_until IS NOT NULL AND agent_name IS NOT NULL \
         RETURNING agent_name, base_branch",
    )
    .bind(&id)
    .fetch_optional(&state.db)
    .await?;
    let Some((agent_name, base_branch)) = claimed else {
        return Err(AppError::BadRequest(
            "This task has no kept container to discard".into(),
        ));
    };

    snapshot_and_destroy(&state.config, &state.db, &id, &agent_name, &base_branch).await;
    let _ = save_system_message(
        &state.db,
        &id,
        "The agent's container was discarded. A snapshot of its work was saved on the server.",
    )
    .await;
    crate::audit::log_event(
        &state.db,
        "task.agent_discarded",
        &user.0.sub,
        Some(&id),
        serde_json::json!({ "agent_name": &agent_name }),
        None,
    )
    .await;

    Ok(Json(serde_json::json!({ "ok": true })))
}

// ── Admin ──

#[derive(Serialize)]
pub struct AgentEntry {
    name: String,
    task_id: Option<String>,
    task_name: Option<String>,
    task_status: Option<String>,
    retained_until: Option<String>,
}

/// GET /api/admin/agents — every agent container, with the task that owns it.
pub async fn list_agents(
    _admin: AdminUser,
    State(state): State<crate::AppState>,
) -> Result<Json<Vec<AgentEntry>>, AppError> {
    let names = shell::list_agents(&state.config).await?;
    let mut entries = Vec::with_capacity(names.len());
    for name in names {
        let task: Option<(String, Option<String>, String, Option<String>)> = sqlx::query_as(
            "SELECT id, name, status, \
             TO_CHAR(agent_retained_until, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') \
             FROM tasks WHERE agent_name = $1 ORDER BY updated_at DESC LIMIT 1",
        )
        .bind(&name)
        .fetch_optional(&state.db)
        .await?;
        let (task_id, task_name, task_status, retained_until) = match task {
            Some((id, n, s, r)) => (Some(id), n, Some(s), r),
            None => (None, None, None, None),
        };
        entries.push(AgentEntry {
            name,
            task_id,
            task_name,
            task_status,
            retained_until,
        });
    }
    Ok(Json(entries))
}

/// POST /api/admin/agents/{name}/destroy — snapshot and destroy a container whose task is
/// finished (or gone). Live tasks are refused: mark them done instead.
pub async fn admin_destroy_agent(
    admin: AdminUser,
    State(state): State<crate::AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !shell::list_agents(&state.config).await?.contains(&name) {
        return Err(AppError::NotFound(format!(
            "No agent container named {name}"
        )));
    }
    let owners = container_owners(&state.db, &name).await?;
    if let Some(live) = owners.iter().find(|o| !is_final(&o.status)) {
        return Err(AppError::BadRequest(format!(
            "Task {} is still {} in this container; mark it done instead",
            live.id, live.status
        )));
    }
    match owners.first() {
        Some(owner) => {
            snapshot_and_destroy(
                &state.config,
                &state.db,
                &owner.id,
                &name,
                &owner.base_branch,
            )
            .await
        }
        None => shell::destroy_agent(&state.config, &name)
            .await
            .map(|_| ())?,
    }
    crate::audit::log_event(
        &state.db,
        "agent.destroyed",
        &admin.0.sub,
        owners.first().map(|o| o.id.as_str()),
        serde_json::json!({ "agent_name": &name }),
        None,
    )
    .await;
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_holding_statuses() {
        for s in [
            "running_claude",
            "pushing",
            "creating_preview",
            "awaiting_followup",
            "push_failed",
        ] {
            assert!(holds_agent_work(s), "{s}");
        }
        for s in [
            "pending",
            "creating_agent",
            "cloning",
            "completed",
            "failed",
        ] {
            assert!(!holds_agent_work(s), "{s}");
        }
    }

    #[test]
    fn prune_removes_only_old_snapshots() {
        let root = std::env::temp_dir().join(format!("snapshots-test-{}", std::process::id()));
        let fresh = root.join("task-a").join("200");
        let old = root.join("task-b").join("100");
        std::fs::create_dir_all(&fresh).unwrap();
        std::fs::create_dir_all(&old).unwrap();
        let past = std::time::SystemTime::now() - Duration::from_secs(3600);
        std::fs::File::open(&old)
            .unwrap()
            .set_modified(past)
            .unwrap();

        prune_snapshots(&root, Duration::from_secs(60));

        assert!(fresh.exists());
        assert!(!old.exists());
        assert!(!root.join("task-b").exists(), "empty task dir is removed");
        std::fs::remove_dir_all(&root).unwrap();
    }
}
