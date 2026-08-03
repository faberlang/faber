//! Tests for the `[device] backend` manifest key (differentiable-GPU campaign
//! S1-5, N1.1): parsing, fail-closed validation, and the precedence fold.

use super::*;
use faber::device::DeviceSelection;
use std::path::Path;

#[test]
fn absent_backend_key_is_none() {
    let result = manifest_backend_selection(None, Path::new("faber.toml")).expect("absent key");
    assert_eq!(result, None);
}

#[test]
fn valid_backend_spellings_parse() {
    for (spelling, expected) in [
        ("auto", DeviceSelection::Auto),
        ("metal", DeviceSelection::Metal),
        ("cuda", DeviceSelection::Cuda),
    ] {
        let result =
            manifest_backend_selection(Some(spelling), Path::new("faber.toml")).expect("parses");
        assert_eq!(result, Some(expected), "spelling {spelling}");
    }
}

#[test]
fn whitespace_around_spelling_is_tolerated() {
    let result = manifest_backend_selection(Some("  metal  "), Path::new("faber.toml"))
        .expect("trimmed spelling");
    assert_eq!(result, Some(DeviceSelection::Metal));
}

#[test]
fn unsupported_backend_fails_closed_with_issue() {
    let err = manifest_backend_selection(Some("rocm"), Path::new("faber.toml"))
        .expect_err("unsupported value must fail closed");
    assert_eq!(err.issue(), Some("package_device_backend_unsupported"));
    assert!(err.message.contains("rocm"));
}
