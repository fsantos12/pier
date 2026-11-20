//! Health and system endpoints.

use axum::Json;
use axum::extract::State;
use pier_api_types::{Health, SystemInfo};

use super::dto;
use crate::error::ApiError;
use crate::state::AppState;

/// Reports that the server is up and whether Docker answers. No authentication.
#[utoipa::path(
    get, path = "/api/health", tag = "system",
    responses((status = 200, description = "Server is up", body = Health))
)]
pub async fn health(State(state): State<AppState>) -> Json<Health> {
    let docker = state.docker.ping().await.is_ok();
    Json(Health {
        status: "ok".into(),
        docker,
    })
}

/// Returns Docker version and host facts for the UI header.
#[utoipa::path(
    get, path = "/api/system", tag = "system",
    responses(
        (status = 200, description = "Docker host facts", body = SystemInfo),
        (status = 401, description = "Not signed in"),
        (status = 503, description = "Docker daemon unreachable"),
    )
)]
pub async fn system(State(state): State<AppState>) -> Result<Json<SystemInfo>, ApiError> {
    let info = state.docker.system_info().await?;
    Ok(Json(dto::system_info(info, state.docker.endpoint())))
}
