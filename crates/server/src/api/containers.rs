//! Container endpoints.

use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use pier_api_types::{Container, ContainerDetail, CreateContainer};
use pier_core::validate;
use serde::Deserialize;

use super::{ApiJson, dto};
use crate::error::ApiError;
use crate::state::AppState;

/// Query string of `GET /api/containers`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListQuery {
    /// Include stopped containers (default `true`).
    all: Option<bool>,
}

/// Lists containers, stopped ones included unless `?all=false`.
#[utoipa::path(get, path = "/api/containers", tag = "containers",
    params(("all" = Option<bool>, Query, description = "Include stopped containers (default true)")),
    responses((status = 200, body = Vec<Container>), (status = 401), (status = 503)))]
pub async fn list(
    State(state): State<AppState>,
    query: Result<Query<ListQuery>, QueryRejection>,
) -> Result<Json<Vec<Container>>, ApiError> {
    let Query(query) = query.map_err(|r| ApiError::Rejected(r.status(), r.body_text()))?;
    let containers = state
        .docker
        .list_containers(query.all.unwrap_or(true))
        .await?;
    Ok(Json(containers.into_iter().map(dto::container).collect()))
}

/// Returns one container with detail.
#[utoipa::path(get, path = "/api/containers/{id}", tag = "containers",
    params(("id" = String, Path, description = "Container id or name")),
    responses((status = 200, body = ContainerDetail), (status = 400), (status = 401), (status = 404), (status = 503)))]
pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ContainerDetail>, ApiError> {
    validate::resource_ref(&id)?;
    Ok(Json(dto::container_detail(
        state.docker.inspect_container(&id).await?,
    )))
}

/// Creates (without starting) a container from an image, an optional name and a command.
#[utoipa::path(post, path = "/api/containers", tag = "containers", request_body = CreateContainer,
    responses((status = 201, body = ContainerDetail), (status = 400), (status = 401), (status = 404), (status = 409), (status = 422), (status = 503)))]
pub async fn create(
    State(state): State<AppState>,
    ApiJson(body): ApiJson<CreateContainer>,
) -> Result<(StatusCode, Json<ContainerDetail>), ApiError> {
    validate::image_reference(&body.image)?;
    validate::command(&body.cmd)?;
    if let Some(name) = &body.name {
        validate::name(name)?;
    }
    let created = state
        .docker
        .create_container(dto::container_create(body))
        .await?;
    Ok((StatusCode::CREATED, Json(dto::container_detail(created))))
}

/// Starts a container; starting a running one succeeds without change.
#[utoipa::path(post, path = "/api/containers/{id}/start", tag = "containers",
    params(("id" = String, Path, description = "Container id or name")),
    responses((status = 204), (status = 400), (status = 401), (status = 404), (status = 503)))]
pub async fn start(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    validate::resource_ref(&id)?;
    state.docker.start_container(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Stops a container; stopping a stopped one succeeds without change.
#[utoipa::path(post, path = "/api/containers/{id}/stop", tag = "containers",
    params(("id" = String, Path, description = "Container id or name")),
    responses((status = 204), (status = 400), (status = 401), (status = 404), (status = 503)))]
pub async fn stop(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    validate::resource_ref(&id)?;
    state.docker.stop_container(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Restarts a container.
#[utoipa::path(post, path = "/api/containers/{id}/restart", tag = "containers",
    params(("id" = String, Path, description = "Container id or name")),
    responses((status = 204), (status = 400), (status = 401), (status = 404), (status = 503)))]
pub async fn restart(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    validate::resource_ref(&id)?;
    state.docker.restart_container(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Removes a container; 409 while it is running.
#[utoipa::path(delete, path = "/api/containers/{id}", tag = "containers",
    params(("id" = String, Path, description = "Container id or name")),
    responses((status = 204), (status = 400), (status = 401), (status = 404), (status = 409), (status = 503)))]
pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    validate::resource_ref(&id)?;
    state.docker.remove_container(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}
