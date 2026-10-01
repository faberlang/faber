//! Tests for the `[device] backend` manifest key (differentiable-GPU campaign
//! S1-5, N1.1): parsing, fail-closed validation, and the precedence fold.

use super::*;
// S3-U2 cut: resolve through the packaged selection type in every build.
use radix::program::mir::DeviceSelection;
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

fn manifest_with_target_tables(tables: &str) -> FaberManifest {
    toml::from_str(&format!(
        "[package]\nname = \"demo\"\n\n[paths]\nentry = \"main.fab\"\n{tables}"
    ))
    .expect("manifest parses")
}

#[test]
fn validate_manifest_rejects_unknown_target_table() {
    let manifest = manifest_with_target_tables("\n[target.wgsl]\nbindings = \"b.toml\"\n");
    let err = validate_manifest(&manifest, Path::new("faber.toml"))
        .expect_err("an unknown [target.<key>] must fail check/build validation");
    assert_eq!(err.issue(), Some("package_target_table_unknown"));
}

#[test]
fn validate_manifest_accepts_target_spelling_table() {
    let manifest = manifest_with_target_tables("\n[target.wgsl-text]\nbindings = \"b.toml\"\n");
    validate_manifest(&manifest, Path::new("faber.toml"))
        .expect("a target-spelling table passes validation");
}
