//! Container operations.

use bollard::models::ContainerCreateBody;
use bollard::query_parameters::{
    CreateContainerOptionsBuilder, ListContainersOptionsBuilder, RemoveContainerOptionsBuilder,
};

use super::DockerClient;
use crate::error::Result;
use crate::model::{Container, ContainerCreate, ContainerDetail};
use crate::validate;

/// Docker answers 304 when a start or stop changes nothing; that is success for us.
fn ignore_not_modified(result: std::result::Result<(), bollard::errors::Error>) -> Result<()> {
    match result {
        Err(bollard::errors::Error::DockerResponseServerError {
            status_code: 304, ..
        }) => Ok(()),
        other => Ok(other?),
    }
}

impl DockerClient {
    pub(super) async fn containers_list(&self, all: bool) -> Result<Vec<Container>> {
        let options = ListContainersOptionsBuilder::new().all(all).build();
        let list = self.docker()?.list_containers(Some(options)).await?;
        Ok(list.into_iter().map(Into::into).collect())
    }

    pub(super) async fn containers_inspect(&self, id: &str) -> Result<ContainerDetail> {
        validate::resource_ref(id)?;
        Ok(self.docker()?.inspect_container(id, None).await?.into())
    }

    pub(super) async fn containers_create(&self, spec: ContainerCreate) -> Result<ContainerDetail> {
        let (options_name, body) = build_create_body(&spec)?;
        let options = options_name
            .as_deref()
            .map(|name| CreateContainerOptionsBuilder::new().name(name).build());
        let created = self.docker()?.create_container(options, body).await?;
        self.containers_inspect(&created.id).await
    }

    pub(super) async fn containers_start(&self, id: &str) -> Result<()> {
        validate::resource_ref(id)?;
        ignore_not_modified(self.docker()?.start_container(id, None).await)
    }

    pub(super) async fn containers_stop(&self, id: &str) -> Result<()> {
        validate::resource_ref(id)?;
        ignore_not_modified(self.docker()?.stop_container(id, None).await)
    }

    pub(super) async fn containers_restart(&self, id: &str) -> Result<()> {
        validate::resource_ref(id)?;
        Ok(self.docker()?.restart_container(id, None).await?)
    }

    pub(super) async fn containers_remove(&self, id: &str) -> Result<()> {
        validate::resource_ref(id)?;
        // No `force`: removing a running container must surface as a 409.
        let options = RemoveContainerOptionsBuilder::new()
            .force(false)
            .v(false)
            .build();
        Ok(self.docker()?.remove_container(id, Some(options)).await?)
    }
}

/// Validates a [`ContainerCreate`] and returns `(name, body)` for bollard.
///
/// The body sets only image and command: no host config, so no mounts, privileges,
/// capabilities, devices or host namespaces can be requested.
pub(crate) fn build_create_body(
    spec: &ContainerCreate,
) -> Result<(Option<String>, ContainerCreateBody)> {
    validate::image_reference(&spec.image)?;
    validate::command(&spec.cmd)?;
    if let Some(name) = &spec.name {
        validate::name(name)?;
    }
    let cmd = (!spec.cmd.is_empty()).then(|| spec.cmd.clone());
    let body = ContainerCreateBody {
        image: Some(spec.image.clone()),
        cmd,
        ..ContainerCreateBody::default()
    };
    Ok((spec.name.clone(), body))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> ContainerCreate {
        ContainerCreate {
            image: "alpine:3.20".into(),
            name: Some("web".into()),
            cmd: vec!["sleep".into(), "1".into()],
        }
    }

    #[test]
    fn body_has_image_cmd_and_no_host_config() {
        let (name, body) = build_create_body(&spec()).unwrap();
        assert_eq!(name.as_deref(), Some("web"));
        assert_eq!(body.image.as_deref(), Some("alpine:3.20"));
        assert_eq!(
            body.cmd.as_deref(),
            Some(&["sleep".to_string(), "1".to_string()][..])
        );
        assert!(body.host_config.is_none());
        assert!(body.entrypoint.is_none());
    }

    #[test]
    fn cmd_stays_an_array_with_shell_metacharacters_intact() {
        let mut s = spec();
        s.cmd = vec!["sh".into(), "-c".into(), "echo hi; rm -rf /".into()];
        let (_, body) = build_create_body(&s).unwrap();
        assert_eq!(body.cmd.unwrap().len(), 3);
    }

    #[test]
    fn rejects_bad_input() {
        let mut s = spec();
        s.name = Some("a b".into());
        assert!(build_create_body(&s).is_err());
        let mut s = spec();
        s.image = "x;y".into();
        assert!(build_create_body(&s).is_err());
        let mut s = spec();
        s.cmd = vec!["a\0".into()];
        assert!(build_create_body(&s).is_err());
    }
}
