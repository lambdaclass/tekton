mod audit;
mod auth;
mod config;
mod cost;
mod db;
mod error;
mod models;
mod policies;
mod pool;
mod previews;
mod public_config;
mod secrets;
mod settings;
mod shell;
mod tasks;
mod ws;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::Request;
use axum::http::{header, HeaderValue};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{delete, get, patch, post, put};
use axum::Router;
use sqlx::PgPool;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

use config::Config;
use tasks::TaskChannels;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: PgPool,
    pub task_channels: TaskChannels,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "dashboard=info,tower_http=info".into()),
        )
        .init();

    let config = Config::from_env()?;
    let db = db::init_pool(&config.database_url).await?;
    let listen_addr = config.listen_addr.clone();
    let static_dir = config.static_dir.clone();

    let state = AppState {
        config: Arc::new(config),
        db,
        task_channels: tasks::new_task_channels(),
    };

    let api = Router::new()
        // Public config
        .route("/config", get(public_config::get_config))
        // Auth
        .route("/auth/login", get(auth::login))
        .route("/auth/callback", get(auth::callback))
        .route("/auth/logout", post(auth::logout))
        .route("/auth/me", get(auth::me))
        // Previews
        .route("/previews", get(previews::list_previews))
        .route("/previews", post(previews::create_preview))
        .route("/previews/{slug}", delete(previews::destroy_preview))
        .route("/previews/{slug}/update", post(previews::update_preview))
        // Tasks
        .route("/tasks", get(tasks::list_tasks))
        .route("/tasks", post(tasks::create_task))
        .route("/tasks/{id}", get(tasks::get_task))
        .route("/tasks/{id}/logs", get(tasks::get_task_logs))
        .route("/tasks/{id}/subtasks", get(tasks::get_subtasks))
        .route("/tasks/{id}/messages", get(tasks::list_messages))
        .route("/tasks/{id}/messages", post(tasks::send_message))
        .route("/tasks/{id}/reopen", post(tasks::reopen_task))
        .route("/tasks/{id}/name", patch(tasks::update_task_name))
        .route("/tasks/{id}/actions", get(tasks::list_actions))
        .route("/tasks/{id}/diff", get(tasks::get_task_diff))
        .route("/tasks/{id}/create-pr", post(tasks::create_pr))
        .route("/tasks/{id}/link-pr", post(tasks::link_pr))
        // Uploads
        .route("/uploads", post(tasks::upload_image))
        // Admin: Users
        .route("/admin/users", get(auth::list_users))
        .route("/admin/users/{login}/role", put(auth::set_user_role))
        .route("/admin/users/{login}/repos", get(auth::get_user_repos))
        .route("/admin/users/{login}/repos", put(auth::set_user_repos))
        // Admin: Secrets
        .route("/admin/secrets", get(secrets::list_secrets))
        .route("/admin/secrets", post(secrets::create_secret))
        .route("/admin/secrets/{id}", delete(secrets::delete_secret))
        // Admin: Policies
        .route("/admin/policies", get(policies::list_policies))
        .route("/admin/policies", post(policies::create_policy))
        .route("/admin/policies/{id}", put(policies::update_policy))
        .route("/admin/policies/{id}", delete(policies::delete_policy))
        // Admin: Cost
        .route("/admin/cost/summary", get(cost::cost_summary))
        .route("/admin/cost/by-user", get(cost::cost_by_user))
        .route("/admin/cost/by-repo", get(cost::cost_by_repo))
        .route("/admin/cost/trends", get(cost::cost_trends))
        // Admin: Budgets
        .route("/admin/budgets", get(cost::list_budgets))
        .route("/admin/budgets", post(cost::create_budget))
        .route("/admin/budgets/{id}", put(cost::update_budget))
        .route("/admin/budgets/{id}", delete(cost::delete_budget))
        // Admin: Agent Pool
        .route("/admin/pool/status", get(pool::get_pool_status))
        .route("/admin/pool/resize", post(pool::resize_pool))
        .route("/admin/pool/refill", post(pool::refill_pool))
        // Admin: Audit Log
        .route("/admin/audit-log", get(audit::list_audit_log))
        // Settings
        .route("/settings/ai", get(settings::get_ai_settings))
        .route("/settings/ai", put(settings::put_ai_settings))
        .route("/settings/ai", delete(settings::delete_ai_settings))
        // Repos
        .route("/repos", get(tasks::list_repos))
        .route("/repos/{owner}/{repo}/branches", get(tasks::list_branches))
        // WebSockets
        .route("/ws/logs/{slug}", get(ws::preview_logs_ws))
        .route("/ws/tasks/{id}", get(ws::task_output_ws));

    // Read index.html once at startup for SPA fallback
    let index_html = std::fs::read_to_string(format!("{static_dir}/index.html"))
        .unwrap_or_else(|_| "<h1>index.html not found</h1>".into());

    // Internal routes (localhost-only, not behind /api so Caddy won't proxy them)
    let internal = Router::new()
        .route(
            "/internal/secrets/{owner}/{repo}",
            get(secrets::internal_list_secrets),
        )
        .route(
            "/internal/preview-logs/{slug}",
            get(ws::internal_preview_logs),
        );

    // Recover tasks that were in-progress before the server restarted
    tasks::recover_interrupted_tasks(
        state.config.clone(),
        state.db.clone(),
        state.task_channels.clone(),
    )
    .await;

    // Spawn background pool refill task
    {
        let config = state.config.clone();
        let interval_secs = config.agent_pool_refill_interval_secs;
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(
                std::time::Duration::from_secs(interval_secs),
            );
            // First tick fires immediately — do an initial refill
            loop {
                interval.tick().await;
                match shell::pool_refill(&config).await {
                    Ok(output) => {
                        let trimmed = output.trim();
                        if !trimmed.is_empty() {
                            tracing::debug!("Pool refill: {trimmed}");
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Pool refill failed: {e}");
                    }
                }
            }
        });
        tracing::info!(
            "Agent pool refill scheduled every {}s (target size: {})",
            state.config.agent_pool_refill_interval_secs,
            state.config.agent_pool_size,
        );
    }

    let app = Router::new()
        .nest("/api", api)
        .merge(internal)
        .fallback_service(
            ServeDir::new(&static_dir)
                .append_index_html_on_directories(true)
                .fallback(get(move || {
                    let html = index_html.clone();
                    async move { Html(html).into_response() }
                })),
        )
        .layer(middleware::from_fn(cache_headers))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&listen_addr).await?;
    tracing::info!("Dashboard listening on {listen_addr}");
    tracing::info!("Serving static files from {static_dir}");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;

    Ok(())
}

/// Middleware: hashed assets get immutable cache, HTML gets no-cache.
async fn cache_headers(request: Request, next: Next) -> Response {
    let path = request.uri().path().to_string();
    let mut response = next.run(request).await;

    if path.starts_with("/assets/") {
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=31536000, immutable"),
        );
    } else if !path.starts_with("/api/") && !path.starts_with("/ws/") {
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-cache, no-store, must-revalidate"),
        );
    }

    response
}
