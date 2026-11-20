//! Container types.

use std::collections::BTreeMap;

use bollard::models::{ContainerInspectResponse, ContainerSummary, MountPoint, PortSummary};
use serde::{Deserialize, Serialize};

/// A published or exposed port of a container.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortMapping {
    /// Port inside the container.
    pub private_port: u16,
    /// Port published on the host, if any.
    pub public_port: Option<u16>,
    /// Protocol (`tcp`, `udp`, `sctp`).
    pub protocol: String,
    /// Host address the port is bound to, if published.
    pub ip: Option<String>,
}

impl From<PortSummary> for PortMapping {
    fn from(p: PortSummary) -> Self {
        Self {
            private_port: p.private_port,
            public_port: p.public_port,
            protocol: p.typ.map(|t| t.to_string()).unwrap_or_default(),
            ip: p.ip,
        }
    }
}

/// Lifecycle state of a container as reported by Docker.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainerState {
    /// `created`, `running`, `paused`, `restarting`, `exited`, `removing` or `dead`.
    pub status: String,
    /// Whether the container is running right now.
    pub running: bool,
    /// Exit code of the last run.
    pub exit_code: Option<i64>,
    /// When the container last started (RFC 3339).
    pub started_at: Option<String>,
    /// When the container last finished (RFC 3339).
    pub finished_at: Option<String>,
}

/// A container as shown in lists.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Container {
    /// Full container ID.
    pub id: String,
    /// Container name without the leading slash.
    pub name: String,
    /// Image the container was created from.
    pub image: String,
    /// Lifecycle state keyword, for example `running`.
    pub state: String,
    /// Human readable status, for example `Up 2 hours`.
    pub status: String,
    /// Creation time as a Unix timestamp.
    pub created: Option<i64>,
    /// Published and exposed ports.
    pub ports: Vec<PortMapping>,
    /// Names of the networks the container is attached to.
    pub networks: Vec<String>,
    /// User and image labels.
    pub labels: BTreeMap<String, String>,
}

fn trim_name(name: &str) -> String {
    name.trim_start_matches('/').to_string()
}

impl From<ContainerSummary> for Container {
    fn from(c: ContainerSummary) -> Self {
        let mut networks: Vec<String> = c
            .network_settings
            .and_then(|n| n.networks)
            .unwrap_or_default()
            .into_keys()
            .collect();
        networks.sort();
        Self {
            id: c.id.unwrap_or_default(),
            name: c
                .names
                .and_then(|n| n.into_iter().next())
                .map(|n| trim_name(&n))
                .unwrap_or_default(),
            image: c.image.unwrap_or_default(),
            state: c.state.map(|s| s.to_string()).unwrap_or_default(),
            status: c.status.unwrap_or_default(),
            created: c.created,
            ports: c
                .ports
                .unwrap_or_default()
                .into_iter()
                .map(Into::into)
                .collect(),
            networks,
            labels: c.labels.unwrap_or_default().into_iter().collect(),
        }
    }
}

/// A mount of a container.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainerMount {
    /// Mount kind (`bind`, `volume`, `tmpfs`).
    pub kind: String,
    /// Volume name for volume mounts.
    pub name: Option<String>,
    /// Host path or volume data path.
    pub source: String,
    /// Path inside the container.
    pub destination: String,
    /// Whether the mount is writable.
    pub read_write: bool,
}

impl From<MountPoint> for ContainerMount {
    fn from(m: MountPoint) -> Self {
        Self {
            kind: m.typ.unwrap_or_default(),
            name: m.name,
            source: m.source.unwrap_or_default(),
            destination: m.destination.unwrap_or_default(),
            read_write: m.rw.unwrap_or(false),
        }
    }
}

/// A container with the extra fields shown on its detail view.
///
/// Environment variables are deliberately omitted in Phase 1 because they commonly
/// hold secrets; a later phase adds them behind an explicit reveal.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainerDetail {
    /// The list-level view of the container.
    pub container: Container,
    /// Detailed lifecycle state.
    pub state_detail: ContainerState,
    /// Entrypoint array of the container.
    pub entrypoint: Vec<String>,
    /// Command array of the container.
    pub cmd: Vec<String>,
    /// Number of times Docker restarted the container.
    pub restart_count: i64,
    /// Mounts of the container.
    pub mounts: Vec<ContainerMount>,
}

impl From<ContainerInspectResponse> for ContainerDetail {
    fn from(c: ContainerInspectResponse) -> Self {
        let config = c.config.unwrap_or_default();
        let state = c.state.unwrap_or_default();
        let status = state.status.map(|s| s.to_string()).unwrap_or_default();
        let mut networks: Vec<String> = c
            .network_settings
            .and_then(|n| n.networks)
            .unwrap_or_default()
            .into_keys()
            .collect();
        networks.sort();
        let container = Container {
            id: c.id.unwrap_or_default(),
            name: c.name.map(|n| trim_name(&n)).unwrap_or_default(),
            image: config.image.unwrap_or_default(),
            state: status.clone(),
            status: status.clone(),
            created: None,
            ports: Vec::new(),
            networks,
            labels: config.labels.unwrap_or_default().into_iter().collect(),
        };
        Self {
            container,
            state_detail: ContainerState {
                status,
                running: state.running.unwrap_or(false),
                exit_code: state.exit_code,
                started_at: state.started_at.filter(|t| !t.starts_with("0001-")),
                finished_at: state.finished_at.filter(|t| !t.starts_with("0001-")),
            },
            entrypoint: config.entrypoint.unwrap_or_default(),
            cmd: config.cmd.unwrap_or_default(),
            restart_count: c.restart_count.unwrap_or(0),
            mounts: c
                .mounts
                .unwrap_or_default()
                .into_iter()
                .map(Into::into)
                .collect(),
        }
    }
}

/// Parameters for creating a container. Phase 1 supports image, name and command only;
/// there is no way to request mounts, privileges, capabilities or host namespaces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainerCreate {
    /// Image reference to create the container from.
    pub image: String,
    /// Optional container name.
    pub name: Option<String>,
    /// Command as an argument array; empty keeps the image default.
    pub cmd: Vec<String>,
}
