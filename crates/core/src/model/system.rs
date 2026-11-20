//! Daemon-level types.

use bollard::models::SystemVersion;
use serde::{Deserialize, Serialize};

/// Facts about the connected Docker daemon.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemInfo {
    /// Docker engine version, for example `27.3.1`.
    pub docker_version: String,
    /// Docker Engine API version the daemon speaks.
    pub api_version: String,
    /// Operating system of the Docker host (`linux`, `windows`).
    pub os: String,
    /// CPU architecture of the Docker host.
    pub arch: String,
    /// Kernel version of the Docker host, when reported.
    pub kernel_version: String,
}

impl From<SystemVersion> for SystemInfo {
    fn from(v: SystemVersion) -> Self {
        Self {
            docker_version: v.version.unwrap_or_default(),
            api_version: v.api_version.unwrap_or_default(),
            os: v.os.unwrap_or_default(),
            arch: v.arch.unwrap_or_default(),
            kernel_version: v.kernel_version.unwrap_or_default(),
        }
    }
}

/// Outcome of an image pull.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullReport {
    /// The reference that was pulled, as requested.
    pub image: String,
    /// The last status line Docker reported (for example `Status: Image is up to date`).
    pub status: String,
}
