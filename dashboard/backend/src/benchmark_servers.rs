use axum::extract::{Path, State};
use axum::Json;

use crate::auth::AdminUser;
use crate::error::AppError;
use crate::models::{BenchmarkServer, CreateBenchmarkServerRequest, UpdateBenchmarkServerRequest};

const SERVER_QUERY: &str = "SELECT id, name, hostname, ssh_user, ssh_key_path, \
     hardware_description, status, setup_log, error_message, created_by, \
     TO_CHAR(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as created_at, \
     TO_CHAR(updated_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as updated_at \
     FROM benchmark_servers";

/// GET /api/admin/benchmark-servers
pub async fn list_servers(
    _admin: AdminUser,
    State(state): State<crate::AppState>,
) -> Result<Json<Vec<BenchmarkServer>>, AppError> {
    let servers = sqlx::query_as::<_, BenchmarkServer>(&format!("{SERVER_QUERY} ORDER BY name"))
        .fetch_all(&state.db)
        .await?;
    Ok(Json(servers))
}

/// POST /api/admin/benchmark-servers
pub async fn create_server(
    admin: AdminUser,
    State(state): State<crate::AppState>,
    Json(req): Json<CreateBenchmarkServerRequest>,
) -> Result<Json<BenchmarkServer>, AppError> {
    let name = req.name.trim().to_string();
    if name.is_empty() {
        return Err(AppError::BadRequest("Name is required".into()));
    }

    let ssh_user = req.ssh_user.unwrap_or_else(|| "root".to_string());

    sqlx::query(
        "INSERT INTO benchmark_servers (name, hostname, ssh_user, ssh_key_path, hardware_description, created_by) \
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(&name)
    .bind(&req.hostname)
    .bind(&ssh_user)
    .bind(&req.ssh_key_path)
    .bind(&req.hardware_description)
    .bind(&admin.0.sub)
    .execute(&state.db)
    .await
    .map_err(|e| {
        if e.to_string().contains("duplicate key") || e.to_string().contains("unique") {
            AppError::BadRequest(format!("A server named '{name}' already exists"))
        } else {
            AppError::Internal(format!("Failed to create server: {e}"))
        }
    })?;

    let server = sqlx::query_as::<_, BenchmarkServer>(&format!("{SERVER_QUERY} WHERE name = $1"))
        .bind(&name)
        .fetch_one(&state.db)
        .await?;

    crate::audit::log_event(
        &state.db,
        "admin.benchmark_server_create",
        &admin.0.sub,
        Some(&name),
        serde_json::json!({ "hostname": &req.hostname }),
        None,
    )
    .await;

    Ok(Json(server))
}

/// PUT /api/admin/benchmark-servers/{id}
pub async fn update_server(
    admin: AdminUser,
    State(state): State<crate::AppState>,
    Path(id): Path<i64>,
    Json(req): Json<UpdateBenchmarkServerRequest>,
) -> Result<Json<BenchmarkServer>, AppError> {
    // Build dynamic update
    let mut sets = Vec::new();
    let mut param_idx = 1u32;
    let mut values: Vec<String> = Vec::new();

    if let Some(ref name) = req.name {
        param_idx += 1;
        sets.push(format!("name = ${param_idx}"));
        values.push(name.clone());
    }
    if let Some(ref hostname) = req.hostname {
        param_idx += 1;
        sets.push(format!("hostname = ${param_idx}"));
        values.push(hostname.clone());
    }
    if let Some(ref ssh_user) = req.ssh_user {
        param_idx += 1;
        sets.push(format!("ssh_user = ${param_idx}"));
        values.push(ssh_user.clone());
    }
    if let Some(ref ssh_key_path) = req.ssh_key_path {
        param_idx += 1;
        sets.push(format!("ssh_key_path = ${param_idx}"));
        values.push(ssh_key_path.clone());
    }
    if let Some(ref hardware_description) = req.hardware_description {
        param_idx += 1;
        sets.push(format!("hardware_description = ${param_idx}"));
        values.push(hardware_description.clone());
    }

    if sets.is_empty() {
        return Err(AppError::BadRequest("No fields to update".into()));
    }

    sets.push("updated_at = NOW()".to_string());
    let sql = format!(
        "UPDATE benchmark_servers SET {} WHERE id = $1",
        sets.join(", ")
    );

    let mut query = sqlx::query(&sql).bind(id);
    for v in &values {
        query = query.bind(v);
    }
    let result = query.execute(&state.db).await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Server not found".into()));
    }

    let server = sqlx::query_as::<_, BenchmarkServer>(&format!("{SERVER_QUERY} WHERE id = $1"))
        .bind(id)
        .fetch_one(&state.db)
        .await?;

    crate::audit::log_event(
        &state.db,
        "admin.benchmark_server_update",
        &admin.0.sub,
        Some(&server.name),
        serde_json::json!({ "server_id": id }),
        None,
    )
    .await;

    Ok(Json(server))
}

/// DELETE /api/admin/benchmark-servers/{id}
pub async fn delete_server(
    admin: AdminUser,
    State(state): State<crate::AppState>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    let server_name: Option<String> =
        sqlx::query_scalar("SELECT name FROM benchmark_servers WHERE id = $1")
            .bind(id)
            .fetch_optional(&state.db)
            .await?;

    let result = sqlx::query("DELETE FROM benchmark_servers WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Server not found".into()));
    }

    if let Some(name) = server_name {
        crate::audit::log_event(
            &state.db,
            "admin.benchmark_server_delete",
            &admin.0.sub,
            Some(&name),
            serde_json::json!({ "server_id": id }),
            None,
        )
        .await;
    }

    Ok(Json(serde_json::json!({ "deleted": true })))
}

/// POST /api/admin/benchmark-servers/{id}/setup
/// Triggers provisioning of the server via SSH. Runs a setup script and records the output.
pub async fn setup_server(
    admin: AdminUser,
    State(state): State<crate::AppState>,
    Path(id): Path<i64>,
) -> Result<Json<BenchmarkServer>, AppError> {
    let server = sqlx::query_as::<_, BenchmarkServer>(&format!("{SERVER_QUERY} WHERE id = $1"))
        .bind(id)
        .fetch_one(&state.db)
        .await
        .map_err(|_| AppError::NotFound("Server not found".into()))?;

    if server.status == "busy" {
        return Err(AppError::BadRequest(
            "Server is currently busy with an autoresearch run".into(),
        ));
    }

    // Set status to provisioning
    sqlx::query("UPDATE benchmark_servers SET status = 'provisioning', error_message = NULL, setup_log = NULL, updated_at = NOW() WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;

    let db = state.db.clone();
    let hostname = server.hostname.clone();
    let ssh_user = server.ssh_user.clone();
    let ssh_key_path = server.ssh_key_path.clone();

    // Run setup in background
    tokio::spawn(async move {
        let result = run_server_setup(&hostname, &ssh_user, ssh_key_path.as_deref()).await;

        match result {
            Ok(log) => {
                let _ = sqlx::query(
                    "UPDATE benchmark_servers SET status = 'ready', setup_log = $2, error_message = NULL, updated_at = NOW() WHERE id = $1",
                )
                .bind(id)
                .bind(&log)
                .execute(&db)
                .await;
            }
            Err(e) => {
                let err_msg = format!("{e}");
                let _ = sqlx::query(
                    "UPDATE benchmark_servers SET status = 'error', error_message = $2, updated_at = NOW() WHERE id = $1",
                )
                .bind(id)
                .bind(&err_msg)
                .execute(&db)
                .await;
            }
        }
    });

    crate::audit::log_event(
        &state.db,
        "admin.benchmark_server_setup",
        &admin.0.sub,
        Some(&server.name),
        serde_json::json!({ "server_id": id }),
        None,
    )
    .await;

    // Return the server with updated status
    let updated = sqlx::query_as::<_, BenchmarkServer>(&format!("{SERVER_QUERY} WHERE id = $1"))
        .bind(id)
        .fetch_one(&state.db)
        .await?;

    Ok(Json(updated))
}

/// GET /api/admin/benchmark-servers/{id}/setup-log
pub async fn get_setup_log(
    _admin: AdminUser,
    State(state): State<crate::AppState>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    let row: (Option<String>, Option<String>, String) = sqlx::query_as(
        "SELECT setup_log, error_message, status FROM benchmark_servers WHERE id = $1",
    )
    .bind(id)
    .fetch_one(&state.db)
    .await
    .map_err(|_| AppError::NotFound("Server not found".into()))?;

    Ok(Json(serde_json::json!({
        "setup_log": row.0,
        "error_message": row.1,
        "status": row.2,
    })))
}

/// Run the setup script on a remote server via SSH.
async fn run_server_setup(
    hostname: &str,
    ssh_user: &str,
    ssh_key_path: Option<&str>,
) -> Result<String, AppError> {
    // Piped into `bash -s` over SSH stdin to avoid having the remote login
    // shell re-parse the script (which would split on embedded `;` and `|`
    // and break control flow like `if ... ; then`).
    let setup_script = r#"set -u
echo "=== Benchmark server verification ==="
echo "Hostname: $(hostname)"
echo "Date: $(date -u)"
echo

status=0
check() {
    local label="$1"; shift
    if "$@" >/dev/null 2>&1; then
        echo "[OK] $label"
    else
        echo "[MISSING] $label"
        status=1
    fi
}

# Classic shell-benchmark prerequisites
check "git binary"    command -v git
check "rsync binary"  command -v rsync
mkdir -p ~/autoresearch 2>/dev/null && echo "[OK] ~/autoresearch workspace"

# EXPB benchmark prerequisites
echo
echo "--- EXPB benchmark prerequisites ---"
check "docker binary"          command -v docker
check "passwordless sudo"      sudo -n true
check "expb binary"            command -v expb
check "overlay FS support"     grep -q overlay /proc/filesystems
# We cannot know the exact paths where the admin keeps the ethrex clone, the
# benchmarks checkout, the state snapshot, or the payload data files since
# tekton does not prescribe them — those are fields on the autoresearch run
# itself. Just remind the admin here.
echo "[INFO] Ethrex clone, benchmarks checkout, state snapshot, and payload"
echo "       data files are configured per autoresearch run. Make sure they"
echo "       exist on this server before starting an EXPB run."

echo
if [ $status -eq 0 ]; then
    echo "=== Verification complete: all prerequisites present ==="
else
    echo "=== Verification complete: some required tools are missing ==="
    echo "Hint: for 'passwordless sudo', add a file under /etc/sudoers.d/ with:"
    echo "    $(whoami) ALL=(ALL) NOPASSWD: ALL"
fi
exit $status
"#;

    let mut args: Vec<String> = vec![
        "-o".into(),
        "StrictHostKeyChecking=no".into(),
        "-o".into(),
        "LogLevel=ERROR".into(),
        "-o".into(),
        "ConnectTimeout=10".into(),
    ];
    if let Some(key) = ssh_key_path {
        args.push("-i".into());
        args.push(key.to_string());
    }
    args.push(format!("{ssh_user}@{hostname}"));
    // `bash -s` reads the script from stdin, so we don't have to quote the
    // script into a remote command line. The remote side runs it as-is.
    args.push("bash".into());
    args.push("-s".into());

    use tokio::io::AsyncWriteExt;
    use tokio::process::Command;
    let mut child = Command::new("ssh")
        .args(&args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| AppError::Internal(format!("Failed to spawn ssh: {e}")))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(setup_script.as_bytes())
            .await
            .map_err(|e| AppError::Internal(format!("Failed to pipe setup script to ssh: {e}")))?;
    }
    let output = child
        .wait_with_output()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to wait for ssh: {e}")))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let log = format!("{stdout}\n{stderr}");

    if !output.status.success() {
        return Err(AppError::Internal(format!(
            "Setup failed (exit code {:?}):\n{log}",
            output.status.code()
        )));
    }

    Ok(log)
}

// ── Non-admin endpoint for autoresearch run creation ──

/// GET /api/autoresearch/benchmark-servers — list servers with status 'ready'
pub async fn list_available_servers(
    _user: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
) -> Result<Json<Vec<BenchmarkServer>>, AppError> {
    let servers = sqlx::query_as::<_, BenchmarkServer>(&format!(
        "{SERVER_QUERY} WHERE status = 'ready' ORDER BY name"
    ))
    .fetch_all(&state.db)
    .await?;
    Ok(Json(servers))
}
