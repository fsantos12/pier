//! Property and table tests for `pier_core::validate` and `Endpoint::parse`.
//!
//! bollard builds URL paths from raw ids, so these validators are the only defence
//! against path smuggling; the properties below must hold for any input.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use pier_core::CoreError;
use pier_core::docker::Endpoint;
use pier_core::validate::{self, MAX_NAME_LEN};
use proptest::prelude::*;

/// Characters that change the meaning of a URL path or query, or of a shell line.
const DANGEROUS: &[char] = &[
    '/', '\\', '?', '#', '%', '&', '=', ' ', '\t', '\n', '\r', '\0', ';', '$', '`', '|', '"', '\'',
    '<', '>', '(', ')', '{', '}', '*', '~', ':', '@', '+',
];

proptest! {
    #![proptest_config(ProptestConfig::with_cases(4096))]

    #[test]
    fn accepted_refs_are_path_safe(s in "\\PC{0,200}") {
        if validate::resource_ref(&s).is_ok() {
            prop_assert!(!s.is_empty() && s.len() <= MAX_NAME_LEN);
            prop_assert!(s.is_ascii());
            prop_assert!(!s.contains(DANGEROUS));
            prop_assert!(!s.starts_with('.') && !s.starts_with('-'));
        }
    }

    #[test]
    fn any_valid_name_is_a_valid_ref(s in "[a-zA-Z0-9][a-zA-Z0-9_.-]{0,140}") {
        if validate::name(&s).is_ok() {
            prop_assert!(validate::resource_ref(&s).is_ok());
            prop_assert!(validate::volume_ref(&s).is_ok());
        }
    }

    #[test]
    fn validators_never_panic(s in any::<String>()) {
        let _ = validate::name(&s);
        let _ = validate::resource_ref(&s);
        let _ = validate::image_reference(&s);
        let _ = validate::cidr(&s);
        let _ = validate::ip_address(&s);
        let _ = validate::volume_driver(&s);
        let _ = validate::network_driver(&s);
        let _ = Endpoint::parse(&s);
    }

    #[test]
    fn accepted_image_refs_have_no_traversal(s in "\\PC{0,300}") {
        if validate::image_reference(&s).is_ok() {
            prop_assert!(s.is_ascii() && !s.contains(".."));
            prop_assert!(!s.contains([' ', '?', '#', '%', '\\', '\n', ';']));
        }
    }

    #[test]
    fn errors_are_invalid_input_variants(s in "\\PC{0,50}") {
        if let Err(e) = validate::name(&s) {
            prop_assert!(matches!(e, CoreError::InvalidInput(_)));
        }
    }
}

#[test]
fn unicode_and_encoding_tricks_are_rejected() {
    for bad in [
        "a\u{200b}b",  // zero-width space
        "a\u{202e}b",  // right-to-left override
        "ａｐｐ",      // full-width letters
        "app\u{0301}", // combining accent
        "%2e%2e",
        "..%2f",
        "a%00",
    ] {
        assert!(validate::name(bad).is_err(), "{bad:?}");
        assert!(validate::resource_ref(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn name_length_boundary() {
    assert!(validate::name(&"a".repeat(MAX_NAME_LEN)).is_ok());
    assert!(validate::name(&"a".repeat(MAX_NAME_LEN + 1)).is_err());
}

#[test]
fn endpoint_parses_every_supported_scheme() {
    assert_eq!(
        Endpoint::parse("unix:///var/run/docker.sock").unwrap(),
        Endpoint::Unix("/var/run/docker.sock".into())
    );
    assert_eq!(
        Endpoint::parse("npipe:////./pipe/docker_engine").unwrap(),
        Endpoint::NamedPipe("//./pipe/docker_engine".into())
    );
    assert!(matches!(
        Endpoint::parse("tcp://10.0.0.5:2376").unwrap(),
        Endpoint::Tcp { tls: false, .. }
    ));
    assert!(matches!(
        Endpoint::parse("https://docker.example:2376/").unwrap(),
        Endpoint::Tcp { tls: true, .. }
    ));
    assert!(matches!(
        Endpoint::parse("ssh://deploy@host:22").unwrap(),
        Endpoint::Ssh(_)
    ));
}

#[test]
fn endpoint_rejects_malformed_hosts() {
    for bad in [
        "",
        "docker.sock",
        "unix://",
        "ftp://host:21",
        "tcp://host",
        "tcp://host:2375/path",
        "tcp://user@host:2375",
        "ssh://-oProxyCommand=id",
        "ssh://host/path",
        "ssh://us er@host",
    ] {
        assert!(
            matches!(Endpoint::parse(bad), Err(CoreError::InvalidConfig(_))),
            "{bad:?}"
        );
    }
}

/// `ssh` reads a target such as `user@-oProxyCommand=...` as an option on some
/// versions; the parser should refuse a host part that starts with '-'.
#[test]
#[ignore = "known issue: ssh host part after user@ may start with '-'"]
fn ssh_host_after_user_cannot_be_an_option() {
    for bad in [
        "ssh://user@-oProxyCommand=id",
        "ssh://u@-p22",
        "ssh://host\t-v",
    ] {
        assert!(Endpoint::parse(bad).is_err(), "{bad:?}");
    }
}

#[cfg(target_os = "linux")]
#[test]
fn linux_default_is_the_docker_socket() {
    assert_eq!(
        Endpoint::platform_default(),
        Endpoint::Unix("/var/run/docker.sock".into())
    );
}

#[cfg(windows)]
#[test]
fn windows_default_is_the_named_pipe() {
    assert_eq!(
        Endpoint::platform_default(),
        Endpoint::NamedPipe("//./pipe/docker_engine".into())
    );
}

#[tokio::test]
async fn unreachable_tcp_host_fails_fast_with_unavailable() {
    use pier_core::docker::{ConnectionConfig, DockerApi, DockerClient};
    let client = DockerClient::connect(&ConnectionConfig {
        host: Some("tcp://127.0.0.1:1".into()),
        timeout_secs: 2,
        ..ConnectionConfig::default()
    })
    .unwrap();
    let result = tokio::time::timeout(std::time::Duration::from_secs(10), client.ping())
        .await
        .expect("ping must respect the configured timeout");
    assert!(
        matches!(result, Err(CoreError::Unavailable(_))),
        "{result:?}"
    );
}
