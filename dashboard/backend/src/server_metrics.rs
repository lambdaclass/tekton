use axum::extract::{Query, State};
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::auth::AdminUser;
use crate::error::AppError;

/// Excludes loopback and the per-container `ve-*` veths: container traffic is
/// NATed out through the physical NIC, so counting veths too would double it.
const NET_DEVICES: &str = r#"device!~"lo|ve-.+""#;

#[derive(Deserialize)]
pub struct ServerMetricsQuery {
    pub range: Option<String>,
}

type Point = [f64; 2];

#[derive(Serialize)]
pub struct UsageSeries {
    pub used: Vec<Point>,
    pub total: Option<f64>,
}

#[derive(Serialize)]
pub struct TrafficSeries {
    pub rx: Vec<Point>,
    pub tx: Vec<Point>,
}

#[derive(Serialize)]
pub struct ServerMetrics {
    pub range: String,
    pub start: f64,
    pub end: f64,
    pub step: u64,
    pub memory: UsageSeries,
    pub disk: UsageSeries,
    pub network: TrafficSeries,
}

/// Maps a range to (seconds, step). Steps keep every range at ~240 points.
fn range_params(range: &str) -> Option<(u64, u64)> {
    match range {
        "1h" => Some((3_600, 15)),
        "6h" => Some((21_600, 90)),
        "24h" => Some((86_400, 360)),
        "7d" => Some((604_800, 2_520)),
        "30d" => Some((2_592_000, 10_800)),
        _ => None,
    }
}

#[derive(Deserialize)]
struct PromResponse {
    data: PromData,
}

#[derive(Deserialize)]
struct PromData {
    result: Vec<PromSeries>,
}

#[derive(Deserialize)]
struct PromSeries {
    values: Vec<(f64, String)>,
}

async fn query_range(
    client: &reqwest::Client,
    base_url: &str,
    query: &str,
    start: u64,
    end: u64,
    step: u64,
) -> Result<Vec<Point>, AppError> {
    let resp = client
        .get(format!("{base_url}/api/v1/query_range"))
        .query(&[
            ("query", query.to_string()),
            ("start", start.to_string()),
            ("end", end.to_string()),
            ("step", step.to_string()),
        ])
        .send()
        .await
        .map_err(|e| {
            tracing::warn!("Prometheus unreachable at {base_url}: {e}");
            AppError::UserError {
                code: "metrics_unavailable",
                message: "Prometheus is not reachable".into(),
            }
        })?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        tracing::error!("Prometheus query failed ({status}): {body}");
        return Err(AppError::Internal("Prometheus query failed".into()));
    }

    let parsed: PromResponse = resp.json().await?;
    Ok(parsed
        .data
        .result
        .into_iter()
        .next()
        .map(|s| {
            s.values
                .into_iter()
                .filter_map(|(t, v)| {
                    v.parse::<f64>()
                        .ok()
                        .filter(|v| v.is_finite())
                        .map(|v| [t, v])
                })
                .collect()
        })
        .unwrap_or_default())
}

fn last_value(points: &[Point]) -> Option<f64> {
    points.last().map(|p| p[1])
}

/// GET /api/admin/server-metrics?range={1h|6h|24h|7d|30d}
/// Memory, root-disk and network history for the host, read from Prometheus.
pub async fn server_metrics(
    _admin: AdminUser,
    State(state): State<crate::AppState>,
    Query(q): Query<ServerMetricsQuery>,
) -> Result<Json<ServerMetrics>, AppError> {
    let range = q.range.unwrap_or_else(|| "24h".into());
    let (span, step) = range_params(&range)
        .ok_or_else(|| AppError::BadRequest(format!("Unknown range: {range}")))?;

    let end = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| AppError::Internal(e.to_string()))?
        .as_secs();
    let start = end - span;
    // rate() needs a window covering several scrapes, and at least one step so
    // coarse ranges don't skip traffic between samples.
    let rate_window = step.max(60);

    let client = reqwest::Client::new();
    let base = state.config.prometheus_url.trim_end_matches('/');
    let q = |query: String| {
        let client = &client;
        async move { query_range(client, base, &query, start, end, step).await }
    };

    let (mem_used, mem_total, disk_used, disk_total, rx, tx) = tokio::try_join!(
        q("node_memory_MemTotal_bytes - node_memory_MemAvailable_bytes".into()),
        q("node_memory_MemTotal_bytes".into()),
        q(r#"node_filesystem_size_bytes{mountpoint="/"} - node_filesystem_avail_bytes{mountpoint="/"}"#.into()),
        q(r#"node_filesystem_size_bytes{mountpoint="/"}"#.into()),
        q(format!("sum(rate(node_network_receive_bytes_total{{{NET_DEVICES}}}[{rate_window}s]))")),
        q(format!("sum(rate(node_network_transmit_bytes_total{{{NET_DEVICES}}}[{rate_window}s]))")),
    )?;

    Ok(Json(ServerMetrics {
        range,
        start: start as f64,
        end: end as f64,
        step,
        memory: UsageSeries {
            used: mem_used,
            total: last_value(&mem_total),
        },
        disk: UsageSeries {
            used: disk_used,
            total: last_value(&disk_total),
        },
        network: TrafficSeries { rx, tx },
    }))
}
