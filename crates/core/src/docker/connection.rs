//! Docker connection settings and the logic that turns them into a bollard client.
//!
//! Parsing ([`Endpoint::parse`]) is pure so every scheme can be unit tested on any OS;
//! platform restrictions (named pipes only on Windows, ssh only on unix) are applied in
//! [`connect`], which is the only place that touches bollard's connectors.

use std::path::{Path, PathBuf};

use bollard::{API_DEFAULT_VERSION, Docker};

use crate::error::{CoreError, Result};

/// Default request timeout, in seconds.
pub const DEFAULT_TIMEOUT_SECS: u64 = 120;

/// Default socket path on Linux and the fallback on macOS.
pub const DEFAULT_UNIX_SOCKET: &str = "/var/run/docker.sock";
/// Default named pipe on Windows (Docker Desktop).
pub const DEFAULT_NAMED_PIPE: &str = "//./pipe/docker_engine";

/// Where the Docker daemon listens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Endpoint {
    /// A unix domain socket path (Linux, macOS).
    Unix(PathBuf),
    /// A Windows named pipe path such as `//./pipe/docker_engine`.
    NamedPipe(String),
    /// A TCP address (`host:port`), optionally protected by TLS.
    Tcp {
        /// `host:port` without a scheme.
        address: String,
        /// Whether TLS is requested (`https://` scheme).
        tls: bool,
    },
    /// An ssh target (`user@host[:port]`), reached through the system `ssh` binary.
    Ssh(String),
}

impl Endpoint {
    /// Parses a `DOCKER_HOST`-style URL.
    ///
    /// Supported schemes: `unix://`, `npipe://`, `tcp://`, `http://`, `https://`, `ssh://`.
    ///
    /// # Errors
    /// Returns [`CoreError::InvalidConfig`] for unknown schemes and empty targets.
    pub fn parse(url: &str) -> Result<Self> {
        let url = url.trim();
        let bad = |msg: &str| CoreError::InvalidConfig(format!("{msg}: {url:?}"));
        let (scheme, rest) = url
            .split_once("://")
            .ok_or_else(|| bad("docker host must look like scheme://target"))?;
        if rest.is_empty() {
            return Err(bad("docker host has an empty target"));
        }
        match scheme {
            "unix" => Ok(Self::Unix(PathBuf::from(rest))),
            "npipe" => Ok(Self::NamedPipe(rest.to_string())),
            "tcp" | "http" | "https" => {
                let address = rest.trim_end_matches('/');
                if address.contains('/') || address.contains('@') || !address.contains(':') {
                    return Err(bad("tcp docker host must be host:port"));
                }
                Ok(Self::Tcp {
                    address: address.to_string(),
                    tls: scheme == "https",
                })
            }
            "ssh" => {
                let target = rest.trim_end_matches('/');
                if target.contains('/') || target.starts_with('-') || target.contains(' ') {
                    return Err(bad("ssh docker host must be [user@]host[:port]"));
                }
                Ok(Self::Ssh(target.to_string()))
            }
            _ => Err(bad("unsupported docker host scheme")),
        }
    }

    /// Returns the platform default: the named pipe on Windows, the Docker Desktop socket
    /// on macOS when it exists, and `/var/run/docker.sock` otherwise.
    #[must_use]
    pub fn platform_default() -> Self {
        if cfg!(windows) {
            return Self::NamedPipe(DEFAULT_NAMED_PIPE.to_string());
        }
        if cfg!(target_os = "macos") {
            if let Some(home) = std::env::var_os("HOME") {
                let desktop = Path::new(&home).join(".docker/run/docker.sock");
                if desktop.exists() {
                    return Self::Unix(desktop);
                }
            }
        }
        Self::Unix(PathBuf::from(DEFAULT_UNIX_SOCKET))
    }

    /// Renders the endpoint for logs and the UI.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Unix(path) => format!("unix://{}", path.display()),
            Self::NamedPipe(pipe) => format!("npipe://{pipe}"),
            Self::Tcp { address, tls: true } => format!("https://{address}"),
            Self::Tcp {
                address,
                tls: false,
            } => format!("tcp://{address}"),
            Self::Ssh(target) => format!("ssh://{target}"),
        }
    }
}

/// Client certificate material for tcp+TLS connections.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TlsFiles {
    /// CA certificate that signed the daemon certificate.
    pub ca: PathBuf,
    /// Client certificate.
    pub cert: PathBuf,
    /// Client private key.
    pub key: PathBuf,
}

impl TlsFiles {
    /// Uses the Docker CLI layout: `ca.pem`, `cert.pem` and `key.pem` inside `dir`.
    #[must_use]
    pub fn from_dir(dir: &Path) -> Self {
        Self {
            ca: dir.join("ca.pem"),
            cert: dir.join("cert.pem"),
            key: dir.join("key.pem"),
        }
    }
}

/// How to reach the Docker daemon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionConfig {
    /// Daemon endpoint; `None` selects [`Endpoint::platform_default`].
    pub host: Option<String>,
    /// TLS files for `https://` (or `tcp://` with TLS verification) endpoints.
    pub tls: Option<TlsFiles>,
    /// When `true`, a `tcp://` host is upgraded to TLS (mirrors `DOCKER_TLS_VERIFY`).
    pub tls_verify: bool,
    /// Per-request timeout in seconds.
    pub timeout_secs: u64,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            host: None,
            tls: None,
            tls_verify: false,
            timeout_secs: DEFAULT_TIMEOUT_SECS,
        }
    }
}

impl ConnectionConfig {
    /// Resolves the configured host (or the platform default) into an [`Endpoint`],
    /// applying `tls_verify` to plain `tcp://` hosts.
    ///
    /// # Errors
    /// Returns [`CoreError::InvalidConfig`] when the host URL is malformed.
    pub fn endpoint(&self) -> Result<Endpoint> {
        let endpoint = match self.host.as_deref().filter(|h| !h.trim().is_empty()) {
            Some(host) => Endpoint::parse(host)?,
            None => Endpoint::platform_default(),
        };
        Ok(match endpoint {
            Endpoint::Tcp { address, .. } if self.tls_verify => {
                Endpoint::Tcp { address, tls: true }
            }
            other => other,
        })
    }
}

/// Builds a bollard client for the configuration. Does not contact the daemon.
///
/// # Errors
/// Returns [`CoreError::InvalidConfig`] for malformed hosts, endpoints the current
/// platform cannot use (unix sockets on Windows, named pipes elsewhere, ssh on Windows),
/// missing TLS files, and TLS endpoints without certificates.
pub fn connect(config: &ConnectionConfig) -> Result<(Docker, Endpoint)> {
    let endpoint = config.endpoint()?;
    let timeout = config.timeout_secs.max(1);
    let docker = match &endpoint {
        Endpoint::Unix(path) => connect_unix(path, timeout)?,
        Endpoint::NamedPipe(pipe) => connect_pipe(pipe, timeout)?,
        Endpoint::Tcp {
            address,
            tls: false,
        } => Docker::connect_with_http(&format!("tcp://{address}"), timeout, API_DEFAULT_VERSION)?,
        Endpoint::Tcp { address, tls: true } => {
            let files = config.tls.as_ref().ok_or_else(|| {
                CoreError::InvalidConfig(
                    "tls endpoint needs ca, cert and key files (docker.cert_path or DOCKER_CERT_PATH)".into(),
                )
            })?;
            for file in [&files.ca, &files.cert, &files.key] {
                if !file.is_file() {
                    return Err(CoreError::InvalidConfig(format!(
                        "tls file not found: {}",
                        file.display()
                    )));
                }
            }
            Docker::connect_with_ssl(
                &format!("tcp://{address}"),
                &files.key,
                &files.cert,
                &files.ca,
                timeout,
                API_DEFAULT_VERSION,
            )?
        }
        Endpoint::Ssh(target) => connect_ssh(target, timeout)?,
    };
    Ok((docker, endpoint))
}

#[cfg(unix)]
fn connect_unix(path: &Path, timeout: u64) -> Result<Docker> {
    // Not checking that the socket exists: the daemon may start after Pier, and a
    // missing socket surfaces as a 503 on each request instead of a startup failure.
    let path = path.to_string_lossy();
    Ok(Docker::connect_with_unix(
        &path,
        timeout,
        API_DEFAULT_VERSION,
    )?)
}

#[cfg(not(unix))]
fn connect_unix(_path: &Path, _timeout: u64) -> Result<Docker> {
    Err(CoreError::InvalidConfig(
        "unix sockets are not supported on this platform; use npipe:// or tcp://".into(),
    ))
}

#[cfg(windows)]
fn connect_pipe(pipe: &str, timeout: u64) -> Result<Docker> {
    Ok(Docker::connect_with_named_pipe(
        pipe,
        timeout,
        API_DEFAULT_VERSION,
    )?)
}

#[cfg(not(windows))]
fn connect_pipe(_pipe: &str, _timeout: u64) -> Result<Docker> {
    Err(CoreError::InvalidConfig(
        "named pipes are only supported on Windows; use unix:// or tcp://".into(),
    ))
}

#[cfg(unix)]
fn connect_ssh(target: &str, timeout: u64) -> Result<Docker> {
    Ok(Docker::connect_with_ssh(
        &format!("ssh://{target}"),
        timeout,
        API_DEFAULT_VERSION,
        None,
    )?)
}

#[cfg(not(unix))]
fn connect_ssh(_target: &str, _timeout: u64) -> Result<Docker> {
    Err(CoreError::InvalidConfig(
        "ssh docker hosts are not supported on this platform; use tcp+tls".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_scheme() {
        assert_eq!(
            Endpoint::parse("unix:///var/run/docker.sock").unwrap(),
            Endpoint::Unix("/var/run/docker.sock".into())
        );
        assert_eq!(
            Endpoint::parse("npipe:////./pipe/docker_engine").unwrap(),
            Endpoint::NamedPipe("//./pipe/docker_engine".into())
        );
        assert_eq!(
            Endpoint::parse("tcp://10.0.0.5:2376").unwrap(),
            Endpoint::Tcp {
                address: "10.0.0.5:2376".into(),
                tls: false
            }
        );
        assert_eq!(
            Endpoint::parse("https://docker.example:2376/").unwrap(),
            Endpoint::Tcp {
                address: "docker.example:2376".into(),
                tls: true
            }
        );
        assert_eq!(
            Endpoint::parse("ssh://root@box:2222").unwrap(),
            Endpoint::Ssh("root@box:2222".into())
        );
    }

    #[test]
    fn rejects_bad_hosts() {
        for bad in [
            "",
            "docker",
            "ftp://x:1",
            "unix://",
            "tcp://nohostport",
            "tcp://a:1/path",
            "tcp://u@a:1",
            "ssh://-oProxyCommand=x",
            "ssh://a b",
            "ssh://a/b",
        ] {
            assert!(
                matches!(Endpoint::parse(bad), Err(CoreError::InvalidConfig(_))),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn tls_verify_upgrades_tcp() {
        let cfg = ConnectionConfig {
            host: Some("tcp://h:2376".into()),
            tls_verify: true,
            ..ConnectionConfig::default()
        };
        assert_eq!(
            cfg.endpoint().unwrap(),
            Endpoint::Tcp {
                address: "h:2376".into(),
                tls: true
            }
        );
    }

    #[test]
    fn blank_host_uses_platform_default() {
        let cfg = ConnectionConfig {
            host: Some("  ".into()),
            ..ConnectionConfig::default()
        };
        assert_eq!(cfg.endpoint().unwrap(), Endpoint::platform_default());
    }

    #[test]
    fn tls_without_files_is_a_config_error() {
        let cfg = ConnectionConfig {
            host: Some("https://h:2376".into()),
            ..ConnectionConfig::default()
        };
        assert!(matches!(connect(&cfg), Err(CoreError::InvalidConfig(_))));
    }

    #[test]
    fn missing_tls_files_are_reported() {
        let cfg = ConnectionConfig {
            host: Some("https://h:2376".into()),
            tls: Some(TlsFiles::from_dir(Path::new("/definitely/not/here"))),
            ..ConnectionConfig::default()
        };
        assert!(matches!(connect(&cfg), Err(CoreError::InvalidConfig(_))));
    }

    #[cfg(not(windows))]
    #[test]
    fn named_pipes_are_rejected_off_windows() {
        let cfg = ConnectionConfig {
            host: Some("npipe:////./pipe/docker_engine".into()),
            ..ConnectionConfig::default()
        };
        assert!(matches!(connect(&cfg), Err(CoreError::InvalidConfig(_))));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn missing_unix_socket_is_unavailable_not_a_config_error() {
        let cfg = ConnectionConfig {
            host: Some("unix:///nonexistent/docker.sock".into()),
            ..ConnectionConfig::default()
        };
        assert!(matches!(connect(&cfg), Err(CoreError::Unavailable(_))));
    }

    #[test]
    fn describe_roundtrips() {
        for url in ["unix:///a.sock", "tcp://h:1", "https://h:2", "ssh://u@h"] {
            assert_eq!(Endpoint::parse(url).unwrap().describe(), url);
        }
    }
}
