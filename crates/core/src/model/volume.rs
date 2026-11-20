//! Volume types.

use std::collections::BTreeMap;

use bollard::models::{Volume as BollardVolume, VolumePruneResponse};
use serde::{Deserialize, Serialize};

/// A Docker volume.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Volume {
    /// Volume name (its identifier).
    pub name: String,
    /// Volume driver, usually `local`.
    pub driver: String,
    /// Path of the volume data on the Docker host.
    pub mountpoint: String,
    /// Scope, `local` or `global`.
    pub scope: String,
    /// Creation timestamp as reported by Docker (RFC 3339), when known.
    pub created_at: Option<String>,
    /// User and system labels.
    pub labels: BTreeMap<String, String>,
}

impl From<BollardVolume> for Volume {
    fn from(v: BollardVolume) -> Self {
        Self {
            name: v.name,
            driver: v.driver,
            mountpoint: v.mountpoint,
            scope: v
                .scope
                .map(|s| s.to_string().to_lowercase())
                .unwrap_or_default(),
            created_at: v.created_at,
            labels: v.labels.into_iter().collect(),
        }
    }
}

/// Parameters for creating a volume. Driver options are intentionally absent: the
/// `local` driver can bind arbitrary host paths through them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VolumeCreate {
    /// Name of the new volume; must pass [`crate::validate::name`].
    pub name: String,
    /// Driver; `None` means the daemon default (`local`).
    pub driver: Option<String>,
}

/// Result of pruning unused volumes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PruneReport {
    /// Names of the volumes that were removed.
    pub volumes_deleted: Vec<String>,
    /// Disk space freed, in bytes.
    pub space_reclaimed: u64,
}

impl From<VolumePruneResponse> for PruneReport {
    fn from(r: VolumePruneResponse) -> Self {
        Self {
            volumes_deleted: r.volumes_deleted.unwrap_or_default(),
            space_reclaimed: r
                .space_reclaimed
                .and_then(|b| u64::try_from(b).ok())
                .unwrap_or(0),
        }
    }
}
