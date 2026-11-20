//! Pier core: a Docker wrapper and the domain types shared by every Pier front end.
//!
//! This crate has no HTTP or database dependency. The server talks to Docker only
//! through [`docker::DockerApi`], and receives [`model`] types rather than raw bollard
//! structs, so a future gRPC service can reuse the same shapes.

pub mod docker;
pub mod error;
pub mod model;
pub mod validate;

pub use error::{CoreError, Error, Result};
