//! Frontmatter integration tests for package loading and compilation.
//!
//! These tests exercise frontmatter peeling, validation, manifest conflict
//! detection, group/sectio module-tree routing, and test-selection defaults
//! at the `load_package` / `compile_package` API level.
//!
//! Unit-level frontmatter tests live in `frontmatter_test.rs`.

use super::test_support::{diagnostic_has_arg, diagnostic_has_issue, test_temp_dir};
use super::{
    compile_package, compile_package_with_test_options, compile_package_with_test_selection,
    discover_package, library_resolver_from_config, load_package,
};
use radix::codegen::rust::TestSelection;
use radix::driver::Config;
use radix::Output;
use std::fs;

// ---------------------------------------------------------------------------
// Frontmatter peeling
// ---------------------------------------------------------------------------

#[test]
fn load_package_peels_frontmatter_before_parse() {
    let dir = test_temp_dir("frontmatter-peel");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"+++
sectio = "smoke"
group = "demo.entry"
+++

incipit { nota "peeled" }
"#,
    )
    .expect("write entry");

    let config = Config::default();
    let spec = discover_package(&entry).expect("package");
    let files = load_package(&spec, &library_resolver_from_config(&config)).expect("load");
    let file = files
        .iter()
        .find(|file| file.path == entry)
        .expect("entry file");

    assert!(!file.source.contains("+++"));
    assert!(!file.source.contains("sectio"));
    assert!(file.raw_source.contains("+++"));
    assert_eq!(
        file.frontmatter.as_ref().and_then(|fm| fm.sectio()),
        Some("smoke")
    );
    assert_eq!(
        file.frontmatter.as_ref().and_then(|fm| fm.group()),
        Some("demo.entry")
    );
    assert_eq!(file.module_segments, vec!["demo", "entry"]);
}

// ---------------------------------------------------------------------------
// Frontmatter validation
// ---------------------------------------------------------------------------

#[test]
fn load_package_rejects_invalid_frontmatter_toml() {
    let dir = test_temp_dir("frontmatter-invalid");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"+++
sectio = 
+++

incipit {}
"#,
    )
    .expect("write entry");

    let config = Config::default();
    let spec = discover_package(&entry).expect("package");
    let result = load_package(&spec, &library_resolver_from_config(&config));
    assert!(result.is_err());
    let err = result.err().expect("diagnostics");
    assert!(err.iter().any(|diag| diag.code == Some("PARSE052")));
}

#[test]
fn load_package_rejects_frontmatter_manifest_build_conflict() {
    let dir = test_temp_dir("frontmatter-manifest-conflict");
    fs::create_dir_all(dir.join("src")).expect("src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "conflict-demo"

[paths]
entry = "main.fab"

[build]
target = "rust"
"#,
    )
    .expect("manifest");
    fs::write(
        dir.join("src/main.fab"),
        r#"+++
[build]
target = "ts"
+++

incipit {}
"#,
    )
    .expect("entry");

    let config = Config::default();
    let spec = discover_package(dir.path()).expect("package");
    let result = load_package(&spec, &library_resolver_from_config(&config));
    assert!(result.is_err());
    let err = result.err().expect("diagnostics");
    assert!(err.iter().any(|diag| {
        diagnostic_has_issue(diag, "frontmatter_manifest_override")
            && diagnostic_has_arg(diag, "frontmatter", "[build].target")
            && diagnostic_has_arg(diag, "frontmatter_value", "ts")
            && diagnostic_has_arg(diag, "manifest", "target")
            && diagnostic_has_arg(diag, "manifest_value", "rust")
    }));
}

// ---------------------------------------------------------------------------
// Frontmatter group / module tree
// ---------------------------------------------------------------------------

#[test]
fn compile_package_honors_group_frontmatter_for_module_tree() {
    let dir = test_temp_dir("frontmatter-group");
    fs::write(
        dir.join("main.fab"),
        r#"
importa ex "./lib" privata lib

incipit {
    nota lib.answer()
}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("lib.fab"),
        r#"+++
group = "custom.lib"
+++

functio answer() → numerus {
    redde 42
}
"#,
    )
    .expect("write lib");

    let result = compile_package(&Config::default(), &dir.join("main.fab"));
    assert!(
        result.success(),
        "expected group frontmatter package compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };

    assert!(output.code.contains("pub mod custom"));
    assert!(output.code.contains("pub mod lib"));
}

// ---------------------------------------------------------------------------
// Frontmatter test selection defaults
// ---------------------------------------------------------------------------

#[test]
fn compile_package_applies_entry_frontmatter_test_selection_defaults() {
    let dir = test_temp_dir("frontmatter-test-defaults");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"+++
sectio = "outer suite/inner suite"

[probanda]
tags = ["focus"]
+++

proba "name match" {
    adfirma verum
}

probandum "outer suite" {
    probandum "inner suite" {
        proba "wrong tag" tag "smoke" {
            adfirma verum
        }

        proba "combined match" tag "focus" {
            adfirma verum
        }
    }
}

incipit {}
"#,
    )
    .expect("write entry");

    let result = compile_package_with_test_selection(&Config::default(), &entry, None);
    assert!(
        result.success(),
        "expected frontmatter test-default compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };

    assert!(output
        .code
        .contains("#[ignore = \"faber: not selected by suite outer suite/inner suite\"]"));
    assert!(output
        .code
        .contains("#[ignore = \"faber: not selected by tag focus\"]"));
}

#[test]
fn compile_package_cli_test_selection_overrides_entry_frontmatter_defaults() {
    let dir = test_temp_dir("frontmatter-test-cli-override");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"+++
sectio = "outer suite/inner suite"

[probanda]
tags = ["focus"]
+++

proba "tag match" tag "smoke" {
    adfirma verum
}

proba "other tag" tag "focus" {
    adfirma verum
}

incipit {}
"#,
    )
    .expect("write entry");

    let selection = TestSelection {
        tag: Some("smoke".to_owned()),
        ..TestSelection::default()
    };
    let result = compile_package_with_test_selection(&Config::default(), &entry, Some(&selection));
    assert!(
        result.success(),
        "expected CLI override compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };

    assert!(!output.code.contains("not selected by suite"));
    assert!(output
        .code
        .contains("#[ignore = \"faber: not selected by tag smoke\"]"));
}

// ---------------------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------------------

#[test]
fn load_package_accepts_empty_frontmatter_delimiters() {
    let dir = test_temp_dir("frontmatter-empty");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"+++
+++

incipit { nota "empty-frontmatter" }
"#,
    )
    .expect("write entry");

    let config = Config::default();
    let spec = discover_package(&entry).expect("package");
    let files = load_package(&spec, &library_resolver_from_config(&config)).expect("load");
    let file = files
        .iter()
        .find(|file| file.path == entry)
        .expect("entry file");

    // An empty frontmatter block produces Some(FileFrontmatter({}));
    // the source body is still peeled of the delimiter lines.
    assert!(
        file.frontmatter.is_some(),
        "empty frontmatter produces an empty map"
    );
    assert!(!file.source.contains("+++"));
}

#[test]
fn load_package_accepts_frontmatter_with_only_comment_fields() {
    let dir = test_temp_dir("frontmatter-comment-only");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"+++
# this is a comment
# another comment
+++

incipit { nota "comment-frontmatter" }
"#,
    )
    .expect("write entry");

    let config = Config::default();
    let spec = discover_package(&entry).expect("package");
    let files = load_package(&spec, &library_resolver_from_config(&config)).expect("load");
    let file = files
        .iter()
        .find(|file| file.path == entry)
        .expect("entry file");

    // Comments-only frontmatter produces Some(FileFrontmatter({}));
    assert!(
        file.frontmatter.is_some(),
        "comment-only frontmatter produces an empty map"
    );
    assert!(!file.source.contains("+++"));
}

// ---------------------------------------------------------------------------
// Library packages without paths.entry
// ---------------------------------------------------------------------------

#[test]
fn compile_lib_package_without_entry_synthesizes_harness_for_proba() {
    let dir = test_temp_dir("lib-no-entry-proba");
    let src = dir.path().join("src");
    fs::create_dir_all(&src).expect("src");
    fs::write(
        dir.path().join("faber.toml"),
        r#"[package]
name = "leafy"
version = "0.1.0"
edition = "2026"

[library]
provider = "leafy"

[paths]
source = "src"

[build]
kind = "lib"
targets = ["rust"]
"#,
    )
    .expect("manifest");
    fs::write(
        src.join("math.fab"),
        r#"genus Vector3 {
    f32 x
    f32 y
    f32 z
}

functio vector3(f32 x, f32 y, f32 z) → Vector3 {
    redde Vector3 { x = x, y = y, z = z }
}
"#,
    )
    .expect("math.fab");
    fs::write(
        src.join("math.proba"),
        r#"importa ex "./math" privata math

proba "vector3 builds" {
    fixum math.Vector3 v ← math.vector3(1.0, 2.0, 3.0)
    adfirma v.x ≡ (1.0 ∷ f32)
}
"#,
    )
    .expect("math.proba");

    let result = compile_package_with_test_options(&Config::default(), dir.path(), None, None);
    assert!(
        result.success(),
        "lib package without paths.entry should compile for faber test, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (&diag.message, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };
    assert!(
        output.code.contains("fn main() {}"),
        "expected synthetic library harness entry:\n{}",
        output.code
    );
    assert!(
        output.code.contains("pub mod math"),
        "expected product module in crate:\n{}",
        output.code
    );
    assert!(
        output.code.contains("pub mod math_proba") || output.code.contains("#[test]"),
        "expected proba tests in crate:\n{}",
        output.code
    );
}
