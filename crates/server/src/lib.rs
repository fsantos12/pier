//! Pier server library: configuration, authentication, the SQLite store, the REST API
//! and the embedded web UI. The `pier` binary in `main.rs` is a thin wrapper around
//! [`build_app`].

pub mod api;
pub mod assets;
pub mod auth;
pub mod config;
pub mod error;
pub mod state;
pub mod store;

pub use api::build_app;
pub use state::AppState;
