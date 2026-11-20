//! Network endpoints.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use pier_api_types::{CreateNetwork, Network};
use pier_core::validate;

use super::{ApiJson, dto};
use crate::error::ApiError;
use crate::state::AppState;

/// Lists all networks.
#[utoipa::path(get, path = "/api/networks", tag = "networks",
    responses((status = 200, body = Vec<Network>), (status = 401), (status = 503)))]
pub async fn list(State(state): State<AppState>) -> Result<Json<Vec<Network>>, ApiError> {
    let networks = state.docker.list_networks().await?;
    Ok(Json(networks.into_iter().map(dto::network).collect()))
}

/// Returns one network with its attached containers.
#[utoipa::path(get, path = "/api/networks/{id}", tag = "networks",
    params(("id" = String, Path, description = "Network id or name")),
    responses((status = 200, body = Network), (status = 400), (status = 401), (status = 404), (status = 503)))]
pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Network>, ApiError> {
    validate::resource_ref(&id)?;
    Ok(Json(dto::network(state.docker.inspect_network(&id).await?)))
}

/// Creates a network.
#[utoipa::path(post, path = "/api/networks", tag = "networks", request_body = CreateNetwork,
    responses((status = 201, body = Network), (status = 400), (status = 401), (status = 409), (status = 503)))]
pub async fn create(
    State(state): State<AppState>,
    ApiJson(body): ApiJson<CreateNetwork>,
) -> Result<(StatusCode, Json<Network>), ApiError> {
    validate::name(&body.name)?;
    let created = state
        .docker
        .create_network(dto::network_create(body))
        .await?;
    Ok((StatusCode::CREATED, Json(dto::network(created))))
}

/// Deletes a network; 409 while containers are attached.
#[utoipa::path(delete, path = "/api/networks/{id}", tag = "networks",
    params(("id" = String, Path, description = "Network id or name")),
    responses((status = 204), (status = 400), (status = 401), (status = 403), (status = 404), (status = 409), (status = 503)))]
pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    validate::resource_ref(&id)?;
    state.docker.remove_network(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}
