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
    let setup_script = r#"
set -euo pipefail
echo "=== Benchmark server setup ==="
echo "Hostname: $(hostname)"
echo "Date: $(date -u)"

# Check basic tools
for cmd in git rsync; do
    if command -v $cmd &>/dev/null; then
        echo "[OK] $cmd: $(command -v $cmd)"
    else
        echo "[INSTALL] Installing $cmd..."
        if command -v apt-get &>/dev/null; then
            sudo apt-get update -qq && sudo apt-get install -y -qq $cmd
        elif command -v dnf &>/dev/null; then
            sudo dnf install -y -q $cmd
        elif command -v nix-env &>/dev/null; then
            nix-env -iA nixpkgs.$cmd
        else
            echo "[ERROR] Cannot install $cmd — unknown package manager"
            exit 1
        fi
        echo "[OK] $cmd installed"
    fi
done

# Create workspace directory for autoresearch runs
mkdir -p ~/autoresearch
echo "[OK] ~/autoresearch workspace ready"

echo ""
echo "=== Setup complete ==="
"#;

    let mut args = vec![
        "-o",
        "StrictHostKeyChecking=no",
        "-o",
        "LogLevel=ERROR",
        "-o",
        "ConnectTimeout=10",
    ];
    if let Some(key) = ssh_key_path {
        args.extend_from_slice(&["-i", key]);
    }
    let target = format!("{ssh_user}@{hostname}");
    args.push(&target);
    args.push("bash");
    args.push("-c");

    let output = tokio::process::Command::new("ssh")
        .args(&args)
        .arg(setup_script)
        .output()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to SSH to server: {e}")))?;

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
