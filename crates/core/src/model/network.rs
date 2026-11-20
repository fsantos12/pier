//! Network types.

use std::collections::BTreeMap;

use bollard::models::{Ipam, Network as BollardNetwork, NetworkInspect};
use serde::{Deserialize, Serialize};

/// An address pool of a network.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IpamPool {
    /// Subnet in CIDR notation.
    pub subnet: Option<String>,
    /// Gateway address.
    pub gateway: Option<String>,
}

/// A container attached to a network (only filled by inspect).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkAttachment {
    /// Full container ID.
    pub container_id: String,
    /// Container name.
    pub name: String,
    /// IPv4 address with prefix length, when assigned.
    pub ipv4_address: Option<String>,
}

/// A Docker network.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Network {
    /// Full network ID.
    pub id: String,
    /// Network name.
    pub name: String,
    /// Driver (`bridge`, `overlay`, ...).
    pub driver: String,
    /// Scope (`local`, `swarm`, `global`).
    pub scope: String,
    /// Whether external access is disabled.
    pub internal: bool,
    /// Whether standalone containers may attach in swarm scope.
    pub attachable: bool,
    /// Creation timestamp as reported by Docker (RFC 3339), when known.
    pub created: Option<String>,
    /// Configured address pools.
    pub ipam: Vec<IpamPool>,
    /// User and system labels.
    pub labels: BTreeMap<String, String>,
    /// Attached containers; empty for list results.
    pub containers: Vec<NetworkAttachment>,
}

fn pools(ipam: Option<Ipam>) -> Vec<IpamPool> {
    ipam.and_then(|i| i.config)
        .unwrap_or_default()
        .into_iter()
        .map(|c| IpamPool {
            subnet: c.subnet,
            gateway: c.gateway,
        })
        .collect()
}

impl From<BollardNetwork> for Network {
    fn from(n: BollardNetwork) -> Self {
        Self {
            id: n.id.unwrap_or_default(),
            name: n.name.unwrap_or_default(),
            driver: n.driver.unwrap_or_default(),
            scope: n.scope.unwrap_or_default(),
            internal: n.internal.unwrap_or(false),
            attachable: n.attachable.unwrap_or(false),
            created: n.created,
            ipam: pools(n.ipam),
            labels: n.labels.unwrap_or_default().into_iter().collect(),
            containers: Vec::new(),
        }
    }
}

impl From<NetworkInspect> for Network {
    fn from(n: NetworkInspect) -> Self {
        let mut containers: Vec<NetworkAttachment> = n
            .containers
            .unwrap_or_default()
            .into_iter()
            .map(|(id, c)| NetworkAttachment {
                container_id: id,
                name: c.name.unwrap_or_default(),
                ipv4_address: c.ipv4_address.filter(|a| !a.is_empty()),
            })
            .collect();
        containers.sort_by(|a, b| a.name.cmp(&b.name));
        Self {
            id: n.id.unwrap_or_default(),
            name: n.name.unwrap_or_default(),
            driver: n.driver.unwrap_or_default(),
            scope: n.scope.unwrap_or_default(),
            internal: n.internal.unwrap_or(false),
            attachable: n.attachable.unwrap_or(false),
            created: n.created,
            ipam: pools(n.ipam),
            labels: n.labels.unwrap_or_default().into_iter().collect(),
            containers,
        }
    }
}

/// Parameters for creating a network.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkCreate {
    /// Name of the new network; must pass [`crate::validate::name`].
    pub name: String,
    /// Driver; `None` means `bridge`.
    pub driver: Option<String>,
    /// Restrict external access.
    pub internal: bool,
    /// Optional subnet in CIDR notation.
    pub subnet: Option<String>,
    /// Optional gateway; requires a subnet.
    pub gateway: Option<String>,
}
