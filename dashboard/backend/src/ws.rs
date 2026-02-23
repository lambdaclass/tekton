use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Path, State, WebSocketUpgrade};
use axum::response::Response;
use tokio::sync::broadcast;

use crate::shell;
use crate::AppState;

/// WebSocket endpoint for streaming preview logs.
pub async fn preview_logs_ws(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Response {
    ws.on_upgrade(move |socket| handle_preview_logs(socket, state, slug))
}

async fn handle_preview_logs(mut socket: WebSocket, state: AppState, slug: String) {
    let (tx, mut rx) = broadcast::channel::<String>(256);

    let config = state.config.clone();
    let slug2 = slug.clone();
    let stream_handle = tokio::spawn(async move {
        let _ = shell::run_cmd_streaming(
            &config.preview_bin,
            &["logs", &slug2, "--follow"],
            tx,
        )
        .await;
    });

    loop {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Ok(line) => {
                        if socket.send(Message::Text(line.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                }
            }
            // Client disconnect
            msg = socket.recv() => {
                if msg.is_none() {
                    break;
                }
                // If client sends close, break
                if let Some(Ok(Message::Close(_))) = msg {
                    break;
                }
            }
        }
    }

    stream_handle.abort();
}

/// WebSocket endpoint for streaming Claude task output.
pub async fn task_output_ws(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    ws.on_upgrade(move |socket| handle_task_output(socket, state, id))
}

async fn handle_task_output(mut socket: WebSocket, state: AppState, task_id: String) {
    // First send any existing logs from the DB
    if let Ok(logs) = sqlx::query_as::<_, crate::models::TaskLog>(
        "SELECT * FROM task_logs WHERE task_id = ? ORDER BY id ASC",
    )
    .bind(&task_id)
    .fetch_all(&state.db)
    .await
    {
        for log in logs {
            if socket.send(Message::Text(log.line.into())).await.is_err() {
                return;
            }
        }
    }

    // Then subscribe to live updates if channel exists
    let rx = state
        .task_channels
        .get(&task_id)
        .map(|entry| entry.value().subscribe());

    let Some(mut rx) = rx else {
        // No active channel — task is done or doesn't exist
        let _ = socket.send(Message::Close(None)).await;
        return;
    };

    loop {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Ok(line) => {
                        if socket.send(Message::Text(line.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        let _ = socket.send(Message::Close(None)).await;
                        break;
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                }
            }
            msg = socket.recv() => {
                if msg.is_none() {
                    break;
                }
                if let Some(Ok(Message::Close(_))) = msg {
                    break;
                }
            }
        }
    }
}
