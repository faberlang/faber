//! Tests for the `[device] device` manifest key (differentiable-GPU campaign
//! S1-5, N1.1): parsing, fail-closed validation, and the precedence fold.

use super::*;
use radix::module::diagnostics::DiagnosticArg;
use std::path::Path;

#[test]
fn absent_device_key_is_none() {
    let result = manifest_device_choice(None, Path::new("faber.toml")).expect("absent key");
    assert_eq!(result, None);
}

#[test]
fn valid_device_spellings_parse() {
    for (spelling, expected) in [
        ("auto", DeviceChoice::Auto),
        ("reference", DeviceChoice::Reference),
        ("cpu", DeviceChoice::Cpu),
        ("metal", DeviceChoice::Metal),
        ("cuda", DeviceChoice::Cuda),
    ] {
        let result =
            manifest_device_choice(Some(spelling), Path::new("faber.toml")).expect("parses");
        assert_eq!(result, Some(expected), "spelling {spelling}");
    }
}

#[test]
fn whitespace_around_spelling_is_tolerated() {
    let result = manifest_device_choice(Some("  metal  "), Path::new("faber.toml"))
        .expect("trimmed spelling");
    assert_eq!(result, Some(DeviceChoice::Metal));
}

#[test]
fn unsupported_device_fails_closed_with_issue() {
    let err = manifest_device_choice(Some("rocm"), Path::new("faber.toml"))
        .expect_err("unsupported value must fail closed");
    assert_eq!(err.issue(), Some("package_device_unsupported"));
    assert!(err.args.contains(&DiagnosticArg::new("device", "rocm")));
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
