//! Server configuration: `pier.toml` merged with environment overrides.
//!
//! Precedence, lowest to highest: built-in defaults, `pier.toml`, environment
//! (`PIER_BIND`, `PIER_DATA_DIR`, `DOCKER_HOST`, `DOCKER_CERT_PATH`, `DOCKER_TLS_VERIFY`).

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use pier_core::docker::{ConnectionConfig, DEFAULT_TIMEOUT_SECS, TlsFiles};
use serde::Deserialize;

/// Default listen address: loopback only, no built-in TLS in Phase 1.
pub const DEFAULT_BIND: &str = "127.0.0.1:8640";
/// Default session lifetime in hours.
pub const DEFAULT_SESSION_TTL_HOURS: u64 = 24;

/// Raw contents of `pier.toml`. Unknown keys are rejected so typos do not go unnoticed.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FileConfig {
    /// Listen address, for example `127.0.0.1:8640`.
    pub bind: Option<String>,
    /// Directory for the SQLite database.
    pub data_dir: Option<PathBuf>,
    /// Mark the session cookie `Secure` (set when serving through an HTTPS proxy).
    pub cookie_secure: Option<bool>,
    /// Session lifetime in hours.
    pub session_ttl_hours: Option<u64>,
    /// Docker connection settings.
    pub docker: DockerFileConfig,
}

/// The `[docker]` table of `pier.toml`.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DockerFileConfig {
    /// `DOCKER_HOST`-style URL (`unix://`, `npipe://`, `tcp://`, `https://`, `ssh://`).
    pub host: Option<String>,
    /// Directory holding `ca.pem`, `cert.pem` and `key.pem` for TLS.
    pub cert_path: Option<PathBuf>,
    /// Upgrade `tcp://` hosts to TLS.
    pub tls_verify: Option<bool>,
    /// Per-request timeout in seconds.
    pub timeout_secs: Option<u64>,
}

/// Resolved runtime configuration.
#[derive(Debug, Clone)]
pub struct Config {
    /// Address the HTTP server binds to.
    pub bind: SocketAddr,
    /// Directory containing the SQLite database.
    pub data_dir: PathBuf,
    /// Whether the session cookie carries the `Secure` attribute.
    pub cookie_secure: bool,
    /// Session lifetime in seconds.
    pub session_ttl_secs: i64,
    /// Docker connection settings.
    pub docker: ConnectionConfig,
}

/// Returns the per-OS default data directory.
#[must_use]
pub fn default_data_dir(env: &dyn Fn(&str) -> Option<String>) -> PathBuf {
    if cfg!(windows) {
        let base = env("APPDATA").unwrap_or_else(|| ".".into());
        PathBuf::from(base).join("Pier")
    } else if cfg!(target_os = "macos") {
        let home = env("HOME").unwrap_or_else(|| ".".into());
        PathBuf::from(home).join("Library/Application Support/Pier")
    } else {
        PathBuf::from("/var/lib/pier")
    }
}

fn truthy(value: &str) -> bool {
    !value.is_empty() && value != "0" && !value.eq_ignore_ascii_case("false")
}

impl FileConfig {
    /// Reads and parses a `pier.toml`.
    ///
    /// # Errors
    /// Fails when the file cannot be read or contains invalid or unknown keys.
    pub fn from_path(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("cannot read config file {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("invalid config file {}", path.display()))
    }
}

impl Config {
    /// Merges a parsed file with environment overrides.
    ///
    /// `env` is injected so tests do not depend on the process environment.
    ///
    /// # Errors
    /// Fails when `bind` is not a socket address or the session lifetime is zero.
    pub fn resolve(file: FileConfig, env: &dyn Fn(&str) -> Option<String>) -> Result<Self> {
        let non_empty = |key: &str| env(key).filter(|v| !v.trim().is_empty());

        let bind_text = non_empty("PIER_BIND")
            .or(file.bind)
            .unwrap_or_else(|| DEFAULT_BIND.to_string());
        let bind: SocketAddr = bind_text
            .parse()
            .with_context(|| format!("invalid bind address {bind_text:?} (expected host:port)"))?;

        let data_dir = non_empty("PIER_DATA_DIR")
            .map(PathBuf::from)
            .or(file.data_dir)
            .unwrap_or_else(|| default_data_dir(env));

        let ttl_hours = file.session_ttl_hours.unwrap_or(DEFAULT_SESSION_TTL_HOURS);
        if ttl_hours == 0 || ttl_hours > 24 * 365 {
            bail!("session_ttl_hours must be between 1 and {}", 24 * 365);
        }

        let tls_verify = non_empty("DOCKER_TLS_VERIFY")
            .map(|v| truthy(&v))
            .or(file.docker.tls_verify)
            .unwrap_or(false);
        let cert_dir = non_empty("DOCKER_CERT_PATH")
            .map(PathBuf::from)
            .or(file.docker.cert_path)
            .or_else(|| {
                non_empty("HOME")
                    .or_else(|| non_empty("USERPROFILE"))
                    .map(|home| PathBuf::from(home).join(".docker"))
            });

        Ok(Self {
            bind,
            data_dir,
            cookie_secure: file.cookie_secure.unwrap_or(false),
            session_ttl_secs: i64::try_from(ttl_hours * 3600).unwrap_or(i64::MAX),
            docker: ConnectionConfig {
                host: non_empty("DOCKER_HOST").or(file.docker.host),
                tls: cert_dir.as_deref().map(TlsFiles::from_dir),
                tls_verify,
                timeout_secs: file.docker.timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS),
            },
        })
    }

    /// Loads configuration from an optional file path plus the process environment.
    ///
    /// # Errors
    /// Propagates file and validation errors from [`FileConfig::from_path`] and
    /// [`Config::resolve`].
    pub fn load(path: Option<&Path>) -> Result<Self> {
        let file = match path {
            Some(path) => FileConfig::from_path(path)?,
            None => FileConfig::default(),
        };
        Self::resolve(file, &|key| std::env::var(key).ok())
    }

    /// Returns `true` when the bind address is not a loopback address.
    #[must_use]
    pub fn binds_publicly(&self) -> bool {
        !self.bind.ip().is_loopback()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_env(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn defaults() {
        let cfg = Config::resolve(FileConfig::default(), &no_env).unwrap();
        assert_eq!(cfg.bind.to_string(), DEFAULT_BIND);
        assert!(!cfg.binds_publicly());
        assert!(!cfg.cookie_secure);
        assert_eq!(cfg.session_ttl_secs, 24 * 3600);
        assert_eq!(cfg.docker.host, None);
        assert_eq!(cfg.docker.timeout_secs, DEFAULT_TIMEOUT_SECS);
    }

    #[test]
    fn file_values_and_env_overrides() {
        let file: FileConfig = toml::from_str(
            r#"
            bind = "0.0.0.0:9000"
            data_dir = "/tmp/from-file"
            [docker]
            host = "tcp://file:2375"
            "#,
        )
        .unwrap();
        let env = |key: &str| match key {
            "PIER_BIND" => Some("127.0.0.1:1234".to_string()),
            "DOCKER_HOST" => Some("unix:///env.sock".to_string()),
            _ => None,
        };
        let cfg = Config::resolve(file, &env).unwrap();
        assert_eq!(cfg.bind.to_string(), "127.0.0.1:1234");
        assert_eq!(cfg.data_dir, PathBuf::from("/tmp/from-file"));
        assert_eq!(cfg.docker.host.as_deref(), Some("unix:///env.sock"));
    }

    #[test]
    fn public_bind_is_detected() {
        let file = FileConfig {
            bind: Some("0.0.0.0:8640".into()),
            ..FileConfig::default()
        };
        assert!(Config::resolve(file, &no_env).unwrap().binds_publicly());
    }

    #[test]
    fn rejects_bad_values_and_unknown_keys() {
        let file = FileConfig {
            bind: Some("not-an-address".into()),
            ..FileConfig::default()
        };
        assert!(Config::resolve(file, &no_env).is_err());
        let file = FileConfig {
            session_ttl_hours: Some(0),
            ..FileConfig::default()
        };
        assert!(Config::resolve(file, &no_env).is_err());
        assert!(toml::from_str::<FileConfig>("bnid = 1").is_err());
    }

    #[test]
    fn tls_verify_env_is_truthy_only_for_real_values() {
        for (value, expected) in [
            ("1", true),
            ("true", true),
            ("0", false),
            ("", false),
            ("false", false),
        ] {
            let env = |key: &str| (key == "DOCKER_TLS_VERIFY").then(|| value.to_string());
            let cfg = Config::resolve(FileConfig::default(), &env).unwrap();
            assert_eq!(cfg.docker.tls_verify, expected, "{value:?}");
        }
    }

    #[test]
    fn example_config_parses() {
        let text = include_str!("../../../pier.example.toml");
        toml::from_str::<FileConfig>(text).unwrap();
    }
}
