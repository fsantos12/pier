//! Volume operations.

use bollard::models::VolumeCreateRequest;
use std::collections::HashMap;

use bollard::query_parameters::{PruneVolumesOptionsBuilder, RemoveVolumeOptionsBuilder};

use super::DockerClient;
use super::create_lock::Kind;
use crate::error::{CoreError, Result};
use crate::model::{PruneReport, Volume, VolumeCreate};
use crate::validate;

impl DockerClient {
    pub(super) async fn volumes_list(&self) -> Result<Vec<Volume>> {
        let response = self
            .docker()?
            .list_volumes(None::<bollard::query_parameters::ListVolumesOptions>)
            .await?;
        Ok(response
            .volumes
            .unwrap_or_default()
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub(super) async fn volumes_inspect(&self, name: &str) -> Result<Volume> {
        validate::volume_ref(name)?;
        Ok(self.docker()?.inspect_volume(name).await?.into())
    }

    pub(super) async fn volumes_create(&self, spec: VolumeCreate) -> Result<Volume> {
        let request = build_create_request(&spec)?;
        // Docker silently reuses an existing volume of the same name, which would attach
        // new work to old data. Inspect by exact name first, holding the per-name lock
        // so parallel creates through this client cannot all pass the check.
        let _guard = self.create_locks.acquire(Kind::Volume, &spec.name).await;
        ensure_absent(
            self.docker()?
                .inspect_volume(&spec.name)
                .await
                .map(Volume::from)
                .map_err(CoreError::from),
            &spec.name,
        )?;
        Ok(self.docker()?.create_volume(request).await?.into())
    }

    pub(super) async fn volumes_remove(&self, name: &str) -> Result<()> {
        validate::volume_ref(name)?;
        // `force` stays off so a volume in use is reported as a conflict.
        let options = RemoveVolumeOptionsBuilder::new().force(false).build();
        Ok(self.docker()?.remove_volume(name, Some(options)).await?)
    }

    pub(super) async fn volumes_prune(&self) -> Result<PruneReport> {
        // Docker never prunes a volume a container references. Since API 1.42 it also
        // skips named volumes unless `all=true`, and Pier only creates named ones.
        let filters = HashMap::from([("all", vec!["true"])]);
        let options = PruneVolumesOptionsBuilder::new().filters(&filters).build();
        let response = self.docker()?.prune_volumes(Some(options)).await?;
        Ok(response.into())
    }
}

/// Interprets the outcome of inspecting a volume that is about to be created.
///
/// `NotFound` means the name is free; a found volume is a [`CoreError::Conflict`]; any
/// other error (daemon down, ...) is passed through unchanged.
pub(crate) fn ensure_absent(inspected: Result<Volume>, name: &str) -> Result<()> {
    match inspected {
        Ok(_) => Err(CoreError::Conflict(format!("volume {name} already exists"))),
        Err(CoreError::NotFound(_)) => Ok(()),
        Err(other) => Err(other),
    }
}

/// Validates a [`VolumeCreate`] and converts it into the bollard request.
pub(crate) fn build_create_request(spec: &VolumeCreate) -> Result<VolumeCreateRequest> {
    validate::name(&spec.name)?;
    if let Some(driver) = &spec.driver {
        validate::volume_driver(driver)?;
    }
    Ok(VolumeCreateRequest {
        name: Some(spec.name.clone()),
        driver: spec.driver.clone(),
        driver_opts: None,
        labels: None,
        cluster_volume_spec: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_request_has_no_driver_opts() {
        let req = build_create_request(&VolumeCreate {
            name: "data".into(),
            driver: None,
        })
        .unwrap();
        assert_eq!(req.name.as_deref(), Some("data"));
        assert!(req.driver_opts.is_none());
    }

    #[test]
    fn existing_volume_is_a_conflict_and_missing_is_free() {
        let found = Ok(Volume {
            name: "data".into(),
            ..Volume::default()
        });
        assert!(matches!(
            ensure_absent(found, "data"),
            Err(CoreError::Conflict(_))
        ));
        assert!(ensure_absent(Err(CoreError::NotFound("data".into())), "data").is_ok());
        assert!(matches!(
            ensure_absent(Err(CoreError::Unavailable("down".into())), "data"),
            Err(CoreError::Unavailable(_))
        ));
        assert!(matches!(
            ensure_absent(Err(CoreError::Docker("boom".into())), "data"),
            Err(CoreError::Docker(_))
        ));
    }

    #[test]
    fn rejects_bad_names_and_drivers() {
        assert!(
            build_create_request(&VolumeCreate {
                name: "../x".into(),
                driver: None
            })
            .is_err()
        );
        assert!(
            build_create_request(&VolumeCreate {
                name: "ok".into(),
                driver: Some("Bad Driver".into())
            })
            .is_err()
        );
    }
}
