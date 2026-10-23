//! Fixtures for the `FABER_*` / `RADIX_*` environment-read ratchet.
//!
//! Operator ruling 2026-10-14: no `FABER_*` / `RADIX_*` environment variable
//! may be read outside the internal test harness; compiler and product
//! behavior is CLI flags or `faber.toml`. The fixtures below are in-memory
//! sources, not checked-in `.rs` files, because `cargo fmt --check` would
//! rewrite a checked-in fixture.

use radix_hygiene::{EnvRead, SourceFile, is_env_harness_path, scan_tool_env_reads};
use std::path::{Path, PathBuf};

fn source_file(path: &str, src: &str) -> SourceFile {
    SourceFile {
        path: PathBuf::from(path),
        content: src.to_owned(),
        scrubbed: radix_hygiene::scrub_rust_source(src),
    }
}

fn scan(src: &str) -> Vec<EnvRead> {
    scan_tool_env_reads(&[source_file("crates/faber/src/package/mod.rs", src)])
}

#[test]
fn faber_literal_read_is_counted() {
    let body = "fn resolve() {\n    let a = 1;\n    let b = 2;\n    std::env::var(\"FABER_LIBRARY_HOME\")\n}\n";
    let reads = scan(body);

    assert_eq!(reads.len(), 1);
    let read = reads.first().expect("one environment read");
    assert_eq!(read.name, "FABER_LIBRARY_HOME");
    assert_eq!(read.line, 4);
    assert_eq!(read.path, PathBuf::from("crates/faber/src/package/mod.rs"));
}

#[test]
fn radix_var_os_is_counted() {
    let body = "fn shuffle() {\n    std::env::var_os(\"RADIX_RUNNER_SHUFFLE_SEED\");\n}\n";
    let reads = scan(body);

    assert_eq!(reads.len(), 1);
    assert_eq!(
        reads.first().expect("one environment read").name,
        "RADIX_RUNNER_SHUFFLE_SEED"
    );
}

#[test]
fn platform_variables_are_not_counted() {
    let body = "fn locate() {\n    std::env::var(\"PATH\");\n    std::env::var_os(\"HOME\");\n    std::env::var_os(\"XDG_CACHE_HOME\");\n}\n";

    assert!(scan(body).is_empty());
}

#[test]
fn comment_and_string_mentions_are_not_counted() {
    let body = "fn docs() {\n    // std::env::var(\"FABER_X\")\n    let note = \"std::env::var(\\\"FABER_X\\\")\";\n}\n";

    assert!(scan(body).is_empty());
}

#[test]
fn multi_line_call_is_counted() {
    let body = "fn multi() {\n    std::env::var(\n        \"FABER_MULTI\",\n    );\n}\n";
    let reads = scan(body);

    assert_eq!(reads.len(), 1);
    let read = reads.first().expect("one environment read");
    assert_eq!(read.name, "FABER_MULTI");
    assert_eq!(read.line, 2);
}

#[test]
fn indirect_name_binding_is_counted() {
    let body =
        "fn indirect() {\n    let name = \"FABER_INDIRECT\";\n    std::env::var_os(name);\n}\n";
    let reads = scan(body);

    assert_eq!(reads.len(), 1);
    assert_eq!(
        reads.first().expect("one environment read").name,
        "FABER_INDIRECT"
    );
}

#[test]
fn foreign_env_module_is_not_counted() {
    let body =
        "fn foreign() {\n    myenv::var(\"FABER_X\");\n    proto_env::var_os(\"RADIX_Y\");\n}\n";

    assert!(scan(body).is_empty());
}

#[test]
fn harness_paths_are_exempt() {
    for path in [
        "crates/exempla/src/exempla_e2e/runner.rs",
        "crates/faber/tests/corpus_cli.rs",
        "crates/radix-semantic/src/test_support.rs",
        "crates/radix-module/src/driver/mod_test.rs",
        "crates/radix-hir-python/src/golden.rs",
    ] {
        assert!(is_env_harness_path(Path::new(path)), "{path}");
    }

    assert!(!is_env_harness_path(Path::new(
        "crates/faber/src/package/mod.rs"
    )));
}
