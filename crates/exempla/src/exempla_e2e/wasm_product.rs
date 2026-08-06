//! Exempla product-runner adapter: the portable `faber-host-wasm` runner
//! mapped to Wasm tier evidence and Stage 1 ledger outcomes.
//!
//! Stage 2 of the wasm-host-parity campaign replaces product use of the stub
//! host and externally reconstructed handle maps with the portable product
//! runner. This module is the one product-owned path to `outcome-checked`:
//! when the portable host runs a real compiler artifact to success and its
//! captured stdout matches the sibling `.expected`, the row is proven parity
//! (`wasm_ledger::row_from_measurement` picks the boosted tier up through
//! `wasm::classify_wasm_exemplum`).

use super::common::{normalize_newline, read_expected_stdout};
use super::wasm::WasmTier;
use faber_host_wasm::{RunConfig, RunOutcome, WasmRtV1Host};
use radix::driver::Session;
use std::path::Path;

/// Entry the portable runner invokes for corpus fixtures (matches the stub
/// host's `ENTRY_EXPORT`).
pub(crate) const PRODUCT_ENTRY: &str = "incipit";

/// Run Wasm bytes through the portable product runner with the corpus entry.
/// Returns `None` only when the portable engine cannot be created.
pub(crate) fn run_product(wasm_bytes: &[u8]) -> Option<RunOutcome> {
    let host = WasmRtV1Host::new().ok()?;
    Some(host.run(wasm_bytes, &RunConfig::default()))
}

/// Tier boost for the Wasm ladder: run the module through the portable
/// product host. A [`RunOutcome::Success`] whose captured stdout matches the
/// sibling `.expected` reaches `outcome-checked` without a stub host or an
/// opaque-handle table. Every other outcome falls through to the existing
/// stub-host classification unchanged.
pub(crate) fn portable_product_boost(
    fab_file: &Path,
    wasm_bytes: &[u8],
) -> Option<(WasmTier, String)> {
    let outcome = run_product(wasm_bytes)?;
    let RunOutcome::Success { stdout, .. } = outcome else {
        return None;
    };
    let expected = read_expected_stdout(fab_file)?;
    if normalize_newline(&stdout) != expected {
        return None;
    }
    Some((
        WasmTier::OutputChecked,
        format!(
            "portable product runner matched sibling .expected ({} lines)",
            expected.lines().count()
        ),
    ))
}

/// Emit the compiler Wasm binary for a corpus fixture (in-process, same
/// probe surface as `wasm::classify_wasm_exemplum`).
fn emit_wasm_bytes(session: &Session, fab_path: &Path) -> Vec<u8> {
    let source = std::fs::read_to_string(fab_path).expect("fixture source must be readable");
    let mut analysis = radix::driver::analyze_source(
        session,
        &fab_path.display().to_string(),
        &source,
    )
    .expect("proof fixture must analyze");
    let mir = radix::mir::lower_analyzed_unit_with_context(&mut analysis)
        .expect("proof fixture must lower to MIR");
    // Use the lowered unit's complete symbol table (lowering interns symbols
    // the analysis interner lacks; the wasm literal table resolves them).
    radix::mir::emit_wasm_text_and_binary_probe_with_context(&mir.validated, &mir.interner)
        .expect("proof fixture must emit Wasm")
        .1
}

/// Parse synthetic reject-module WAT to Wasm bytes so the runner receives
/// only Wasm bytes (its input contract never includes WAT).
fn wat_bytes(wat: &str) -> Vec<u8> {
    wat::parse_str(wat).expect("synthetic reject module must parse")
}

/// Stage 2 proof: real Stage-1 ledger artifacts run through the portable
/// product runner and match their Rust oracle outcomes, and the explicit
/// reject cases produce typed validation/import/link/entry/trap/runtime
/// outcomes.
#[test]
fn wasm_product() {
    let session = super::wasm::wasm_session();

    // Proof fixtures selected from the Stage 1 ledger: both emit only scalar
    // `faber_rt_v1` diagnostics, so their execution needs no externally
    // reconstructed opaque-handle table.
    let proofs: &[(&str, &str)] = &[
        ("sic/sic.fab", "9"),
        ("per/per.fab", "0\n2\n4\n6"),
    ];
    for (fab_rel, oracle_stdout) in proofs {
        let fab_path = crate::paths::corpus_dir().join(fab_rel);
        let wasm_bytes = emit_wasm_bytes(&session, &fab_path);
        let outcome =
            run_product(&wasm_bytes).expect("portable product engine must initialize");
        let RunOutcome::Success { stdout, .. } = &outcome else {
            panic!(
                "{fab_rel} must run to success through the portable runner, got: {outcome:?}"
            );
        };
        assert_eq!(
            normalize_newline(stdout),
            normalize_newline(oracle_stdout),
            "{fab_rel} stdout must match the Rust oracle outcome"
        );
    }

    // Reject proof: legacy module, unknown field/signature, and missing entry
    // each produce explicit typed outcomes; validation/link/trap/runtime
    // distinctions are preserved.
    let host = WasmRtV1Host::new().expect("portable product engine must initialize");
    let default = RunConfig::default();

    let legacy = wat_bytes(r#"
(module
  (import "faber_diag" "nota_i64" (func $legacy (param i64)))
  (func (export "incipit") (call $legacy (i64.const 1)))
)
"#);
    let outcome = host.run(&legacy, &default);
    assert!(
        matches!(
            &outcome,
            RunOutcome::ImportRejected { module, field, .. }
                if module == "faber_diag" && field == "nota_i64"
        ),
        "legacy import module must be an explicit import rejection, got: {outcome:?}"
    );

    // W14: the tensor family is now admitted, so the genuinely unadmitted
    // field moves to `tensor_div` (the emitter fails closed on the division
    // row — no host binding exists for it).
    let unknown_field = wat_bytes(r#"
(module
  (import "faber_rt_v1" "__faber_rt_v1_tensor_div" (func $len (param i32) (result i32)))
  (func (export "incipit") (drop (call $len (i32.const 0))))
)
"#);
    let outcome = host.run(&unknown_field, &default);
    assert!(
        matches!(
            &outcome,
            RunOutcome::ImportRejected { field, .. } if field == "__faber_rt_v1_tensor_div"
        ),
        "unknown v1 field must be an explicit import rejection, got: {outcome:?}"
    );

    let bad_signature = wat_bytes(r#"
(module
  (import "faber_rt_v1" "__faber_rt_v1_diagnostic_nota_i64" (func $nota (param i32)))
  (func (export "incipit") (call $nota (i32.const 1)))
)
"#);
    let outcome = host.run(&bad_signature, &default);
    assert_eq!(
        outcome.category(),
        faber_host_wasm::OutcomeCategory::LinkFailed,
        "declared signature conflicting with the admitted binding must be LinkFailed, got: {outcome:?}"
    );

    let missing_entry = wat_bytes(r#"
(module
  (func (export "other") (return))
)
"#);
    let outcome = host.run(&missing_entry, &default);
    assert!(
        matches!(&outcome, RunOutcome::EntryMissing { entry } if entry == "incipit"),
        "missing entry must be an explicit entry-missing outcome, got: {outcome:?}"
    );
}

/// W13 per-fixture runner probes for the closed Stage 5B/6 collection/scalar
/// display rows: each fixture emits as a real compiler artifact, runs through
/// the portable product runner, and its captured stdout matches the sibling
/// `.expected` byte for byte (the same evidence the ledger's
/// `outcome-checked` rows record). The set is the collection/scalar display
/// cluster: lista/tabula/copia construction and display, array index/option
/// reads, scalar (bivalens/option) display, and the closely-adjacent assert
/// rows the same fixtures route through.
#[test]
fn wasm_collection_scalar_cluster_runs_through_the_product_host() {
    let session = super::wasm::wasm_session();
    let fixtures: &[&str] = &[
        // lista construction + scalar reads + display.
        "lista/lista.fab",
        "lista/methodi-functionales.fab",
        "destructura/lista.fab",
        "sparge/sparge.fab",
        "ceteri/ceteri.fab",
        "clausa/clausa.fab",
        "in/in.fab",
        "itera/ex.fab",
        "itera/in-functione.fab",
        // scalar display: bivalens, option/null, format/convert rows.
        "literalia/boolean.fab",
        "literalia/nihil.fab",
        "binarius/binarius.fab",
        "vel/vel.fab",
        "aut/aut.fab",
        "varia/typi-ligata.fab",
        "conversio/bivalens.fab",
        "intrinseca/fractus-approximata.fab",
        "operatores/numeric-value-eq.fab",
        "operatores/comparatio.fab",
        // assert rows (`adfirma` smoke fixtures exit 0 with pinned stdout).
        "dum/dum.fab",
        "et/et.fab",
        "falsum/falsum.fab",
        "fixum/fixum.fab",
        "non/non.fab",
        "si/si.fab",
        "verum/verum.fab",
    ];
    for rel in fixtures {
        let fab_path = crate::paths::corpus_dir().join(rel);
        let wasm_bytes = emit_wasm_bytes(&session, &fab_path);
        let outcome = run_product(&wasm_bytes).expect("portable product engine must initialize");
        let expected = read_expected_stdout(&fab_path)
            .unwrap_or_else(|| panic!("{rel} must have a sibling .expected"));
        match &outcome {
            RunOutcome::Success { stdout, .. } => {
                assert_eq!(
                    normalize_newline(stdout),
                    expected,
                    "{rel} captured stdout must match the sibling .expected"
                );
            }
            other => panic!(
                "{rel} must run to success through the portable runner, got: {other:?}"
            ),
        }
    }
}


/// W14 per-fixture runner probes for the closed tensor display rows: each
/// fixture emits as a real compiler artifact, runs through the portable
/// product runner, and its captured stdout matches the sibling `.expected`
/// byte for byte (the same evidence the ledger's `outcome-checked` rows
/// record). The set is the tensor display cluster: construction
/// (`vacua`/`tensor_new`, `strue`/`tensor_from_flat`, `crea`/`tensor_create`,
/// `reple`/`tensor_fill`), reads (`longitudo`/`tensor_rank`,
/// `magnitudines`/`tensor_shape`, `accipe`/`tensor_get`), writes
/// (`ponde`/`tensor_set`), transforms (`forma`/`tensor_reshape`,
/// `sectio`/`tensor_slice`, `materialize`, `planata`/`tensor_flatten`),
/// arithmetic (`addita`/`subtrahe`/`multiplica`, `matmul`), reductions
/// (`summa`/`tensor_sum`, `media`/`tensor_mean`), the element-width convert
/// (`tensor_convert`), and the text-element tensor (`tensor<textus, [N]>`).
#[test]
fn wasm_tensor_display_cluster_runs_through_the_product_host() {
    let session = super::wasm::wasm_session();
    let fixtures: &[&str] = &[
        // Tensor construction/read/write/transform display.
        "tensor/decl.fab",
        "tensor/shape.fab",
        "tensor/bracket-access.fab",
        "tensor/index-width.fab",
        "tensor/method-policy.fab",
        "tensor/textus.fab",
        // Tensor arithmetic + reductions.
        "tensor/arithmetic-elementwise.fab",
        "tensor/arithmetic-matmul.fab",
        "tensor/arithmetic-reduction.fab",
        // Lista↔tensor conversio bridges.
        "conversio/tensor.fab",
        "conversio/lista-tensor-shaped.fab",
        "conversio/rectangular-lista-literal-tensor.fab",
    ];
    for rel in fixtures {
        let fab_path = crate::paths::corpus_dir().join(rel);
        let wasm_bytes = emit_wasm_bytes(&session, &fab_path);
        let outcome = run_product(&wasm_bytes).expect("portable product engine must initialize");
        let expected = read_expected_stdout(&fab_path)
            .unwrap_or_else(|| panic!("{rel} must have a sibling .expected"));
        match &outcome {
            RunOutcome::Success { stdout, .. } => {
                assert_eq!(
                    normalize_newline(stdout),
                    expected,
                    "{rel} captured stdout must match the sibling .expected"
                );
            }
            other => panic!(
                "{rel} must run to success through the portable runner, got: {other:?}"
            ),
        }
    }
}


/// W12 per-fixture runner probes for the closed Stage 5B text/diagnostic
/// rows: each fixture emits as a real compiler artifact, runs through the
/// portable product runner, and its captured stdout matches the sibling
/// `.expected` byte for byte (the same evidence the ledger's
/// `outcome-checked` rows record).
#[test]
fn wasm_text_diagnostic_cluster_runs_through_the_product_host() {
    let session = super::wasm::wasm_session();
    let fixtures: &[&str] = &[
        "literalia/ascii.fab",
        "literalia/textus.fab",
        "literalia/regex.fab",
        "nota/nota.fab",
        "octeti/octeti.fab",
        "vide/vide.fab",
        "literalia/block-string.fab",
        "mone/mone.fab",
    ];
    for rel in fixtures {
        let fab_path = crate::paths::corpus_dir().join(rel);
        let wasm_bytes = emit_wasm_bytes(&session, &fab_path);
        let outcome = run_product(&wasm_bytes).expect("portable product engine must initialize");
        let expected = read_expected_stdout(&fab_path)
            .unwrap_or_else(|| panic!("{rel} must have a sibling .expected"));
        match &outcome {
            RunOutcome::Success { stdout, .. } => {
                assert_eq!(
                    normalize_newline(stdout),
                    expected,
                    "{rel} captured stdout must match the sibling .expected"
                );
            }
            other => panic!(
                "{rel} must run to success through the portable runner, got: {other:?}"
            ),
        }
    }
}
