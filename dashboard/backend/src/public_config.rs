use axum::extract::State;
use axum::Json;
use serde::Serialize;

use crate::AppState;

#[derive(Serialize)]
pub struct PublicConfig {
    pub preview_domain: String,
    pub ssh_host: String,
    pub github_org: String,
}

pub async fn get_config(State(state): State<AppState>) -> Json<PublicConfig> {
    Json(PublicConfig {
        preview_domain: state.config.preview_domain.clone(),
        ssh_host: state.config.ssh_host.clone(),
        github_org: state.config.github_org.clone(),
    })
}
