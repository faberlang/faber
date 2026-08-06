//! Tests for the Wasm target ledger schema lock and checker (Stage 1).
//!
//! Covers round-trip, deterministic regeneration, the committed-ledger check
//! against the live corpus, and rejection of stale/duplicate/missing/orphan/
//! ownerless/policy-without-reason/obsolete-blocker/inconsistent rows.

use super::{
    check_committed_ledger, check_ledger_rows, generate_ledger_text, ledger_path,
    parse_ledger_text, verify_committed_is_fresh, LedgerFile, RowOutcome, RustClass, WasmLedgerRow,
    WasmTreatment, LEDGER_SCHEMA_VERSION,
};
use std::fs;

fn committed_file() -> LedgerFile {
    let text =
        fs::read_to_string(ledger_path()).expect("committed ledger must exist for these tests");
    parse_ledger_text(&text).expect("committed ledger must parse")
}

fn committed_rows() -> Vec<WasmLedgerRow> {
    committed_file().rows
}

// ---------------------------------------------------------------------------
// Schema lock: round-trip and determinism
// ---------------------------------------------------------------------------

#[test]
fn ledger_round_trip_preserves_rows() {
    let rows = committed_rows();
    let text = generate_ledger_text(&rows).expect("serialize");
    let file = parse_ledger_text(&text).expect("re-parse");
    assert_eq!(file.schema_version, LEDGER_SCHEMA_VERSION);
    assert_eq!(
        file.rows, rows,
        "round-trip must preserve every row exactly"
    );
}

#[test]
fn ledger_generation_is_deterministic() {
    let rows = committed_rows();
    let first = generate_ledger_text(&rows).expect("first serialize");
    let second = generate_ledger_text(&rows).expect("second serialize");
    assert_eq!(
        first, second,
        "regeneration of identical rows must be byte-identical"
    );
}

#[test]
fn committed_schema_version_is_current() {
    let file = committed_file();
    assert_eq!(
        file.schema_version, LEDGER_SCHEMA_VERSION,
        "committed ledger schema must be current"
    );
}

#[test]
fn committed_rows_are_sorted_by_path() {
    let rows = committed_rows();
    let mut sorted = rows.clone();
    sorted.sort_by(|left, right| left.path.cmp(&right.path));
    assert_eq!(
        rows, sorted,
        "ledger rows must be sorted by corpus-relative path"
    );
}

// ---------------------------------------------------------------------------
// Live check (closeout): structure + join + digests + policy, and full
// re-measurement freshness.
// ---------------------------------------------------------------------------

#[test]
fn committed_ledger_passes_check() {
    let summary = check_committed_ledger().expect("committed ledger must pass the checker");
    assert_eq!(summary.rows, committed_rows().len());
}

#[test]
fn committed_ledger_matches_live_measurement() {
    // Full closeout: re-run the frontend/MIR/Wasm probes and require the fresh
    // regeneration to be byte-identical to the committed ledger.
    let summary =
        verify_committed_is_fresh().expect("committed ledger must match live measurement");
    assert_eq!(summary.rows, committed_rows().len());
}

// ---------------------------------------------------------------------------
// Ratchet: stale / duplicate / missing / orphan / ownerless /
// policy-without-reason / obsolete-blocker / inconsistent rows rejected.
// ---------------------------------------------------------------------------

#[test]
fn stale_digest_row_rejected() {
    let mut rows = committed_rows();
    rows[0].digest = format!("sha256:{}", "0".repeat(64));
    let errors = check_ledger_rows(&rows);
    assert!(
        errors.iter().any(|error| error.contains("digest is stale")),
        "expected a stale-digest error, got: {errors:?}"
    );
}

#[test]
fn duplicate_row_rejected() {
    let mut rows = committed_rows();
    rows.push(rows[0].clone());
    let errors = check_ledger_rows(&rows);
    assert!(
        errors.iter().any(|error| error.contains("duplicate rows")),
        "expected a duplicate-row error, got: {errors:?}"
    );
}

#[test]
fn missing_row_rejected() {
    let mut rows = committed_rows();
    rows.remove(0);
    let errors = check_ledger_rows(&rows);
    assert!(
        errors.iter().any(|error| error.contains("missing row")),
        "expected a missing-row error, got: {errors:?}"
    );
}

#[test]
fn orphan_row_rejected() {
    let mut rows = committed_rows();
    rows.push(WasmLedgerRow {
        path: "no/such/fixture.fab".to_owned(),
        digest: format!("sha256:{}", "0".repeat(64)),
        rust_class: RustClass::RunSuccess,
        treatment: WasmTreatment::Shared,
        current_tier: super::EvidenceTier::Discovered,
        required_tier: super::EvidenceTier::OutcomeChecked,
        outcome: RowOutcome::Gap,
        owner: super::WasmOwner::WasmHost,
        blocker: "host_instantiation_blocked".to_owned(),
        reason: Some("orphan probe".to_owned()),
    });
    let errors = check_ledger_rows(&rows);
    assert!(
        errors.iter().any(|error| error.contains("orphan row")),
        "expected an orphan-row error, got: {errors:?}"
    );
}

#[test]
fn ownerless_row_rejected() {
    // Removing the owner field makes the ledger text fail to parse.
    let text =
        fs::read_to_string(ledger_path()).expect("committed ledger must exist for these tests");
    let line = text
        .lines()
        .find(|line| line.trim_start().starts_with("owner = "))
        .expect("owner line");
    let mutated = text.replace(line, "# owner removed for test");
    let error = parse_ledger_text(&mutated).expect_err("missing owner must fail to parse");
    assert!(
        error.contains("owner"),
        "expected a missing-owner parse error, got: {error}"
    );
}

#[test]
fn gap_without_blocker_rejected() {
    let mut rows = committed_rows();
    let idx = rows
        .iter()
        .position(|row| row.outcome == RowOutcome::Gap)
        .expect("baseline must contain at least one gap row");
    rows[idx].blocker = "none".to_owned();
    let errors = check_ledger_rows(&rows);
    assert!(
        errors.iter().any(|error| error.contains("has no blocker")),
        "expected an ownerless-gap error, got: {errors:?}"
    );
}

#[test]
fn obsolete_blocker_rejected() {
    let mut rows = committed_rows();
    let idx = rows
        .iter()
        .position(|row| row.outcome == RowOutcome::Gap)
        .expect("baseline must contain at least one gap row");
    rows[idx].blocker = "bogus_blocker".to_owned();
    let errors = check_ledger_rows(&rows);
    assert!(
        errors
            .iter()
            .any(|error| error.contains("obsolete/unknown blocker")),
        "expected an obsolete-blocker error, got: {errors:?}"
    );
}

#[test]
fn parity_row_with_blocker_rejected() {
    let mut rows = committed_rows();
    let idx = rows
        .iter()
        .position(|row| row.outcome == RowOutcome::Parity)
        .expect("baseline must contain at least one parity row");
    rows[idx].blocker = "output_mismatch".to_owned();
    let errors = check_ledger_rows(&rows);
    assert!(
        errors.iter().any(|error| error.contains("expected none")),
        "expected a blocker-on-parity error, got: {errors:?}"
    );
}

#[test]
fn policy_without_reason_rejected() {
    let mut rows = committed_rows();
    let idx = rows
        .iter()
        .position(|row| row.outcome == RowOutcome::ContractReject)
        .expect("baseline must contain at least one contract-reject row");
    rows[idx].reason = None;
    let errors = check_ledger_rows(&rows);
    assert!(
        errors
            .iter()
            .any(|error| error.contains("without a policy reason")),
        "expected a policy-without-reason error, got: {errors:?}"
    );
}

#[test]
fn inconsistent_outcome_rejected() {
    let mut rows = committed_rows();
    let idx = rows
        .iter()
        .position(|row| row.outcome == RowOutcome::Parity)
        .expect("baseline must contain at least one parity row");
    rows[idx].outcome = RowOutcome::Gap;
    let errors = check_ledger_rows(&rows);
    assert!(
        errors
            .iter()
            .any(|error| error.contains("outcome") && error.contains("derived")),
        "expected an outcome/tier inconsistency error, got: {errors:?}"
    );
}

#[test]
fn inconsistent_rust_class_rejected() {
    let mut rows = committed_rows();
    let idx = rows
        .iter()
        .position(|row| row.outcome == RowOutcome::ContractReject)
        .expect("baseline must contain at least one contract-reject row");
    rows[idx].rust_class = RustClass::RunSuccess;
    let errors = check_ledger_rows(&rows);
    assert!(
        errors
            .iter()
            .any(|error| error.contains("rust_class") || error.contains("treatment")),
        "expected a rust_class/treatment inconsistency error, got: {errors:?}"
    );
}

#[test]
fn unknown_enum_rejected() {
    let text =
        fs::read_to_string(ledger_path()).expect("committed ledger must exist for these tests");
    let mutated = text.replacen("treatment = \"shared\"", "treatment = \"bogus\"", 1);
    let error = parse_ledger_text(&mutated).expect_err("unknown treatment enum must fail to parse");
    assert!(
        error.contains("treatment"),
        "expected an unknown-enum parse error, got: {error}"
    );
}

// ---------------------------------------------------------------------------
// Regeneration entrypoint (run explicitly with --ignored)
// ---------------------------------------------------------------------------

#[test]
#[ignore = "regenerate the checked-in ledger from live measurement; run: cargo test -p exempla --test e2e_harness wasm_ledger_regenerate -- --ignored --nocapture"]
fn wasm_ledger_regenerate() {
    let summary = super::regenerate_ledger_file().expect("regenerate ledger from live corpus");
    println!("regenerated ledger: {summary:?}");
}
