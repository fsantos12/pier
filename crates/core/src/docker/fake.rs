//! In-memory [`DockerApi`] used by tests (enable the `test-util` feature).
//!
//! It mimics the Docker behaviours Pier relies on: name conflicts, 404s, `409` for
//! resources in use, no-op start/stop, and an "unreachable daemon" switch.

use std::collections::BTreeMap;
use std::sync::Mutex;

use async_trait::async_trait;

use super::DockerApi;
use crate::error::{CoreError, Result};
use crate::model::{
    Container, ContainerCreate, ContainerDetail, ContainerState, Network, NetworkAttachment,
    NetworkCreate, PruneReport, PullReport, SystemInfo, Volume, VolumeCreate,
};
use crate::validate;

#[derive(Default)]
struct State {
    networks: BTreeMap<String, Network>,
    volumes: BTreeMap<String, Volume>,
    containers: BTreeMap<String, ContainerDetail>,
    /// Volumes referenced by containers, by volume name.
    volumes_in_use: Vec<String>,
    next_id: u64,
    unreachable: bool,
}

impl State {
    fn new_id(&mut self) -> String {
        self.next_id += 1;
        format!("{:064x}", self.next_id)
    }
}

/// A configurable in-memory Docker host.
#[derive(Default)]
pub struct FakeDocker {
    state: Mutex<State>,
}

impl FakeDocker {
    /// Creates an empty fake host.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        // Invariant: tests never panic while holding the lock; recover from poisoning anyway.
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn reachable(&self) -> Result<std::sync::MutexGuard<'_, State>> {
        let state = self.lock();
        if state.unreachable {
            return Err(CoreError::Unavailable("fake daemon is unreachable".into()));
        }
        Ok(state)
    }

    /// Makes every following call fail with [`CoreError::Unavailable`] (or succeed again).
    pub fn set_unreachable(&self, unreachable: bool) {
        self.lock().unreachable = unreachable;
    }

    /// Marks a volume as used by a container so removing it conflicts.
    pub fn mark_volume_in_use(&self, name: &str) {
        self.lock().volumes_in_use.push(name.to_string());
    }

    /// Attaches a container to a network so removing the network conflicts.
    pub fn attach(&self, network: &str, container: &str) {
        let mut state = self.lock();
        if let Some(net) = state.networks.values_mut().find(|n| n.name == network) {
            net.containers.push(NetworkAttachment {
                container_id: container.to_string(),
                name: container.to_string(),
                ipv4_address: None,
            });
        }
    }

    /// Returns how many containers exist (for assertions that nothing was created).
    #[must_use]
    pub fn container_count(&self) -> usize {
        self.lock().containers.len()
    }
}

fn find_key<T>(
    map: &BTreeMap<String, T>,
    reference: &str,
    name_of: impl Fn(&T) -> &str,
) -> Option<String> {
    map.iter()
        .find(|(id, item)| name_of(item) == reference || id.starts_with(reference))
        .map(|(id, _)| id.clone())
}

#[async_trait]
impl DockerApi for FakeDocker {
    async fn ping(&self) -> Result<()> {
        self.reachable().map(|_| ())
    }

    async fn system_info(&self) -> Result<SystemInfo> {
        let _guard = self.reachable()?;
        Ok(SystemInfo {
            docker_version: "27.0.0-fake".into(),
            api_version: "1.47".into(),
            os: "linux".into(),
            arch: "amd64".into(),
            kernel_version: "6.0.0".into(),
        })
    }

    fn endpoint(&self) -> String {
        "fake://docker".into()
    }

    async fn list_networks(&self) -> Result<Vec<Network>> {
        Ok(self.reachable()?.networks.values().cloned().collect())
    }

    async fn inspect_network(&self, id: &str) -> Result<Network> {
        validate::resource_ref(id)?;
        let state = self.reachable()?;
        find_key(&state.networks, id, |n| &n.name)
            .and_then(|k| state.networks.get(&k).cloned())
            .ok_or_else(|| CoreError::NotFound(format!("network {id}")))
    }

    async fn create_network(&self, spec: NetworkCreate) -> Result<Network> {
        validate::name(&spec.name)?;
        let mut state = self.reachable()?;
        if state.networks.values().any(|n| n.name == spec.name) {
            return Err(CoreError::Conflict(format!(
                "network {} already exists",
                spec.name
            )));
        }
        let id = state.new_id();
        let network = Network {
            id: id.clone(),
            name: spec.name,
            driver: spec.driver.unwrap_or_else(|| "bridge".into()),
            scope: "local".into(),
            internal: spec.internal,
            ..Network::default()
        };
        state.networks.insert(id, network.clone());
        Ok(network)
    }

    async fn remove_network(&self, id: &str) -> Result<()> {
        validate::resource_ref(id)?;
        let mut state = self.reachable()?;
        let key = find_key(&state.networks, id, |n| &n.name)
            .ok_or_else(|| CoreError::NotFound(format!("network {id}")))?;
        if !state.networks[&key].containers.is_empty() {
            return Err(CoreError::Conflict("network has active endpoints".into()));
        }
        state.networks.remove(&key);
        Ok(())
    }

    async fn list_volumes(&self) -> Result<Vec<Volume>> {
        Ok(self.reachable()?.volumes.values().cloned().collect())
    }

    async fn inspect_volume(&self, name: &str) -> Result<Volume> {
        validate::volume_ref(name)?;
        self.reachable()?
            .volumes
            .get(name)
            .cloned()
            .ok_or_else(|| CoreError::NotFound(format!("volume {name}")))
    }

    async fn create_volume(&self, spec: VolumeCreate) -> Result<Volume> {
        validate::name(&spec.name)?;
        let mut state = self.reachable()?;
        // Mirrors the real client, which refuses duplicates although Docker would silently reuse the volume.
        if state.volumes.contains_key(&spec.name) {
            return Err(CoreError::Conflict(format!(
                "volume {} already exists",
                spec.name
            )));
        }
        let volume = Volume {
            mountpoint: format!("/var/lib/docker/volumes/{}/_data", spec.name),
            name: spec.name.clone(),
            driver: spec.driver.unwrap_or_else(|| "local".into()),
            scope: "local".into(),
            ..Volume::default()
        };
        state.volumes.insert(spec.name, volume.clone());
        Ok(volume)
    }

    async fn remove_volume(&self, name: &str) -> Result<()> {
        validate::volume_ref(name)?;
        let mut state = self.reachable()?;
        if !state.volumes.contains_key(name) {
            return Err(CoreError::NotFound(format!("volume {name}")));
        }
        if state.volumes_in_use.iter().any(|v| v == name) {
            return Err(CoreError::Conflict("volume is in use".into()));
        }
        state.volumes.remove(name);
        Ok(())
    }

    async fn prune_volumes(&self) -> Result<PruneReport> {
        let mut state = self.reachable()?;
        let in_use = state.volumes_in_use.clone();
        let doomed: Vec<String> = state
            .volumes
            .keys()
            .filter(|name| !in_use.contains(name))
            .cloned()
            .collect();
        for name in &doomed {
            state.volumes.remove(name);
        }
        Ok(PruneReport {
            volumes_deleted: doomed,
            space_reclaimed: 0,
        })
    }

    async fn list_containers(&self, all: bool) -> Result<Vec<Container>> {
        Ok(self
            .reachable()?
            .containers
            .values()
            .filter(|c| all || c.state_detail.running)
            .map(|c| c.container.clone())
            .collect())
    }

    async fn inspect_container(&self, id: &str) -> Result<ContainerDetail> {
        validate::resource_ref(id)?;
        let state = self.reachable()?;
        find_key(&state.containers, id, |c| &c.container.name)
            .and_then(|k| state.containers.get(&k).cloned())
            .ok_or_else(|| CoreError::NotFound(format!("container {id}")))
    }

    async fn create_container(&self, spec: ContainerCreate) -> Result<ContainerDetail> {
        validate::image_reference(&spec.image)?;
        validate::command(&spec.cmd)?;
        if let Some(name) = &spec.name {
            validate::name(name)?;
        }
        let mut state = self.reachable()?;
        let id = state.new_id();
        let name = spec.name.unwrap_or_else(|| format!("fake_{}", &id[60..]));
        if state.containers.values().any(|c| c.container.name == name) {
            return Err(CoreError::Conflict(format!(
                "container name {name} is already in use"
            )));
        }
        let detail = ContainerDetail {
            container: Container {
                id: id.clone(),
                name,
                image: spec.image,
                state: "created".into(),
                status: "Created".into(),
                ..Container::default()
            },
            state_detail: ContainerState {
                status: "created".into(),
                ..ContainerState::default()
            },
            cmd: spec.cmd,
            ..ContainerDetail::default()
        };
        state.containers.insert(id, detail.clone());
        Ok(detail)
    }

    async fn start_container(&self, id: &str) -> Result<()> {
        self.set_running(id, true)
    }

    async fn stop_container(&self, id: &str) -> Result<()> {
        self.set_running(id, false)
    }

    async fn restart_container(&self, id: &str) -> Result<()> {
        self.set_running(id, true)
    }

    async fn remove_container(&self, id: &str) -> Result<()> {
        validate::resource_ref(id)?;
        let mut state = self.reachable()?;
        let key = find_key(&state.containers, id, |c| &c.container.name)
            .ok_or_else(|| CoreError::NotFound(format!("container {id}")))?;
        if state.containers[&key].state_detail.running {
            return Err(CoreError::Conflict(
                "cannot remove a running container".into(),
            ));
        }
        state.containers.remove(&key);
        Ok(())
    }

    async fn pull_image(&self, reference: &str) -> Result<PullReport> {
        validate::image_reference(reference)?;
        let _guard = self.reachable()?;
        if reference.starts_with("missing") {
            return Err(CoreError::NotFound(format!("image {reference} not found")));
        }
        Ok(PullReport {
            image: reference.to_string(),
            status: "Status: Downloaded newer image".into(),
        })
    }
}

impl FakeDocker {
    fn set_running(&self, id: &str, running: bool) -> Result<()> {
        validate::resource_ref(id)?;
        let mut state = self.reachable()?;
        let key = find_key(&state.containers, id, |c| &c.container.name)
            .ok_or_else(|| CoreError::NotFound(format!("container {id}")))?;
        if let Some(detail) = state.containers.get_mut(&key) {
            // Starting a running container or stopping a stopped one is a no-op, as with Docker's 304.
            detail.state_detail.running = running;
            let label = if running { "running" } else { "exited" };
            detail.state_detail.status = label.into();
            detail.container.state = label.into();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn duplicate_network_and_volume_names_conflict_like_the_real_client() {
        let fake = FakeDocker::new();
        let net = NetworkCreate {
            name: "backend".into(),
            driver: None,
            internal: false,
            subnet: None,
            gateway: None,
        };
        fake.create_network(net.clone()).await.unwrap();
        assert!(matches!(
            fake.create_network(net).await,
            Err(CoreError::Conflict(_))
        ));
        let other = NetworkCreate {
            name: "backend2".into(),
            driver: None,
            internal: false,
            subnet: None,
            gateway: None,
        };
        assert!(
            fake.create_network(other).await.is_ok(),
            "a name that merely shares a prefix is free"
        );

        let vol = VolumeCreate {
            name: "data".into(),
            driver: None,
        };
        fake.create_volume(vol.clone()).await.unwrap();
        assert!(matches!(
            fake.create_volume(vol).await,
            Err(CoreError::Conflict(_))
        ));
    }
}
