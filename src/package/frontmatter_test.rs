use super::*;
use radix::codegen::rust::TestSelection;
use radix::driver::parse_file_frontmatter;

// ── validate_frontmatter_against_manifest ─────────────────────────────────

fn test_manifest() -> FaberManifest {
    let toml_str = r#"
[package]
name = "test-package"
version = "1.0.0"
edition = "2026"

[build]
target = "rust"
kind = "bin"

[paths]
source = "src"
entry = "main.fab"
"#;
    toml::from_str(toml_str).expect("valid test manifest")
}

#[test]
fn validate_frontmatter_no_conflict_when_values_match() {
    let path = Path::new("main.fab");
    let frontmatter =
        parse_file_frontmatter(r#"build = { target = "rust", kind = "bin" }"#).expect("frontmatter");
    let manifest = test_manifest();
    let result = validate_frontmatter_against_manifest(path, Some(&frontmatter), &manifest);
    assert!(result.is_none(), "matching values should not produce a conflict");
}

#[test]
fn validate_frontmatter_no_conflict_when_no_frontmatter() {
    let path = Path::new("main.fab");
    let manifest = test_manifest();
    let result = validate_frontmatter_against_manifest(path, None, &manifest);
    assert!(result.is_none(), "no frontmatter should not produce a conflict");
}

#[test]
fn validate_frontmatter_rejects_target_conflict() {
    let path = Path::new("main.fab");
    let frontmatter =
        parse_file_frontmatter(r#"build = { target = "scena" }"#).expect("frontmatter");
    let manifest = test_manifest();
    let result = validate_frontmatter_against_manifest(path, Some(&frontmatter), &manifest);
    assert!(result.is_some(), "target conflict should produce a diagnostic");
    let diag = result.unwrap();
    assert!(diag.message.contains("[build].target"));
    assert!(diag.message.contains("scena"));
}

#[test]
fn validate_frontmatter_rejects_kind_conflict() {
    let path = Path::new("main.fab");
    let frontmatter =
        parse_file_frontmatter(r#"build = { kind = "lib" }"#).expect("frontmatter");
    let manifest = test_manifest();
    let result = validate_frontmatter_against_manifest(path, Some(&frontmatter), &manifest);
    assert!(result.is_some(), "kind conflict should produce a diagnostic");
    let diag = result.unwrap();
    assert!(diag.message.contains("[build].kind"));
    assert!(diag.message.contains("lib"));
}

#[test]
fn validate_frontmatter_rejects_package_name_conflict() {
    let path = Path::new("main.fab");
    let frontmatter = parse_file_frontmatter(
        "[package]\nname = \"other\"",
    )
    .expect("frontmatter");
    let manifest = test_manifest();
    let result = validate_frontmatter_against_manifest(path, Some(&frontmatter), &manifest);
    assert!(result.is_some(), "name conflict should produce a diagnostic");
    let diag = result.unwrap();
    assert!(diag.message.contains("[package].name"));
}

#[test]
fn validate_frontmatter_rejects_package_version_conflict() {
    let path = Path::new("main.fab");
    let frontmatter = parse_file_frontmatter(
        "[package]\nversion = \"2.0.0\"",
    )
    .expect("frontmatter");
    let manifest = test_manifest();
    let result = validate_frontmatter_against_manifest(path, Some(&frontmatter), &manifest);
    assert!(result.is_some(), "version conflict should produce a diagnostic");
    let diag = result.unwrap();
    assert!(diag.message.contains("[package].version"));
}

#[test]
fn validate_frontmatter_rejects_paths_source_conflict() {
    let path = Path::new("main.fab");
    let frontmatter = parse_file_frontmatter(
        "[paths]\nsource = \"lib\"",
    )
    .expect("frontmatter");
    let manifest = test_manifest();
    let result = validate_frontmatter_against_manifest(path, Some(&frontmatter), &manifest);
    assert!(result.is_some(), "paths.source conflict should produce a diagnostic");
    let diag = result.unwrap();
    assert!(diag.message.contains("[paths].source"));
}

#[test]
fn validate_frontmatter_rejects_paths_entry_conflict() {
    let path = Path::new("main.fab");
    let frontmatter = parse_file_frontmatter(
        "[paths]\nentry = \"other.fab\"",
    )
    .expect("frontmatter");
    let manifest = test_manifest();
    let result = validate_frontmatter_against_manifest(path, Some(&frontmatter), &manifest);
    assert!(result.is_some(), "paths.entry conflict should produce a diagnostic");
    let diag = result.unwrap();
    assert!(diag.message.contains("[paths].entry"));
}

#[test]
fn validate_frontmatter_no_paths_entry_conflict_when_not_set_in_manifest() {
    let toml_str = r#"
[package]
name = "test-package"
version = "1.0.0"
edition = "2026"

[build]
target = "rust"
kind = "bin"

[paths]
source = "src"
"#;
    let manifest: FaberManifest = toml::from_str(toml_str).expect("valid manifest");
    let path = Path::new("main.fab");
    let frontmatter = parse_file_frontmatter(
        "[paths]\nentry = \"custom.fab\"",
    )
    .expect("frontmatter");
    let result = validate_frontmatter_against_manifest(path, Some(&frontmatter), &manifest);
    assert!(result.is_none(), "no conflict when manifest has no entry");
}

// ── merge_entry_test_selection ────────────────────────────────────────────

#[test]
fn merge_entry_test_selection_uses_cli_when_provided() {
    let cli = TestSelection {
        name: Some("test_foo".to_owned()),
        suite: None,
        tag: None,
    };
    let result = merge_entry_test_selection(Some(&cli), None);
    assert_eq!(result.as_ref().and_then(|s| s.name.as_deref()), Some("test_foo"));
}

#[test]
fn merge_entry_test_selection_uses_frontmatter_when_no_cli() {
    let frontmatter = parse_file_frontmatter(
        "[probanda]\ntags = [\"integration\"]",
    )
    .expect("frontmatter");
    let result = merge_entry_test_selection(None, Some(&frontmatter));
    assert!(result.is_some());
    assert_eq!(result.as_ref().and_then(|s| s.tag.as_deref()), Some("integration"));
}

#[test]
fn merge_entry_test_selection_uses_frontmatter_sectio() {
    let frontmatter = parse_file_frontmatter("sectio = \"math\"").expect("frontmatter");
    let result = merge_entry_test_selection(None, Some(&frontmatter));
    assert!(result.is_some());
    assert_eq!(result.as_ref().and_then(|s| s.suite.as_deref()), Some("math"));
}

#[test]
fn merge_entry_test_selection_cli_overrides_frontmatter() {
    let cli = TestSelection {
        name: Some("test_cli".to_owned()),
        suite: None,
        tag: None,
    };
    let frontmatter = parse_file_frontmatter(
        "[probanda]\ntags = [\"frontmatter-tag\"]",
    )
    .expect("frontmatter");
    let result = merge_entry_test_selection(Some(&cli), Some(&frontmatter));
    assert_eq!(
        result.as_ref().and_then(|s| s.name.as_deref()),
        Some("test_cli")
    );
    assert_eq!(result.as_ref().and_then(|s| s.tag.as_deref()), None);
}

#[test]
fn merge_entry_test_selection_cli_suite_overrides_frontmatter_tag() {
    let cli = TestSelection {
        name: None,
        suite: Some("math".to_owned()),
        tag: None,
    };
    let frontmatter = parse_file_frontmatter(
        "[probanda]\ntags = [\"integration\"]",
    )
    .expect("frontmatter");
    let result = merge_entry_test_selection(Some(&cli), Some(&frontmatter));
    assert_eq!(
        result.as_ref().and_then(|s| s.suite.as_deref()),
        Some("math")
    );
    assert_eq!(result.as_ref().and_then(|s| s.tag.as_deref()), None);
}

#[test]
fn merge_entry_test_selection_returns_none_when_no_selection() {
    let result = merge_entry_test_selection(None, None);
    assert!(result.is_none());
}

#[test]
fn merge_entry_test_selection_returns_none_when_cli_fields_all_none_and_no_frontmatter() {
    let cli = TestSelection {
        name: None,
        suite: None,
        tag: None,
    };
    let result = merge_entry_test_selection(Some(&cli), None);
    assert!(result.is_none());
}

// ── frontmatter_manifest_conflict ─────────────────────────────────────────

#[test]
fn frontmatter_manifest_conflict_returns_none_when_values_match() {
    let result = frontmatter_manifest_conflict("file.fab", "target", "rust", "target", "rust");
    assert!(result.is_none());
}

#[test]
fn frontmatter_manifest_conflict_returns_diagnostic_on_mismatch() {
    let result =
        frontmatter_manifest_conflict("file.fab", "target", "scena", "target", "rust");
    assert!(result.is_some());
    let diag = result.unwrap();
    assert!(diag.message.contains("frontmatter target"));
    assert!(diag.message.contains("scena"));
    assert!(diag.message.contains("rust"));
}
