//! Image endpoints.

use axum::Json;
use axum::extract::State;
use pier_api_types::{PullImage, PullResult};
use pier_core::validate;

use super::{ApiJson, dto};
use crate::error::ApiError;
use crate::state::AppState;

/// Pulls an image and returns once the pull has finished.
///
/// The call is a plain async task, so a long pull does not block other requests.
#[utoipa::path(post, path = "/api/images/pull", tag = "images", request_body = PullImage,
    responses((status = 200, body = PullResult), (status = 400), (status = 401), (status = 404), (status = 502), (status = 503)))]
pub async fn pull(
    State(state): State<AppState>,
    ApiJson(body): ApiJson<PullImage>,
) -> Result<Json<PullResult>, ApiError> {
    validate::image_reference(&body.image)?;
    Ok(Json(dto::pull_result(
        state.docker.pull_image(&body.image).await?,
    )))
}
