//! Network operations.

use bollard::models::{Ipam, IpamConfig, NetworkCreateRequest};

use super::DockerClient;
use super::create_lock::Kind;
use crate::error::{CoreError, Result};
use crate::model::{Network, NetworkCreate};
use crate::validate;

impl DockerClient {
    pub(super) async fn networks_list(&self) -> Result<Vec<Network>> {
        let networks = self.docker()?.list_networks(None).await?;
        Ok(networks.into_iter().map(Into::into).collect())
    }

    pub(super) async fn networks_inspect(&self, id: &str) -> Result<Network> {
        validate::resource_ref(id)?;
        Ok(self.docker()?.inspect_network(id, None).await?.into())
    }

    pub(super) async fn networks_create(&self, spec: NetworkCreate) -> Result<Network> {
        let request = build_create_request(&spec)?;
        // Docker happily creates several networks with the same name. List and compare
        // exact names ourselves: Docker's name filter also matches partial names and id
        // prefixes. Docker has no atomic "create unless exists", so the per-name lock
        // keeps parallel creates through this client from all passing the check.
        let _guard = self.create_locks.acquire(Kind::Network, &spec.name).await;
        ensure_name_free(&self.networks_list().await?, &spec.name)?;
        let created = self.docker()?.create_network(request).await?;
        // The create response only carries the ID; inspect for the full object.
        self.networks_inspect(&created.id).await
    }

    pub(super) async fn networks_remove(&self, id: &str) -> Result<()> {
        validate::resource_ref(id)?;
        Ok(self.docker()?.remove_network(id).await?)
    }
}

/// Fails with [`CoreError::Conflict`] when a network named exactly `name` already exists.
///
/// Only the `name` field is compared, case-sensitively and in full, so neither an id
/// prefix nor a longer or shorter name can collide.
pub(crate) fn ensure_name_free(existing: &[Network], name: &str) -> Result<()> {
    if existing.iter().any(|n| n.name == name) {
        return Err(CoreError::Conflict(format!(
            "network {name} already exists"
        )));
    }
    Ok(())
}

/// Validates a [`NetworkCreate`] and converts it into the bollard request.
pub(crate) fn build_create_request(spec: &NetworkCreate) -> Result<NetworkCreateRequest> {
    validate::name(&spec.name)?;
    let driver = spec.driver.as_deref().unwrap_or("bridge");
    validate::network_driver(driver)?;
    if let Some(subnet) = &spec.subnet {
        validate::cidr(subnet)?;
    }
    if let Some(gateway) = &spec.gateway {
        validate::ip_address(gateway)?;
        if spec.subnet.is_none() {
            return Err(CoreError::InvalidInput("a gateway needs a subnet".into()));
        }
    }
    let ipam = spec.subnet.as_ref().map(|subnet| Ipam {
        driver: Some("default".to_string()),
        config: Some(vec![IpamConfig {
            subnet: Some(subnet.clone()),
            gateway: spec.gateway.clone(),
            ..IpamConfig::default()
        }]),
        options: None,
    });
    Ok(NetworkCreateRequest {
        name: spec.name.clone(),
        driver: Some(driver.to_string()),
        internal: Some(spec.internal),
        ipam,
        ..NetworkCreateRequest::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CoreError;

    fn spec(name: &str) -> NetworkCreate {
        NetworkCreate {
            name: name.into(),
            driver: None,
            internal: false,
            subnet: None,
            gateway: None,
        }
    }

    #[test]
    fn defaults_to_bridge() {
        let req = build_create_request(&spec("web")).unwrap();
        assert_eq!(req.driver.as_deref(), Some("bridge"));
        assert!(req.ipam.is_none());
    }

    #[test]
    fn rejects_bad_input() {
        assert!(matches!(
            build_create_request(&spec("a/b")),
            Err(CoreError::InvalidInput(_))
        ));
        let mut s = spec("web");
        s.driver = Some("host".into());
        assert!(build_create_request(&s).is_err());
        let mut s = spec("web");
        s.gateway = Some("10.0.0.1".into());
        assert!(build_create_request(&s).is_err(), "gateway without subnet");
    }

    fn net(id: &str, name: &str) -> Network {
        Network {
            id: id.into(),
            name: name.into(),
            ..Network::default()
        }
    }

    #[test]
    fn duplicate_network_names_conflict_on_exact_match_only() {
        let existing = [net("abcdef0123456789", "backend"), net("ffff", "backend-2")];
        assert!(matches!(
            ensure_name_free(&existing, "backend"),
            Err(CoreError::Conflict(_))
        ));
        assert!(
            ensure_name_free(&existing, "back").is_ok(),
            "a prefix of a name is free"
        );
        assert!(
            ensure_name_free(&existing, "backend-22").is_ok(),
            "a longer name is free"
        );
        assert!(
            ensure_name_free(&existing, "Backend").is_ok(),
            "names are case-sensitive"
        );
        assert!(
            ensure_name_free(&existing, "abcdef012345").is_ok(),
            "an id prefix is not a name"
        );
        assert!(ensure_name_free(&[], "backend").is_ok());
    }

    #[test]
    fn builds_ipam() {
        let mut s = spec("web");
        s.subnet = Some("10.9.0.0/24".into());
        s.gateway = Some("10.9.0.1".into());
        let req = build_create_request(&s).unwrap();
        let cfg = req.ipam.unwrap().config.unwrap();
        assert_eq!(cfg[0].subnet.as_deref(), Some("10.9.0.0/24"));
        assert_eq!(cfg[0].gateway.as_deref(), Some("10.9.0.1"));
    }
}
