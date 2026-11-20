//! The Docker wrapper: [`DockerApi`] (the interface the server depends on) and
//! [`DockerClient`] (the bollard-backed implementation).
//!
//! Operations are implemented per resource kind in the sibling modules. Every method
//! validates its identifiers with [`crate::validate`] first, so nothing user-controlled
//! reaches a Docker URL unchecked, and converts bollard types into [`crate::model`].

mod connection;
mod containers;
mod create_lock;
#[cfg(feature = "test-util")]
pub mod fake;
mod images;
mod networks;
mod volumes;

use std::sync::Mutex;

use async_trait::async_trait;
use bollard::Docker;

pub use connection::{
    ConnectionConfig, DEFAULT_NAMED_PIPE, DEFAULT_TIMEOUT_SECS, DEFAULT_UNIX_SOCKET, Endpoint,
    TlsFiles,
};

use crate::error::{CoreError, Result};
use crate::model::{
    Container, ContainerCreate, ContainerDetail, Network, NetworkCreate, PruneReport, PullReport,
    SystemInfo, Volume, VolumeCreate,
};

/// Everything Pier can ask a Docker host to do.
///
/// The server holds an `Arc<dyn DockerApi>`, which lets tests substitute a fake and keeps
/// bollard out of every crate except this one.
#[async_trait]
pub trait DockerApi: Send + Sync {
    /// Checks that the daemon answers.
    async fn ping(&self) -> Result<()>;
    /// Returns daemon version and host facts.
    async fn system_info(&self) -> Result<SystemInfo>;
    /// Describes the endpoint this client talks to, for logs and the UI.
    fn endpoint(&self) -> String;

    /// Lists all networks.
    async fn list_networks(&self) -> Result<Vec<Network>>;
    /// Returns one network with its attached containers.
    async fn inspect_network(&self, id: &str) -> Result<Network>;
    /// Creates a network and returns it.
    async fn create_network(&self, spec: NetworkCreate) -> Result<Network>;
    /// Removes a network; fails with a conflict while containers are attached.
    async fn remove_network(&self, id: &str) -> Result<()>;

    /// Lists all volumes.
    async fn list_volumes(&self) -> Result<Vec<Volume>>;
    /// Returns one volume.
    async fn inspect_volume(&self, name: &str) -> Result<Volume>;
    /// Creates a volume and returns it.
    async fn create_volume(&self, spec: VolumeCreate) -> Result<Volume>;
    /// Removes a volume; fails with a conflict while a container uses it.
    async fn remove_volume(&self, name: &str) -> Result<()>;
    /// Removes unused volumes and reports which ones.
    async fn prune_volumes(&self) -> Result<PruneReport>;

    /// Lists containers; `all` includes stopped ones.
    async fn list_containers(&self, all: bool) -> Result<Vec<Container>>;
    /// Returns one container with detail.
    async fn inspect_container(&self, id: &str) -> Result<ContainerDetail>;
    /// Creates (without starting) a container and returns it.
    async fn create_container(&self, spec: ContainerCreate) -> Result<ContainerDetail>;
    /// Starts a container; starting a running one is a no-op.
    async fn start_container(&self, id: &str) -> Result<()>;
    /// Stops a container; stopping a stopped one is a no-op.
    async fn stop_container(&self, id: &str) -> Result<()>;
    /// Restarts a container.
    async fn restart_container(&self, id: &str) -> Result<()>;
    /// Removes a container; fails with a conflict while it is running.
    async fn remove_container(&self, id: &str) -> Result<()>;

    /// Pulls an image and waits until the pull finishes.
    async fn pull_image(&self, reference: &str) -> Result<PullReport>;
}

/// Bollard-backed [`DockerApi`] implementation.
pub struct DockerClient {
    config: ConnectionConfig,
    endpoint: Endpoint,
    /// Cached bollard client. It stays `None` while the socket does not exist yet, and
    /// every call retries, so Pier can start before the daemon and recover after it.
    client: Mutex<Option<Docker>>,
    /// Serialises check-and-create per network or volume name; see [`create_lock`].
    create_locks: create_lock::CreateLocks,
}

impl std::fmt::Debug for DockerClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DockerClient")
            .field("endpoint", &self.endpoint.describe())
            .finish_non_exhaustive()
    }
}

impl DockerClient {
    /// Builds a client from connection settings without sending any request.
    ///
    /// A daemon that is not running yet is not an error here: calls fail with
    /// [`crate::CoreError::Unavailable`] until it appears.
    ///
    /// # Errors
    /// Returns [`crate::CoreError::InvalidConfig`] when the settings are malformed or
    /// unsupported on this platform. See [`ConnectionConfig`].
    pub fn connect(config: &ConnectionConfig) -> Result<Self> {
        let endpoint = config.endpoint()?;
        let client = match connection::connect(config) {
            Ok((docker, _)) => Some(docker),
            Err(CoreError::Unavailable(reason)) => {
                tracing::warn!(%reason, endpoint = %endpoint.describe(), "docker daemon not reachable yet");
                None
            }
            Err(err) => return Err(err),
        };
        Ok(Self {
            config: config.clone(),
            endpoint,
            client: Mutex::new(client),
            create_locks: create_lock::CreateLocks::default(),
        })
    }

    /// Returns a bollard client, connecting on demand.
    pub(crate) fn docker(&self) -> Result<Docker> {
        let mut cached = self
            .client
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(docker) = cached.as_ref() {
            return Ok(docker.clone());
        }
        let (docker, _) = connection::connect(&self.config)?;
        *cached = Some(docker.clone());
        Ok(docker)
    }

    /// Returns the resolved endpoint.
    #[must_use]
    pub fn endpoint_info(&self) -> &Endpoint {
        &self.endpoint
    }
}

#[async_trait]
impl DockerApi for DockerClient {
    async fn ping(&self) -> Result<()> {
        self.docker()?.ping().await?;
        Ok(())
    }

    async fn system_info(&self) -> Result<SystemInfo> {
        Ok(self.docker()?.version().await?.into())
    }

    fn endpoint(&self) -> String {
        self.endpoint.describe()
    }

    async fn list_networks(&self) -> Result<Vec<Network>> {
        Self::networks_list(self).await
    }
    async fn inspect_network(&self, id: &str) -> Result<Network> {
        Self::networks_inspect(self, id).await
    }
    async fn create_network(&self, spec: NetworkCreate) -> Result<Network> {
        Self::networks_create(self, spec).await
    }
    async fn remove_network(&self, id: &str) -> Result<()> {
        Self::networks_remove(self, id).await
    }

    async fn list_volumes(&self) -> Result<Vec<Volume>> {
        Self::volumes_list(self).await
    }
    async fn inspect_volume(&self, name: &str) -> Result<Volume> {
        Self::volumes_inspect(self, name).await
    }
    async fn create_volume(&self, spec: VolumeCreate) -> Result<Volume> {
        Self::volumes_create(self, spec).await
    }
    async fn remove_volume(&self, name: &str) -> Result<()> {
        Self::volumes_remove(self, name).await
    }
    async fn prune_volumes(&self) -> Result<PruneReport> {
        Self::volumes_prune(self).await
    }

    async fn list_containers(&self, all: bool) -> Result<Vec<Container>> {
        Self::containers_list(self, all).await
    }
    async fn inspect_container(&self, id: &str) -> Result<ContainerDetail> {
        Self::containers_inspect(self, id).await
    }
    async fn create_container(&self, spec: ContainerCreate) -> Result<ContainerDetail> {
        Self::containers_create(self, spec).await
    }
    async fn start_container(&self, id: &str) -> Result<()> {
        Self::containers_start(self, id).await
    }
    async fn stop_container(&self, id: &str) -> Result<()> {
        Self::containers_stop(self, id).await
    }
    async fn restart_container(&self, id: &str) -> Result<()> {
        Self::containers_restart(self, id).await
    }
    async fn remove_container(&self, id: &str) -> Result<()> {
        Self::containers_remove(self, id).await
    }

    async fn pull_image(&self, reference: &str) -> Result<PullReport> {
        Self::images_pull(self, reference).await
    }
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::*;

    fn missing_socket() -> DockerClient {
        let config = ConnectionConfig {
            host: Some("unix:///nonexistent/pier-test.sock".into()),
            ..ConnectionConfig::default()
        };
        DockerClient::connect(&config).expect("a missing socket must not fail startup")
    }

    #[tokio::test]
    async fn unreachable_daemon_yields_unavailable_for_every_call() {
        let client = missing_socket();
        assert!(matches!(
            client.ping().await,
            Err(CoreError::Unavailable(_))
        ));
        assert!(matches!(
            client.list_networks().await,
            Err(CoreError::Unavailable(_))
        ));
        assert!(matches!(
            client.list_containers(true).await,
            Err(CoreError::Unavailable(_))
        ));
        assert!(matches!(
            client.prune_volumes().await,
            Err(CoreError::Unavailable(_))
        ));
    }

    #[tokio::test]
    async fn validation_runs_before_the_daemon_is_contacted() {
        let client = missing_socket();
        // InvalidInput, not Unavailable: the bad id is rejected before any connection attempt.
        assert!(matches!(
            client.inspect_container("../etc").await,
            Err(CoreError::InvalidInput(_))
        ));
    }
}
