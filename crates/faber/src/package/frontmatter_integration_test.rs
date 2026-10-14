//! Frontmatter integration tests for package loading and compilation.
//!
//! These tests exercise frontmatter peeling, validation, manifest conflict
//! detection, group/sectio module-tree routing, and test-selection defaults
//! at the `load_package` / `compile_package` API level.
//!
//! Unit-level frontmatter tests live in `frontmatter_test.rs`.

use super::library_resolver_from_config;
use super::test_support::{diagnostic_has_arg, diagnostic_has_issue, test_temp_dir};
#[cfg(feature = "hir-rust")]
use radix::module::Output;
use radix::module::driver::Config;
use radix::program::analyze::AnalysisSeams;
use radix::program::discovery::PackageSpec;
use radix::program::emit::compile_package as compile_package_direct;
#[cfg(feature = "hir-rust")]
use radix::program::emit::{
    compile_package_with_test_options as compile_package_with_test_options_direct,
    compile_package_with_test_selection as compile_package_with_test_selection_direct,
};
#[cfg(feature = "hir-rust")]
use radix::program::frontmatter::RustTestSelection as TestSelection;
use radix::program::library::LibraryResolver;
#[cfg(feature = "hir-rust")]
use radix::program::test_source_filter::TestSourceFilter;
use std::fs;
use std::path::Path;

fn seams() -> AnalysisSeams {
    crate::package::faber_analysis_seams()
}

fn compile_with_faber_seams(config: &Config, input: &Path) -> radix::module::CompileResult {
    compile_package_direct(config, input, &crate::package::faber_emission_seams())
}

#[cfg(feature = "hir-rust")]
fn compile_with_test_selection(
    config: &Config,
    input: &Path,
    selection: Option<&TestSelection>,
) -> radix::module::CompileResult {
    compile_package_with_test_selection_direct(
        config,
        input,
        selection,
        &crate::package::faber_emission_seams(),
    )
}

#[cfg(feature = "hir-rust")]
fn compile_with_test_options(
    config: &Config,
    input: &Path,
    selection: Option<&TestSelection>,
    filter: Option<&TestSourceFilter>,
) -> radix::module::CompileResult {
    compile_package_with_test_options_direct(
        config,
        input,
        selection,
        filter,
        &crate::package::faber_emission_seams(),
    )
}

fn discover_package(
    input: &Path,
) -> Result<PackageSpec, Box<radix::module::diagnostics::Diagnostic>> {
    crate::package::discover_package(input)
}

fn load_with_faber_seams(
    spec: &PackageSpec,
    resolver: &LibraryResolver,
) -> Result<Vec<radix::program::PackageFile>, Vec<radix::module::diagnostics::Diagnostic>> {
    let config = Config::default();
    radix::program::analyze::load_package_with_config(
        spec,
        resolver,
        &config,
        false,
        None,
        &seams(),
    )
}

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

main { print "peeled" }
"#,
    )
    .expect("write entry");

    let config = Config::default();
    let spec = discover_package(&entry).expect("package");
    let files = load_with_faber_seams(&spec, &library_resolver_from_config(&config)).expect("load");
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

#[test]
fn load_package_selects_reader_pack_per_file_before_lexing() {
    let dir = test_temp_dir("frontmatter-per-file-locale");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"import from "./latin" latin

main {
    print latin.salutatio()
}
"#,
    )
    .expect("write English entry");
    let latin = dir.join("latin.fab");
    fs::write(
        &latin,
        r#"+++
locale = "la"
+++

functio salutatio() → textus {
    redde "salve"
}
"#,
    )
    .expect("write Latin sibling");

    let config = Config::default();
    let spec = discover_package(&entry).expect("package");
    let files = load_with_faber_seams(&spec, &library_resolver_from_config(&config)).expect("load");
    let entry_file = files
        .iter()
        .find(|file| file.path == entry)
        .expect("entry file");
    let latin_file = files
        .iter()
        .find(|file| file.path == latin)
        .expect("Latin file");

    assert_eq!(
        entry_file
            .locale_pack
            .as_ref()
            .map(|pack| pack.metadata.id.as_str()),
        Some("en")
    );
    assert_eq!(
        latin_file
            .locale_pack
            .as_ref()
            .map(|pack| pack.metadata.id.as_str()),
        Some("la")
    );
}

#[test]
fn package_diagnostics_use_physical_frontmatter_lines() {
    let dir = test_temp_dir("frontmatter-diagnostic-lines");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        "+++\nlocale = \"en\"\nsectio = \"smoke\"\n+++\n\nmain {\n    print missing\n}\n",
    )
    .expect("write diagnostic fixture");

    let result = compile_with_faber_seams(&Config::default(), &entry);
    let diagnostic = result
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == Some("SEM008"))
        .expect("undefined name diagnostic");
    assert_eq!(diagnostic.line, Some(7));
    assert_eq!(diagnostic.source_line.as_deref(), Some("    print missing"));
    assert!(diagnostic.file.ends_with("main.fab"));
    assert!(diagnostic.phase.to_string().contains("analysis"));
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
        r"+++
sectio = 
+++

main {}
",
    )
    .expect("write entry");

    let config = Config::default();
    let spec = discover_package(&entry).expect("package");
    let result = load_with_faber_seams(&spec, &library_resolver_from_config(&config));
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

main {}
"#,
    )
    .expect("entry");

    let config = Config::default();
    let spec = discover_package(dir.path()).expect("package");
    let result = load_with_faber_seams(&spec, &library_resolver_from_config(&config));
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

#[cfg(feature = "hir-rust")]
#[test]
fn compile_package_honors_group_frontmatter_for_module_tree() {
    let dir = test_temp_dir("frontmatter-group");
    fs::write(
        dir.join("main.fab"),
        r#"
import from "./lib" lib

main {
    print lib.answer()
}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("lib.fab"),
        r#"+++
group = "custom.lib"
+++

@ public
fn answer() → int {
    return 42
}
"#,
    )
    .expect("write lib");

    let result = compile_with_faber_seams(&Config::default(), &dir.join("main.fab"));
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

#[cfg(feature = "hir-rust")]
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

test "name match" {
    assert true
}

describe "outer suite" {
    describe "inner suite" {
        test "wrong tag" tag "smoke" {
            assert true
        }

        test "combined match" tag "focus" {
            assert true
        }
    }
}

main {}
"#,
    )
    .expect("write entry");

    let result = compile_with_test_selection(&Config::default(), &entry, None);
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

    assert!(
        output
            .code
            .contains("#[ignore = \"faber: not selected by suite outer suite/inner suite\"]")
    );
    assert!(
        output
            .code
            .contains("#[ignore = \"faber: not selected by tag focus\"]")
    );
}

#[cfg(feature = "hir-rust")]
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

test "tag match" tag "smoke" {
    assert true
}

test "other tag" tag "focus" {
    assert true
}

main {}
"#,
    )
    .expect("write entry");

    let selection = TestSelection {
        tag: Some("smoke".to_owned()),
        ..TestSelection::default()
    };
    let result = compile_with_test_selection(&Config::default(), &entry, Some(&selection));
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
    assert!(
        output
            .code
            .contains("#[ignore = \"faber: not selected by tag smoke\"]")
    );
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

main { print "empty-frontmatter" }
"#,
    )
    .expect("write entry");

    let config = Config::default();
    let spec = discover_package(&entry).expect("package");
    let files = load_with_faber_seams(&spec, &library_resolver_from_config(&config)).expect("load");
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

main { print "comment-frontmatter" }
"#,
    )
    .expect("write entry");

    let config = Config::default();
    let spec = discover_package(&entry).expect("package");
    let files = load_with_faber_seams(&spec, &library_resolver_from_config(&config)).expect("load");
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

#[cfg(feature = "hir-rust")]
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
        r"@ public
class Vector3 {
    var f32 x
    var f32 y
    var f32 z
}

@ public
fn vector3(f32 x, f32 y, f32 z) → Vector3 {
    return Vector3 { x = x, y = y, z = z }
}
",
    )
    .expect("math.fab");
    fs::write(
        src.join("math.proba"),
        r#"import from "./math" math

test "vector3 builds" {
    const math.Vector3 v ← math.vector3(1.0, 2.0, 3.0)
    assert v.x ≡ (1.0 ∷ f32)
}
"#,
    )
    .expect("math.proba");

    let result = compile_with_test_options(&Config::default(), dir.path(), None, None);
    assert!(
        result.success(),
        "lib package without paths.entry should compile for faber test, got {:?}",
        result.diagnostics
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };
    // The synthetic crate root is a library root (web4): no `main`; the package
    // builder writes it as `src/lib.rs`.
    assert!(
        output.code.contains("Generated library package crate root"),
        "expected synthetic library crate root:\n{}",
        output.code
    );
    assert!(
        !output.code.contains("fn main"),
        "a library crate root carries no main:\n{}",
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
