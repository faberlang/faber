//! NR-3 (nary-range-walk) package-link proofs: the rewrite walk enters
//! every range-product extent and the body, so each namespaced library
//! call inside the product survives into the merged program and the
//! linked walk runs row-major.

use super::*;
use crate::mir::{run_package_mir, with_interpreted_lowered_package_mir};
use radix_mir::{MirFunctionId, dump_program};
use radix_mir_lower::lower::LoweredMirUnit;
use radix_mir_runner::BufferHost;
use radix_module::driver::Config;
use std::fs;

const RANGE_UTIL: &str = "@ public\nfn row_max() → int {\n    return 2\n}\n\n@ public\nfn col_max() → int {\n    return 3\n}\n\n@ public\nfn emit_cell(int v) → int {\n    return v\n}\n";

const RANGE_MAIN: &str = "import from \"./util\" * as utilModule\n\nmain {\n    for range 0‥utilModule.row_max(), 0‥utilModule.col_max() let i, j {\n        print utilModule.emit_cell(i * 10 + j)\n    }\n}\n";

fn write_range_product_package(dir: &Path, label: &str) -> PathBuf {
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create package src");
    fs::write(
        dir.join("faber.toml"),
        format!(
            r#"
[package]
name = "{label}"
version = "1.0.0"
edition = "2026"

[paths]
source = "src"
entry = "main.fab"

[build]
kind = "bin"
"#
        ),
    )
    .expect("write faber.toml");
    fs::write(src.join("util.fab"), RANGE_UTIL).expect("write util.fab");
    fs::write(src.join("main.fab"), RANGE_MAIN).expect("write main.fab");
    src.join("main.fab")
}

fn linked_function_id(lowered: &LoweredMirUnit<'_>, name: &str) -> MirFunctionId {
    lowered
        .validated
        .program()
        .functions
        .iter()
        .find(|function| {
            function
                .name
                .is_some_and(|symbol| lowered.interner.resolve(symbol) == name)
        })
        .map_or_else(
            || panic!("linked function {name} must survive into the merged program"),
            |function| function.id,
        )
}

#[test]
fn package_link_rewrites_every_range_product_source_and_the_body() {
    let dir = crate::test_support::test_temp_dir("nary-range-link");
    let entry = write_range_product_package(dir.as_ref(), "nary-range-link");
    let config = Config::default();

    with_interpreted_lowered_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        |lowered| {
            linked_function_id(lowered, "row_max");
            linked_function_id(lowered, "col_max");
            linked_function_id(lowered, "emit_cell");
            let dump = dump_program(lowered.validated.program());
            // The entry's only plain calls are the two extent calls plus
            // the body call: one linked call per rewritten product source
            // and one for the rewritten body.
            let plain_calls = dump.matches(" call ").count();
            assert_eq!(
                plain_calls, 3,
                "both product extents and the body must lower as linked calls:\n{dump}"
            );
        },
    )
    .unwrap_or_else(|errors| panic!("range-product package MIR must link, got {errors:?}"));
}

#[test]
fn package_mir_runs_the_linked_range_product_row_major() {
    let dir = crate::test_support::test_temp_dir("nary-range-run");
    let entry = write_range_product_package(dir.as_ref(), "nary-range-run");
    let config = Config::default();
    let mut host = BufferHost::default();
    run_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        &mut host,
    )
    .expect("linked range-product package must run");
    assert_eq!(host.stdout_lines, ["0", "1", "2", "10", "11", "12"]);
}
