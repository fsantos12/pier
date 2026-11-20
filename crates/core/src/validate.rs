//! Input validation for everything that ends up in a Docker API path, query or body.
//!
//! Docker builds request URLs from resource names (`/containers/{id}`), so an unchecked
//! value could smuggle `/`, `?` or `..` into the path. Every identifier is therefore
//! validated here before it reaches the bollard wrapper, and the server calls the same
//! functions so bad input is rejected with a clean 400 before any Docker call.

use std::net::IpAddr;

use crate::error::{CoreError, Result};

/// Longest accepted resource name or reference.
pub const MAX_NAME_LEN: usize = 128;
/// Longest accepted image reference (Docker's own limit is 255).
pub const MAX_IMAGE_REF_LEN: usize = 255;
/// Maximum number of command arguments for a new container.
pub const MAX_CMD_ARGS: usize = 64;
/// Maximum byte length of a single command argument.
pub const MAX_CMD_ARG_LEN: usize = 4096;
/// Network drivers the Foundation phase lets users pick.
pub const NETWORK_DRIVERS: [&str; 4] = ["bridge", "overlay", "macvlan", "ipvlan"];

fn invalid(msg: impl Into<String>) -> CoreError {
    CoreError::InvalidInput(msg.into())
}

fn is_hex_id_like(value: &str) -> bool {
    (12..=64).contains(&value.len())
        && value
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn has_name_charset(value: &str) -> bool {
    let mut bytes = value.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    first.is_ascii_alphanumeric()
        && bytes.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
}

/// Validates a name for a *new* container, network or volume.
///
/// Names must match `[a-zA-Z0-9][a-zA-Z0-9_.-]*` and be at most [`MAX_NAME_LEN`] bytes.
/// Names that look like a Docker ID (12 to 64 lowercase hex characters) are refused so
/// a name can never shadow another resource's ID prefix.
///
/// # Errors
/// Returns [`CoreError::InvalidInput`] when the name breaks any rule.
pub fn name(value: &str) -> Result<()> {
    if value.is_empty() || value.len() > MAX_NAME_LEN {
        return Err(invalid(format!(
            "name must be 1 to {MAX_NAME_LEN} characters"
        )));
    }
    if !has_name_charset(value) {
        return Err(invalid(
            "name may only contain letters, digits, '_', '.' and '-' and must start with a letter or digit",
        ));
    }
    if is_hex_id_like(value) {
        return Err(invalid("name must not look like a Docker ID"));
    }
    Ok(())
}

/// Validates a reference (name or ID) to an *existing* container or network.
///
/// # Errors
/// Returns [`CoreError::InvalidInput`] unless the value is a plain name or a 12 to 64
/// character lowercase hex ID.
pub fn resource_ref(value: &str) -> Result<()> {
    if value.is_empty() || value.len() > MAX_NAME_LEN || !has_name_charset(value) {
        return Err(invalid("malformed resource identifier"));
    }
    Ok(())
}

/// Validates a reference to an existing volume (volumes are addressed by name only).
///
/// # Errors
/// Returns [`CoreError::InvalidInput`] when the name has characters Docker does not allow.
pub fn volume_ref(value: &str) -> Result<()> {
    resource_ref(value)
}

/// Validates an image reference such as `nginx`, `nginx:1.27`, `ghcr.io/org/app:tag`
/// or `repo@sha256:<digest>`.
///
/// # Errors
/// Returns [`CoreError::InvalidInput`] for empty, oversized or oddly shaped references.
pub fn image_reference(value: &str) -> Result<()> {
    if value.is_empty() || value.len() > MAX_IMAGE_REF_LEN {
        return Err(invalid(format!(
            "image reference must be 1 to {MAX_IMAGE_REF_LEN} characters"
        )));
    }
    let allowed =
        |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-' | b'/' | b':' | b'@');
    if !value.bytes().all(allowed) {
        return Err(invalid("image reference contains forbidden characters"));
    }
    let starts_ok = value
        .as_bytes()
        .first()
        .is_some_and(u8::is_ascii_alphanumeric);
    if !starts_ok || value.contains("..") || value.contains("//") || value.ends_with('/') {
        return Err(invalid("malformed image reference"));
    }
    Ok(())
}

/// Validates the argument array of a container command.
///
/// Arguments are passed to Docker as an array and never joined into a shell string, so
/// only size limits and NUL bytes are checked.
///
/// # Errors
/// Returns [`CoreError::InvalidInput`] when there are too many arguments, one is too
/// long, or one contains a NUL byte.
pub fn command(args: &[String]) -> Result<()> {
    if args.len() > MAX_CMD_ARGS {
        return Err(invalid(format!("at most {MAX_CMD_ARGS} command arguments")));
    }
    for arg in args {
        if arg.len() > MAX_CMD_ARG_LEN {
            return Err(invalid("command argument too long"));
        }
        if arg.contains('\0') {
            return Err(invalid("command argument contains a NUL byte"));
        }
    }
    Ok(())
}

/// Validates a network driver against [`NETWORK_DRIVERS`].
///
/// # Errors
/// Returns [`CoreError::InvalidInput`] for any other driver.
pub fn network_driver(value: &str) -> Result<()> {
    if NETWORK_DRIVERS.contains(&value) {
        Ok(())
    } else {
        Err(invalid(format!(
            "network driver must be one of: {}",
            NETWORK_DRIVERS.join(", ")
        )))
    }
}

/// Validates a volume driver name (lowercase letters, digits, `_`, `.`, `-`).
///
/// # Errors
/// Returns [`CoreError::InvalidInput`] when the driver name is empty, too long or odd.
pub fn volume_driver(value: &str) -> Result<()> {
    let ok = !value.is_empty()
        && value.len() <= 64
        && has_name_charset(value)
        && !value.bytes().any(|b| b.is_ascii_uppercase());
    if ok {
        Ok(())
    } else {
        Err(invalid("malformed volume driver name"))
    }
}

/// Validates an IPv4 or IPv6 CIDR such as `172.28.0.0/16`.
///
/// # Errors
/// Returns [`CoreError::InvalidInput`] for anything that is not `address/prefix`.
pub fn cidr(value: &str) -> Result<()> {
    let (addr, prefix) = value
        .split_once('/')
        .ok_or_else(|| invalid("subnet must be in CIDR notation"))?;
    let addr: IpAddr = addr
        .parse()
        .map_err(|_| invalid("subnet has an invalid address"))?;
    let max = if addr.is_ipv4() { 32 } else { 128 };
    match prefix.parse::<u8>() {
        Ok(p) if p <= max => Ok(()),
        _ => Err(invalid("subnet has an invalid prefix length")),
    }
}

/// Validates a plain IP address (used for gateways).
///
/// # Errors
/// Returns [`CoreError::InvalidInput`] when the value is not an IP address.
pub fn ip_address(value: &str) -> Result<()> {
    value
        .parse::<IpAddr>()
        .map(|_| ())
        .map_err(|_| invalid("invalid IP address"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_docker_style_names() {
        for ok in ["web", "my-app_1", "a.b-c", "A1", "x"] {
            assert!(name(ok).is_ok(), "{ok}");
        }
    }

    #[test]
    fn rejects_injection_shaped_names() {
        let long = "a".repeat(MAX_NAME_LEN + 1);
        for bad in [
            "", "-lead", ".lead", "_lead", "a/b", "../x", "a b", "a;b", "a$(id)", "a`id`", "a\nb",
            "a\0b", "näme", "a?x=1", "a%2Fb", "аpp", &long,
        ] {
            assert!(name(bad).is_err(), "{bad:?}");
            assert!(resource_ref(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn names_that_look_like_ids_are_refused_but_ids_are_valid_refs() {
        let id = "0123456789abcdef".repeat(4);
        assert!(name(&id).is_err());
        assert!(name("abcdef012345").is_err());
        assert!(resource_ref(&id).is_ok());
        assert!(
            name("abcdef01234").is_ok(),
            "11 hex characters is not ID-like"
        );
    }

    #[test]
    fn image_references() {
        for ok in [
            "nginx",
            "nginx:1.27",
            "ghcr.io/org/app:v1",
            "localhost:5000/a/b",
            "repo@sha256:abc",
        ] {
            assert!(image_reference(ok).is_ok(), "{ok}");
        }
        for bad in [
            "", "-x", "a b", "a;b", "a//b", "../x", "a/", "a\nb", "a?b=c", "a\\b",
        ] {
            assert!(image_reference(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn commands_are_limited() {
        assert!(command(&["sh".into(), "-c".into(), "echo $HOME; ls".into()]).is_ok());
        assert!(command(&["a\0b".into()]).is_err());
        assert!(command(&vec!["x".to_string(); MAX_CMD_ARGS + 1]).is_err());
        assert!(command(&["y".repeat(MAX_CMD_ARG_LEN + 1)]).is_err());
    }

    #[test]
    fn network_fields() {
        assert!(network_driver("bridge").is_ok());
        assert!(network_driver("host").is_err());
        assert!(cidr("172.28.0.0/16").is_ok());
        assert!(cidr("fd00::/64").is_ok());
        assert!(cidr("172.28.0.0/33").is_err());
        assert!(cidr("172.28.0.0").is_err());
        assert!(ip_address("172.28.0.1").is_ok());
        assert!(ip_address("nope").is_err());
        assert!(volume_driver("local").is_ok());
        assert!(volume_driver("Local").is_err());
    }
}
