//! Image operations.

use bollard::query_parameters::CreateImageOptionsBuilder;
use futures_util::StreamExt;

use super::DockerClient;
use crate::error::{CoreError, Result};
use crate::model::PullReport;
use crate::validate;

/// Splits a reference into `(name, tag_or_digest)`; a missing tag becomes `latest`.
///
/// Docker pulls *every* tag when none is given, so the default is made explicit.
fn split_reference(reference: &str) -> (&str, &str) {
    if reference.contains('@') {
        // Digest references go through as a whole, with no separate tag.
        return (reference, "");
    }
    let last_slash = reference.rfind('/').map_or(0, |i| i + 1);
    match reference[last_slash..].rfind(':') {
        Some(i) => (
            &reference[..last_slash + i],
            &reference[last_slash + i + 1..],
        ),
        None => (reference, "latest"),
    }
}

/// Maps a pull failure message from the registry to a domain error.
fn classify_pull_error(message: String) -> CoreError {
    let lower = message.to_lowercase();
    if lower.contains("not found")
        || lower.contains("does not exist")
        || lower.contains("pull access denied")
        || lower.contains("manifest unknown")
    {
        CoreError::NotFound(message)
    } else {
        CoreError::Docker(message)
    }
}

impl DockerClient {
    pub(super) async fn images_pull(&self, reference: &str) -> Result<PullReport> {
        validate::image_reference(reference)?;
        let (name, tag) = split_reference(reference);
        let mut builder = CreateImageOptionsBuilder::new().from_image(name);
        if !tag.is_empty() {
            builder = builder.tag(tag);
        }
        let docker = self.docker()?;
        let mut stream = docker.create_image(Some(builder.build()), None, None);
        let mut status = String::new();
        // The pull is driven to completion here; other requests keep running because
        // this only awaits the Docker stream.
        while let Some(item) = stream.next().await {
            let info = match item {
                Ok(info) => info,
                Err(bollard::errors::Error::DockerResponseServerError {
                    status_code,
                    message,
                }) if status_code == 404 || status_code == 500 => {
                    return Err(classify_pull_error(message));
                }
                Err(err) => return Err(err.into()),
            };
            if let Some(message) = info.error_detail.and_then(|e| e.message) {
                return Err(classify_pull_error(message));
            }
            if let Some(s) = info.status {
                status = s;
            }
        }
        Ok(PullReport {
            image: reference.to_string(),
            status,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_references() {
        assert_eq!(split_reference("nginx"), ("nginx", "latest"));
        assert_eq!(split_reference("nginx:1.27"), ("nginx", "1.27"));
        assert_eq!(
            split_reference("localhost:5000/app"),
            ("localhost:5000/app", "latest")
        );
        assert_eq!(
            split_reference("localhost:5000/app:v2"),
            ("localhost:5000/app", "v2")
        );
        assert_eq!(split_reference("repo@sha256:abc"), ("repo@sha256:abc", ""));
    }

    #[test]
    fn registry_messages_are_classified() {
        assert!(matches!(
            classify_pull_error("pull access denied for x".into()),
            CoreError::NotFound(_)
        ));
        assert!(matches!(
            classify_pull_error("net/http: timeout".into()),
            CoreError::Docker(_)
        ));
    }
}
