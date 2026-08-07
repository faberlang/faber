//! Tests for the reusable package-to-Wasm builder (U6-D).

use super::*;
use crate::package::test_support::{test_temp_dir, TestDir};
use faber_host_wasm::{OutcomeCategory, RunConfig, RunOutcome, WasmRtV1Host};
use radix::codegen::Target;
use std::fs;

const EXTERNAL_SALUTA: &str =
    "__faber_external_product_importa_wasm_module_auxilium_func_saluta";

const FIXTURE_ENTRY: &str = r#"# importa-wasm entry — carrier-typed sibling import (U6-D)
import from "./auxilium" private auxilium

main {
    print auxilium.saluta(41)
}
"#;

const FIXTURE_SIBLING: &str = r#"# importa-wasm auxilium — sibling helper module
functio saluta(numerus n) → numerus {
    return n + 1
}
"#;

/// Write the carrier-typed sibling-import fixture under a temp dir and return
/// the entry path. The fixture directory name is fixed so the product
/// identity (`importa-wasm`) is deterministic.
fn write_importa_wasm_fixture(dir: &TestDir) -> PathBuf {
    let package_dir = dir.join("importa-wasm");
    fs::create_dir_all(&package_dir).expect("create importa-wasm fixture dir");
    fs::write(package_dir.join("auxilium.fab"), FIXTURE_SIBLING).expect("write auxilium.fab");
    fs::write(package_dir.join("importa.fab"), FIXTURE_ENTRY).expect("write importa.fab");
    fs::write(package_dir.join("importa.expected"), "42\n").expect("write expected");
    package_dir.join("importa.fab")
}

fn wasm_config() -> radix::Config {
    radix::Config::default().with_target(Target::MirWasmBinary)
}

fn build_fixture(dir: &TestDir) -> (PathBuf, PackageWasmBuild) {
    let entry = write_importa_wasm_fixture(dir);
    let options = PackageWasmOptions::new(dir.join("out"));
    let build = build_package_wasm(&wasm_config(), &entry, &options)
        .expect("package-to-Wasm build must succeed");
    (entry, build)
}

#[test]
fn package_wasm_builder_emits_one_module_per_unit_with_canonical_symbols() {
    let dir = test_temp_dir("wasm-builder");
    let (entry, build) = build_fixture(&dir);

    assert_eq!(build.product, "importa-wasm");
    assert_eq!(build.entry_unit, entry);

    // Two units — the entry consumer and the imported sibling — one module each.
    assert_eq!(build.modules.len(), 2, "expected two package units");
    let sibling = build
        .modules
        .iter()
        .find(|module| module.module_segments == ["auxilium".to_owned()])
        .expect("sibling unit must be present");
    let entry_module = build
        .modules
        .iter()
        .find(|module| module.is_entry)
        .expect("entry unit must be present");
    assert!(
        sibling.unit_path.ends_with("auxilium.fab"),
        "sibling module must carry its source unit path, got {}",
        sibling.unit_path.display()
    );
    assert!(
        entry_module.unit_path.ends_with("importa.fab"),
        "entry module must carry its source unit path, got {}",
        entry_module.unit_path.display()
    );
    assert!(sibling.wat_path.is_file(), "sibling .wat must be written");
    assert!(
        entry_module.wat_path.is_file(),
        "entry .wat must be written"
    );

    // Cross-unit call parity: the entry declares/calls the sibling's external
    // symbol; the sibling module exports the SAME canonical symbol so the
    // product host (`bind_external_imports`: field F → export `__faber_{F}`)
    // resolves the package import.
    assert!(
        entry_module.wat.contains(EXTERNAL_SALUTA),
        "entry module must declare/call the sibling external symbol:\n{}",
        entry_module.wat
    );
    assert!(
        sibling.wat.contains(&format!("(export \"{EXTERNAL_SALUTA}\")")),
        "sibling module must export the canonical external symbol:\n{}",
        sibling.wat
    );

    // The entry keeps the `incipit` export the product host invokes; the
    // sibling keeps its ordinary export alongside the canonical one.
    assert!(
        entry_module.wat.contains("(export \"incipit\")"),
        "entry module must export incipit:\n{}",
        entry_module.wat
    );
    assert!(
        sibling.wat.contains("(export \"saluta\")"),
        "sibling module must keep its ordinary export:\n{}",
        sibling.wat
    );
    assert!(
        !sibling.wat.contains("(export \"incipit\")"),
        "library module must not export an entry:\n{}",
        sibling.wat
    );

    // Manifest invariants: exactly one entry module, one module per unit, and
    // the byte modules travel in the manifest.
    assert_eq!(
        build.manifest.entry_module, entry_module.wat_path,
        "manifest must record the exactly-one entry module"
    );
    assert_eq!(
        build.manifest.modules.len(),
        2,
        "manifest must list one module per unit"
    );
    assert_eq!(build.manifest.sibling_bytes.len(), 1);
    assert!(!build.manifest.entry_bytes.is_empty());
    assert!(!build.manifest.sibling_bytes[0].is_empty());
}

#[test]
fn package_wasm_build_links_and_runs_through_product_host() {
    let dir = test_temp_dir("wasm-run");
    let (_, build) = build_fixture(&dir);

    let host = WasmRtV1Host::new().expect("portable product engine must initialize");
    let outcome = host.run_package(
        &build.manifest.entry_bytes,
        &build
            .manifest
            .sibling_bytes
            .iter()
            .map(Vec::as_slice)
            .collect::<Vec<_>>(),
        &RunConfig::default(),
    );
    assert_eq!(
        outcome,
        RunOutcome::Success {
            stdout: "42\n".to_owned(),
            stderr: String::new(),
        },
        "the package-aware module set must link and run through the product host, got: {outcome:?}"
    );
    assert_eq!(outcome.category(), OutcomeCategory::Success);
}

#[test]
fn package_wasm_plan_supports_the_representable_fixture() {
    let dir = test_temp_dir("wasm-plan");
    let entry = write_importa_wasm_fixture(&dir);
    let mut package = crate::package::compile::analyze_package(&wasm_config(), &entry)
        .expect("importa-wasm package must analyze");
    let plan = crate::package::artifact_plan::plan_package(&package, Target::MirWasmBinary);
    assert!(plan.supported, "wasm package plan must be supported");
    assert!(plan.rejection.is_none());
    let entry_artifact = plan
        .entry_artifact
        .clone()
        .expect("wasm plan must name an entry artifact");
    assert!(entry_artifact.starts_with("wasm:entry:"));

    let wasm_modules = plan
        .nodes
        .iter()
        .filter(|node| node.target == Some("wasm"))
        .count();
    assert_eq!(wasm_modules, 2, "one module per package unit");
}
