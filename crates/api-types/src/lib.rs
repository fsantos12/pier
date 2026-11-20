//! Wire types of the Pier REST API, shared by `pier-server` and `pier-web`.
//!
//! This crate depends only on serde (and optionally `utoipa` for `OpenAPI`), so it compiles
//! to WASM unchanged. Request types use `deny_unknown_fields`: an unexpected field such
//! as `privileged` is rejected instead of silently ignored.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// JSON body of every error response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ErrorBody {
    /// Human readable, lowercase error message without secrets.
    pub error: String,
}

/// Response of `GET /api/health`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Health {
    /// Always `ok` while the server runs.
    pub status: String,
    /// Whether the Docker daemon answered a ping.
    pub docker: bool,
}

/// Response of `GET /api/setup/status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SetupStatus {
    /// `true` until the first admin user exists.
    pub needs_setup: bool,
}

/// Credentials for `POST /api/setup` and `POST /api/auth/login`.
#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Credentials {
    /// Account name.
    pub username: String,
    /// Plain-text password; never logged.
    pub password: String,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials")
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .finish()
    }
}

/// The signed-in user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct UserInfo {
    /// User id.
    pub id: i64,
    /// Account name.
    pub username: String,
    /// Role, `admin` for the only role in Phase 1.
    pub role: String,
}

/// Response of `GET /api/system`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SystemInfo {
    /// Docker engine version.
    pub docker_version: String,
    /// Docker Engine API version.
    pub api_version: String,
    /// Operating system of the Docker host.
    pub os: String,
    /// CPU architecture of the Docker host.
    pub arch: String,
    /// Kernel version of the Docker host.
    pub kernel_version: String,
    /// Endpoint Pier talks to, for example `unix:///var/run/docker.sock`.
    pub endpoint: String,
    /// Version of the Pier server.
    pub pier_version: String,
}

/// An address pool of a network.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct IpamPool {
    /// Subnet in CIDR notation.
    pub subnet: Option<String>,
    /// Gateway address.
    pub gateway: Option<String>,
}

/// A container attached to a network.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct NetworkAttachment {
    /// Full container id.
    pub container_id: String,
    /// Container name.
    pub name: String,
    /// IPv4 address with prefix length.
    pub ipv4_address: Option<String>,
}

/// A Docker network.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Network {
    /// Full network id.
    pub id: String,
    /// Network name.
    pub name: String,
    /// Driver.
    pub driver: String,
    /// Scope.
    pub scope: String,
    /// Whether external access is disabled.
    pub internal: bool,
    /// Whether standalone containers may attach in swarm scope.
    pub attachable: bool,
    /// Creation timestamp (RFC 3339).
    pub created: Option<String>,
    /// Address pools.
    pub ipam: Vec<IpamPool>,
    /// Labels.
    pub labels: BTreeMap<String, String>,
    /// Attached containers (inspect only).
    pub containers: Vec<NetworkAttachment>,
}

/// Body of `POST /api/networks`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CreateNetwork {
    /// Network name (`[a-zA-Z0-9][a-zA-Z0-9_.-]*`).
    pub name: String,
    /// `bridge` (default), `overlay`, `macvlan` or `ipvlan`.
    #[serde(default)]
    pub driver: Option<String>,
    /// Restrict external access.
    #[serde(default)]
    pub internal: bool,
    /// Subnet in CIDR notation.
    #[serde(default)]
    pub subnet: Option<String>,
    /// Gateway address; requires `subnet`.
    #[serde(default)]
    pub gateway: Option<String>,
}

/// A Docker volume.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Volume {
    /// Volume name.
    pub name: String,
    /// Driver.
    pub driver: String,
    /// Data path on the Docker host.
    pub mountpoint: String,
    /// Scope.
    pub scope: String,
    /// Creation timestamp (RFC 3339).
    pub created_at: Option<String>,
    /// Labels.
    pub labels: BTreeMap<String, String>,
}

/// Body of `POST /api/volumes`. Driver options are not accepted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CreateVolume {
    /// Volume name.
    pub name: String,
    /// Volume driver, default `local`.
    #[serde(default)]
    pub driver: Option<String>,
}

/// Response of `POST /api/volumes/prune`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PruneReport {
    /// Names of the removed volumes.
    pub volumes_deleted: Vec<String>,
    /// Bytes freed.
    pub space_reclaimed: u64,
}

/// A port of a container.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PortMapping {
    /// Container-side port.
    pub private_port: u16,
    /// Host-side port, when published.
    pub public_port: Option<u16>,
    /// `tcp`, `udp` or `sctp`.
    pub protocol: String,
    /// Host address, when published.
    pub ip: Option<String>,
}

/// A container in a list.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Container {
    /// Full container id.
    pub id: String,
    /// Container name.
    pub name: String,
    /// Image reference.
    pub image: String,
    /// State keyword such as `running` or `exited`.
    pub state: String,
    /// Human readable status.
    pub status: String,
    /// Unix creation timestamp.
    pub created: Option<i64>,
    /// Ports.
    pub ports: Vec<PortMapping>,
    /// Attached network names.
    pub networks: Vec<String>,
    /// Labels.
    pub labels: BTreeMap<String, String>,
}

/// Detailed lifecycle state of a container.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ContainerState {
    /// State keyword.
    pub status: String,
    /// Whether it is running.
    pub running: bool,
    /// Last exit code.
    pub exit_code: Option<i64>,
    /// Last start time (RFC 3339).
    pub started_at: Option<String>,
    /// Last finish time (RFC 3339).
    pub finished_at: Option<String>,
}

/// A mount of a container.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ContainerMount {
    /// `bind`, `volume` or `tmpfs`.
    pub kind: String,
    /// Volume name for volume mounts.
    pub name: Option<String>,
    /// Source path on the host.
    pub source: String,
    /// Path inside the container.
    pub destination: String,
    /// Whether the mount is writable.
    pub read_write: bool,
}

/// Response of `GET /api/containers/{id}`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ContainerDetail {
    /// List-level view.
    pub container: Container,
    /// Detailed state.
    pub state_detail: ContainerState,
    /// Entrypoint array.
    pub entrypoint: Vec<String>,
    /// Command array.
    pub cmd: Vec<String>,
    /// Restart count.
    pub restart_count: i64,
    /// Mounts.
    pub mounts: Vec<ContainerMount>,
}

/// Body of `POST /api/containers`. Only image, name and command are accepted: mounts,
/// privileges, capabilities, host namespaces and devices are rejected as unknown fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CreateContainer {
    /// Image reference.
    pub image: String,
    /// Optional container name.
    #[serde(default)]
    pub name: Option<String>,
    /// Command as an argument array (never a shell string).
    #[serde(default)]
    pub cmd: Vec<String>,
}

/// Body of `POST /api/images/pull`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PullImage {
    /// Image reference, for example `nginx:1.27`.
    pub image: String,
}

/// Response of `POST /api/images/pull`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PullResult {
    /// The pulled reference.
    pub image: String,
    /// Last status line from Docker.
    pub status: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_requests_reject_unknown_fields() {
        let privileged = r#"{"image":"alpine","privileged":true}"#;
        assert!(serde_json::from_str::<CreateContainer>(privileged).is_err());
        let mounts = r#"{"image":"alpine","host_config":{"Binds":["/:/host"]}}"#;
        assert!(serde_json::from_str::<CreateContainer>(mounts).is_err());
        let opts = r#"{"name":"v","driver_opts":{"device":"/"}}"#;
        assert!(serde_json::from_str::<CreateVolume>(opts).is_err());
        let host = r#"{"name":"n","network_mode":"host"}"#;
        assert!(serde_json::from_str::<CreateNetwork>(host).is_err());
    }

    #[test]
    fn optional_fields_default() {
        let c: CreateContainer = serde_json::from_str(r#"{"image":"alpine"}"#).unwrap();
        assert!(c.name.is_none() && c.cmd.is_empty());
    }

    #[test]
    fn credentials_debug_hides_password() {
        let c = Credentials {
            username: "a".into(),
            password: "hunter2hunter2".into(),
        };
        assert!(!format!("{c:?}").contains("hunter2"));
    }
}
