//! Domain types returned by [`crate::docker::DockerApi`].
//!
//! These are plain serde structs that hide bollard's generated models, so callers
//! (the HTTP server today, gRPC later) share one stable shape. Conversions from the
//! bollard types live next to each type.

mod container;
mod network;
mod system;
mod volume;

pub use container::{
    Container, ContainerCreate, ContainerDetail, ContainerMount, ContainerState, PortMapping,
};
pub use network::{IpamPool, Network, NetworkAttachment, NetworkCreate};
pub use system::{PullReport, SystemInfo};
pub use volume::{PruneReport, Volume, VolumeCreate};
