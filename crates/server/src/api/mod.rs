//! The REST API: routing, the JSON extractor and the `OpenAPI` document.

mod containers;
mod dto;
mod images;
mod networks;
mod system;
mod volumes;

use axum::extract::rejection::JsonRejection;
use axum::extract::{DefaultBodyLimit, FromRequest, Request};
use axum::middleware::{from_fn, from_fn_with_state};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::de::DeserializeOwned;
use tower_http::compression::CompressionLayer;
use tower_http::trace::TraceLayer;
use utoipa::OpenApi;

use crate::error::ApiError;
use crate::state::AppState;
use crate::{assets, auth};

/// Largest accepted request body. Every API body is a few small fields.
const MAX_BODY_BYTES: usize = 64 * 1024;

/// A `Json` extractor whose failures use Pier's JSON error body.
///
/// Unknown fields (for example `privileged` on container create) end up here as 422.
#[derive(Debug)]
pub struct ApiJson<T>(pub T);

impl<S, T> FromRequest<S> for ApiJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(value)) => Ok(Self(value)),
            Err(rejection @ JsonRejection::MissingJsonContentType(_)) => Err(ApiError::Rejected(
                rejection.status(),
                "content-type must be application/json".into(),
            )),
            Err(rejection) => Err(ApiError::Rejected(
                rejection.status(),
                rejection.body_text(),
            )),
        }
    }
}

/// `OpenAPI` description of the API, served (behind login) at `GET /api/docs`.
#[derive(OpenApi)]
#[openapi(
    info(title = "Pier API", description = "Manage Docker networks, volumes and containers."),
    paths(
        system::health, system::system,
        networks::list, networks::get, networks::create, networks::delete,
        volumes::list, volumes::get, volumes::create, volumes::delete, volumes::prune,
        containers::list, containers::get, containers::create, containers::start,
        containers::stop, containers::restart, containers::delete,
        images::pull,
    ),
    tags(
        (name = "system"), (name = "networks"), (name = "volumes"),
        (name = "containers"), (name = "images"),
    )
)]
pub struct ApiDoc;

async fn openapi() -> Json<utoipa::openapi::OpenApi> {
    Json(ApiDoc::openapi())
}

async fn not_found() -> ApiError {
    ApiError::NotFound
}

/// Builds the `/api` router: public routes, then everything behind the session guard.
fn api_router(state: &AppState) -> Router<AppState> {
    let public = Router::new()
        .route("/health", get(system::health))
        .route("/setup/status", get(auth::setup_status))
        .route("/setup", post(auth::setup))
        .route("/auth/login", post(auth::login));

    let protected = Router::new()
        .route("/auth/logout", post(auth::logout))
        .route("/auth/me", get(auth::me))
        .route("/docs", get(openapi))
        .route("/system", get(system::system))
        .route("/networks", get(networks::list).post(networks::create))
        .route(
            "/networks/{id}",
            get(networks::get).delete(networks::delete),
        )
        .route("/volumes", get(volumes::list).post(volumes::create))
        .route("/volumes/prune", post(volumes::prune))
        .route("/volumes/{name}", get(volumes::get).delete(volumes::delete))
        .route(
            "/containers",
            get(containers::list).post(containers::create),
        )
        .route(
            "/containers/{id}",
            get(containers::get).delete(containers::delete),
        )
        .route("/containers/{id}/start", post(containers::start))
        .route("/containers/{id}/stop", post(containers::stop))
        .route("/containers/{id}/restart", post(containers::restart))
        .route("/images/pull", post(images::pull))
        .route_layer(from_fn_with_state(
            state.clone(),
            auth::middleware::require_auth,
        ));

    public
        .merge(protected)
        .fallback(not_found)
        .layer(from_fn(auth::middleware::csrf_guard))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
}

/// Builds the complete application: the API under `/api` and the embedded UI for
/// everything else.
pub fn build_app(state: AppState) -> Router {
    Router::new()
        .nest("/api", api_router(&state))
        .fallback(assets::serve)
        .layer(from_fn(auth::middleware::security_headers))
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
