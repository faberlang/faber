//! Package-aware Wasm lane (codex-gap Stage 6 U6-D).
//!
//! The Faber package path accepts `Target::MirWasmBinary`: the reusable
//! package-to-Wasm builder (`faber_cli::package::build_package_wasm`) emits
//! one package-aware module per unit (entry + sibling helpers), and the
//! product host (`faber-host-wasm::WasmRtV1Host::run_package`) instantiates
//! the module set together — resolving the `faber_external` imports the
//! emitter declares for same-package cross-module identities against the
//! siblings' canonical external-symbol exports — and runs the entry.
//!
//! Fixture `importa-wasm` mirrors the `importa` sibling-import shape with a
//! CARRIER-TYPED cross-module call (`saluta(numerus) → numerus`), the
//! recorded deviation per the U6-E routing note: separate module instances
//! own separate linear memories, so a textus HANDLE passed entry ↔ sibling
//! cannot resolve today (the per-module literal table interns rows into the
//! shared host arena in module order). A numerus carrier crosses the module
//! boundary as a plain i64, which the host symbol binding + invocation
//! proves (the U6-E carrier proof: 41 → 42 → stdout).

use faber_host_wasm::{OutcomeCategory, RunConfig, RunOutcome, WasmRtV1Host};
use radix::codegen::Target;
use std::path::PathBuf;

use super::common::{make_temp_root, normalize_newline, read_expected_stdout};
use super::wasm::WasmTier;

/// Fixture package under `faber/corpus/` (`paths::package_corpus_dir`).
const IMPORTA_WASM_FIXTURE: &str = "importa-wasm";

/// The canonical external symbol the entry imports and the sibling exports
/// (`importa-wasm:auxilium:saluta`).
pub(crate) const EXTERNAL_SALUTA: &str =
    "__faber_external_product_importa_wasm_module_auxilium_func_saluta";

fn fixture_entry() -> PathBuf {
    crate::paths::package_corpus_dir()
        .join(IMPORTA_WASM_FIXTURE)
        .join("importa.fab")
}

/// Build the fixture package through the faber package-wasm path and run the
/// module set through the product host. Returns the outcome plus the wasm
/// lane tier evidence for the fixture.
fn run_fixture_package() -> (RunOutcome, WasmTier, String) {
    let entry = fixture_entry();
    let config = radix::Config::default().with_target(Target::MirWasmBinary);
    let temp_root = make_temp_root();
    let options = faber_cli::package::PackageWasmOptions::new(
        temp_root.join("importa-wasm.modules"),
    );
    let build = faber_cli::package::build_package_wasm(&config, &entry, &options)
        .expect("importa-wasm package build must succeed");

    assert_eq!(
        build.modules.len(),
        2,
        "importa-wasm package graph must resolve two units (entry + sibling)"
    );
    let entry_module = build
        .modules
        .iter()
        .find(|module| module.is_entry)
        .expect("entry module");
    let sibling_module = build
        .modules
        .iter()
        .find(|module| !module.is_entry)
        .expect("sibling module");
    assert!(
        entry_module.wat.contains(EXTERNAL_SALUTA),
        "entry module must declare/call the sibling external symbol:\n{}",
        entry_module.wat
    );
    assert!(
        sibling_module
            .wat
            .contains(&format!("(export \"{EXTERNAL_SALUTA}\")")),
        "sibling module must export the canonical external symbol:\n{}",
        sibling_module.wat
    );
    assert_eq!(
        build.manifest.entry_module, entry_module.wat_path,
        "manifest must record the exactly-one entry module"
    );

    let host = WasmRtV1Host::new().expect("portable product engine must initialize");
    let sibling_bytes = build
        .manifest
        .sibling_bytes
        .iter()
        .map(Vec::as_slice)
        .collect::<Vec<_>>();
    let outcome = host.run_package(
        &build.manifest.entry_bytes,
        &sibling_bytes,
        &RunConfig::default(),
    );

    let expected = read_expected_stdout(&entry).expect("fixture must have a sibling .expected");
    let (tier, reason) = match &outcome {
        RunOutcome::Success { stdout, .. } if normalize_newline(stdout) == expected => (
            WasmTier::OutputChecked,
            format!(
                "product host run_package matched sibling .expected ({} lines)",
                expected.lines().count()
            ),
        ),
        RunOutcome::Success { stdout, .. } => (
            WasmTier::Runnable,
            format!(
                "product host run_package succeeded but .expected mismatch: got {:?}, want {:?}",
                stdout, expected
            ),
        ),
        other => (
            WasmTier::CompileValid,
            format!("product host package run produced a typed non-success: {other:?}"),
        ),
    };
    (outcome, tier, reason)
}

/// U6-D done_when core: the package-aware wasm lane reports the fixture at
/// the runnable/output-checked tier, and the product host links + runs the
/// module set with captured stdout matching the sibling `.expected`.
#[test]
fn package_wasm_lane_links_and_runs_importa_wasm() {
    let (outcome, tier, reason) = run_fixture_package();

    assert!(
        tier >= WasmTier::Runnable,
        "the wasm package lane must report a runnable tier for importa-wasm: {reason}"
    );
    assert_eq!(
        outcome.category(),
        OutcomeCategory::Success,
        "importa-wasm package must run to success through the product host, got: {outcome:?}"
    );

    eprintln!("[wasm-package:importa-wasm] tier {tier:?} :: {reason}");
    match &outcome {
        RunOutcome::Success { stdout, stderr } => {
            assert_eq!(
                normalize_newline(stdout),
                "42",
                "importa-wasm captured stdout must match the sibling .expected"
            );
            assert!(stderr.is_empty(), "unexpected stderr: {stderr:?}");
        }
        other => panic!("expected Success, got: {other:?}"),
    }
}

/// U6-D artifact evidence: the package lane writes one `.wat` module per
/// unit (entry + sibling) and the sibling carries the canonical export, so
/// the host `faber_external` resolver has exactly the symbols the entry
/// imports.
#[test]
fn package_wasm_lane_emits_one_module_per_unit() {
    let entry = fixture_entry();
    let config = radix::Config::default().with_target(Target::MirWasmBinary);
    let temp_root = make_temp_root();
    let options = faber_cli::package::PackageWasmOptions::new(temp_root.join("importa-wasm.modules"));
    let build = faber_cli::package::build_package_wasm(&config, &entry, &options)
        .expect("importa-wasm package build must succeed");

    assert_eq!(build.product, "importa-wasm");
    assert_eq!(
        build.manifest.modules.len(),
        2,
        "manifest must list one module per unit"
    );
    assert_eq!(build.manifest.sibling_bytes.len(), 1);
    for module in &build.modules {
        assert!(module.wat_path.is_file(), "{}", module.wat_path.display());
        assert!(!module.bytes.is_empty(), "module bytes must be non-empty");
    }
}
