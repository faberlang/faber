//! B10: a library module analyzed as a dependency (package mode) must remap
//! imported union variant payloads that name a nominal from a module it does
//! not import directly.
//!
//! `consumer` imports only `tensor`; `tensor.TensorError.ShapeFailure` carries
//! a `shape.ShapeError` payload. File mode already installs the transitive
//! `shape` interface; the library-cache analysis used when a package imports
//! `lib10:consumer` installed only direct imports, so the payload failed
//! remap and the variant lost its own field (arity 1 instead of shared
//! `message` + `inner`) and dropped out of the coverage variant list.

use super::*;
use crate::analyze::analyze_package;
use std::fs;
use std::path::Path;

const LIB_MANIFEST: &str = r#"
[package]
name = "lib10"
version = "1.0.0"
edition = "2026"

[library]
provider = "lib10"

[paths]
source = "src"

[build]
kind = "lib"
targets = ["fmir"]

[reader]
locale = "en"
"#;

const SHAPE_SOURCE: &str = "
@ public
union ShapeError {
    @ shared
    string message

    Bad
}
";

const TENSOR_SOURCE: &str = r#"
import from "lib10:shape" shape

@ public
union TensorError {
    @ shared
    string message

    InvalidShape,
    ShapeFailure {
        shape.ShapeError inner
    }
}
"#;

fn consumer_source(shape_failure_arm: &str) -> String {
    format!(
        r#"
import from "lib10:tensor" tensor

@ public
fn describe(tensor.TensorError err) → string {{
    match err {{
        case tensor.TensorError.InvalidShape let message {{
            return message
        }}
{shape_failure_arm}
    }}
}}
"#
    )
}

const MAIN_SOURCE: &str = r#"+++
locale = "en"
+++
import from "lib10:consumer" consumer
import from "lib10:tensor" tensor

main {
    print consumer.describe(variant tensor.TensorError.InvalidShape { message = "x" })
}
"#;

/// Write the three-module library plus a consuming package; return the
/// package entry path.
fn write_fixture(dir: &Path, shape_failure_arm: &str) -> std::path::PathBuf {
    let lib_root = dir.join("libhome").join("lib10");
    let lib_src = lib_root.join("src");
    fs::create_dir_all(&lib_src).expect("create lib10 src");
    fs::write(lib_root.join("faber.toml"), LIB_MANIFEST).expect("write lib10 manifest");
    fs::write(lib_src.join("shape.fab"), SHAPE_SOURCE).expect("write shape.fab");
    fs::write(lib_src.join("tensor.fab"), TENSOR_SOURCE).expect("write tensor.fab");
    fs::write(
        lib_src.join("consumer.fab"),
        consumer_source(shape_failure_arm),
    )
    .expect("write consumer.fab");
    let app = dir.join("app");
    fs::create_dir_all(&app).expect("create app");
    let entry = app.join("main.fab");
    fs::write(&entry, MAIN_SOURCE).expect("write main.fab");
    entry
}

fn error_codes(dir: &Path, shape_failure_arm: &str) -> Vec<(Option<String>, Option<String>)> {
    let entry = write_fixture(dir, shape_failure_arm);
    let config = Config::default().with_library_home(dir.join("libhome"));
    match analyze_package(&config, &entry, &crate::test_support::test_analysis_seams()) {
        Ok(_) => Vec::new(),
        Err(diagnostics) => diagnostics
            .iter()
            .filter(|diag| diag.is_error())
            .map(|diag| {
                (
                    diag.code.map(str::to_owned),
                    diag.issue().map(str::to_owned),
                )
            })
            .collect(),
    }
}

const SHARED_AND_OWN_ARM: &str = "
        case tensor.TensorError.ShapeFailure let message, _ {
            return message
        }";

const SHARED_ONLY_ARM: &str = "
        case tensor.TensorError.ShapeFailure let message {
            return message
        }";

#[test]
fn library_dependency_binds_shared_and_transitive_payload_field() {
    let dir = crate::test_support::test_temp_dir("b10-shared-and-own");
    let errors = error_codes(&dir, SHARED_AND_OWN_ARM);
    assert!(
        errors.is_empty(),
        "shared `message` + transitive `inner` must bind in a library dependency: {errors:?}"
    );
}

#[test]
fn library_dependency_rejects_shared_only_binding_arity() {
    let dir = crate::test_support::test_temp_dir("b10-shared-only");
    let errors = error_codes(&dir, SHARED_ONLY_ARM);
    assert!(
        errors
            .iter()
            .any(|(_, issue)| issue.as_deref() == Some("variant_pattern_arity_mismatch")),
        "ShapeFailure binds two fields; one pattern is an arity mismatch: {errors:?}"
    );
}

#[test]
fn library_dependency_coverage_requires_transitive_payload_variant() {
    let dir = crate::test_support::test_temp_dir("b10-missing-arm");
    let errors = error_codes(&dir, "");
    assert!(
        errors
            .iter()
            .any(|(_, issue)| issue.as_deref() == Some("missing_variants")),
        "a match missing ShapeFailure must name the missing variant: {errors:?}"
    );
}
