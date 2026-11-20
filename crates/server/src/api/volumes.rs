//! Volume endpoints.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use pier_api_types::{CreateVolume, PruneReport, Volume};
use pier_core::validate;

use super::{ApiJson, dto};
use crate::error::ApiError;
use crate::state::AppState;

/// Lists all volumes.
#[utoipa::path(get, path = "/api/volumes", tag = "volumes",
    responses((status = 200, body = Vec<Volume>), (status = 401), (status = 503)))]
pub async fn list(State(state): State<AppState>) -> Result<Json<Vec<Volume>>, ApiError> {
    let volumes = state.docker.list_volumes().await?;
    Ok(Json(volumes.into_iter().map(dto::volume).collect()))
}

/// Returns one volume.
#[utoipa::path(get, path = "/api/volumes/{name}", tag = "volumes",
    params(("name" = String, Path, description = "Volume name")),
    responses((status = 200, body = Volume), (status = 400), (status = 401), (status = 404), (status = 503)))]
pub async fn get(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<Volume>, ApiError> {
    validate::volume_ref(&name)?;
    Ok(Json(dto::volume(state.docker.inspect_volume(&name).await?)))
}

/// Creates a volume.
#[utoipa::path(post, path = "/api/volumes", tag = "volumes", request_body = CreateVolume,
    responses((status = 201, body = Volume), (status = 400), (status = 401), (status = 409), (status = 503)))]
pub async fn create(
    State(state): State<AppState>,
    ApiJson(body): ApiJson<CreateVolume>,
) -> Result<(StatusCode, Json<Volume>), ApiError> {
    validate::name(&body.name)?;
    let created = state.docker.create_volume(dto::volume_create(body)).await?;
    Ok((StatusCode::CREATED, Json(dto::volume(created))))
}

/// Deletes a volume; 409 while a container uses it.
#[utoipa::path(delete, path = "/api/volumes/{name}", tag = "volumes",
    params(("name" = String, Path, description = "Volume name")),
    responses((status = 204), (status = 400), (status = 401), (status = 404), (status = 409), (status = 503)))]
pub async fn delete(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<StatusCode, ApiError> {
    validate::volume_ref(&name)?;
    state.docker.remove_volume(&name).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Removes unused volumes and reports which ones.
#[utoipa::path(post, path = "/api/volumes/prune", tag = "volumes",
    responses((status = 200, body = PruneReport), (status = 401), (status = 503)))]
pub async fn prune(State(state): State<AppState>) -> Result<Json<PruneReport>, ApiError> {
    Ok(Json(dto::prune_report(state.docker.prune_volumes().await?)))
}
