//! Wasm target product ledger: schema lock, checker, and deterministic
//! regeneration (Stage 1 of the Wasm host-parity campaign).
//!
//! The checked-in ledger at `radix/docs/factory/wasm-host-parity/
//! baseline-gap-ledger.toml` is a *measured artifact*, not a hand-maintained
//! allowlist: every canonical corpus `.fab` path joins exactly one row that
//! records its shared Rust oracle class, Wasm treatment, measured current
//! evidence tier, product-contract required tier, outcome, earliest owner,
//! and blocker. This module is the schema lock and executable checker; the
//! ledger owns measured state.
//!
//! Policy rules (locked by the Stage 1 delivery and goal):
//! - Corpus-relative path is the row identity; the source digest detects
//!   staleness.
//! - Rust outcomes stay shared (`super::oracle` is the authority; no copied
//!   classification tables). Wasm treatment stays target-specific.
//! - Only product-contract reasoning may classify a row `contract-reject`,
//!   `deferred`, or `n/a`; implementation difficulty is a gap.
//! - Every below-required-tier shared row records one earliest owner and one
//!   blocker ID (planning-index blocked-unit routing).
//! - The checker rejects missing, duplicate, stale-digest, unknown-enum,
//!   ownerless-gap, policy-without-reason, and obsolete-blocker rows.

use super::common::{collect_exempla_files, corpus_relative_key, make_temp_root};
use super::oracle::{rust_oracle, RustOracleOutcome};
use super::wasm::{classify_wasm_exemplum, wasm_session, WasmE2eResult, WasmTier};
use super::wasm_external::WasmInstantiationBucket;
use radix::driver::Session;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

/// Schema version of the checked-in ledger file.
pub(crate) const LEDGER_SCHEMA_VERSION: u32 = 1;

/// Relative path (from the worktree root) of the checked-in ledger.
const LEDGER_REL_PATH: &str = "radix/docs/factory/wasm-host-parity/baseline-gap-ledger.toml";

// ---------------------------------------------------------------------------
// Schema enums (locked; new product-policy classes stop the phase and revise
// the goal rather than growing this enum silently).
// ---------------------------------------------------------------------------

/// Wasm target treatment for a fixture. Only product-contract reasoning may
/// use `contract-reject`, `deferred`, or `n/a`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum WasmTreatment {
    #[serde(rename = "shared")]
    Shared,
    #[serde(rename = "declaration-only")]
    DeclarationOnly,
    #[serde(rename = "contract-reject")]
    ContractReject,
    #[serde(rename = "deferred")]
    Deferred,
    #[serde(rename = "n/a")]
    Na,
}

/// Highest evidence tier reached by the Wasm probes. Ordered by strength.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) enum EvidenceTier {
    #[serde(rename = "discovered")]
    Discovered,
    #[serde(rename = "frontend")]
    Frontend,
    #[serde(rename = "mir")]
    Mir,
    #[serde(rename = "emitted")]
    Emitted,
    #[serde(rename = "validated")]
    Validated,
    #[serde(rename = "linked")]
    Linked,
    #[serde(rename = "invoked")]
    Invoked,
    #[serde(rename = "outcome-checked")]
    OutcomeChecked,
}

/// Shared Rust oracle class (mirrors `super::oracle::RustOracleOutcome`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum RustClass {
    #[serde(rename = "run_success")]
    RunSuccess,
    #[serde(rename = "declaration_only")]
    DeclarationOnly,
    #[serde(rename = "expected_runtime_failure")]
    ExpectedRuntimeFailure,
    #[serde(rename = "expected_nonzero_exit")]
    ExpectedNonzeroExit,
    #[serde(rename = "expected_compile_failure")]
    ExpectedCompileFailure,
    #[serde(rename = "wrong_lane")]
    WrongLane,
}

/// Row outcome: the measured disposition of the fixture on the Wasm lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum RowOutcome {
    #[serde(rename = "parity")]
    Parity,
    #[serde(rename = "gap")]
    Gap,
    #[serde(rename = "deferred")]
    Deferred,
    #[serde(rename = "contract-reject")]
    ContractReject,
    #[serde(rename = "n/a")]
    Na,
}

/// Earliest owner of a row's disposition (architecture.md owner enum).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum WasmOwner {
    #[serde(rename = "frontend")]
    Frontend,
    #[serde(rename = "shared-mir")]
    SharedMir,
    #[serde(rename = "wasm-encoding")]
    WasmEncoding,
    #[serde(rename = "cpu-abi")]
    CpuAbi,
    #[serde(rename = "wasm-host")]
    WasmHost,
    #[serde(rename = "capability-policy")]
    CapabilityPolicy,
    #[serde(rename = "package")]
    Package,
    #[serde(rename = "product-cli")]
    ProductCli,
}

/// One ledger row. Every field except `reason` is required; `reason` is
/// required exactly when the row carries a product-policy disposition
/// (`contract-reject`, `deferred`, `n/a`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct WasmLedgerRow {
    /// Canonical corpus-relative path (row identity), forward slashes.
    pub(crate) path: String,
    /// `sha256:<hex>` of the `.fab` source bytes (staleness detection).
    pub(crate) digest: String,
    pub(crate) rust_class: RustClass,
    pub(crate) treatment: WasmTreatment,
    pub(crate) current_tier: EvidenceTier,
    pub(crate) required_tier: EvidenceTier,
    pub(crate) outcome: RowOutcome,
    pub(crate) owner: WasmOwner,
    /// Required for `gap`/`deferred` rows; `"none"` otherwise.
    pub(crate) blocker: String,
    /// Policy reason (required for policy rows) or boundary reason for gaps.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub(crate) reason: Option<String>,
}

/// Top-level ledger document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct LedgerFile {
    pub(crate) schema_version: u32,
    #[serde(rename = "row")]
    pub(crate) rows: Vec<WasmLedgerRow>,
}

// ---------------------------------------------------------------------------
// Product-contract policy
// ---------------------------------------------------------------------------

/// Product-contract deferrals (path → (owner, blocker, reason)).
///
/// Only campaign-documented product contracts may add entries here;
/// implementation difficulty is a gap, never a deferral. The initial Stage 1
/// baseline has no deferred fixtures: every below-required-tier row below is
/// an honest gap with an earliest owner.
pub(crate) const DEFERRED_POLICY: &[(&str, WasmOwner, &str, &str)] = &[];

/// Catalog of known blocker IDs. A gap/deferred row whose blocker is not in
/// this catalog is an obsolete-blocker error.
pub(crate) const KNOWN_BLOCKERS: &[&str] = &[
    "none",
    "frontend_analysis_failed",
    "mir_lowering_failed",
    "wasm_emission_unsupported",
    "wasm_validation_failed",
    "runtime_import_unresolved",
    "host_instantiation_blocked",
    "output_mismatch",
];

/// Sentinel blocker for rows that are not below their required tier.
const NO_BLOCKER: &str = "none";

/// Evidence tier the product contract requires for a fixture, derived from
/// the shared Rust oracle class. Every executable class requires the full
/// outcome to be verified (sibling `.expected` match or accepted failure),
/// so its required tier is `outcome-checked`. Contract-reject and wrong-lane
/// fixtures are fully disposed by the shared frontend, so their only
/// obligation is to be discovered/readable and to fail identically.
fn required_tier_for(rust_class: RustClass) -> EvidenceTier {
    match rust_class {
        RustClass::RunSuccess
        | RustClass::ExpectedRuntimeFailure
        | RustClass::ExpectedNonzeroExit => EvidenceTier::OutcomeChecked,
        RustClass::DeclarationOnly => EvidenceTier::Validated,
        RustClass::ExpectedCompileFailure | RustClass::WrongLane => EvidenceTier::Discovered,
    }
}

/// Wasm treatment derived from the shared oracle class plus the explicit
/// deferral table. The oracle class is the only source of `contract-reject` /
/// `n/a`; a fixture may only become `deferred` through [`DEFERRED_POLICY`].
fn treatment_for(path: &str, rust_class: RustClass) -> WasmTreatment {
    if DEFERRED_POLICY
        .iter()
        .any(|(deferred_path, ..)| *deferred_path == path)
    {
        return WasmTreatment::Deferred;
    }
    match rust_class {
        RustClass::ExpectedCompileFailure => WasmTreatment::ContractReject,
        RustClass::WrongLane => WasmTreatment::Na,
        RustClass::DeclarationOnly => WasmTreatment::DeclarationOnly,
        RustClass::RunSuccess
        | RustClass::ExpectedRuntimeFailure
        | RustClass::ExpectedNonzeroExit => WasmTreatment::Shared,
    }
}

fn outcome_for(
    treatment: WasmTreatment,
    current: EvidenceTier,
    required: EvidenceTier,
) -> RowOutcome {
    match treatment {
        WasmTreatment::Deferred => RowOutcome::Deferred,
        WasmTreatment::ContractReject => RowOutcome::ContractReject,
        WasmTreatment::Na => RowOutcome::Na,
        WasmTreatment::Shared | WasmTreatment::DeclarationOnly => {
            if current >= required {
                RowOutcome::Parity
            } else {
                RowOutcome::Gap
            }
        }
    }
}

/// Earliest owner for a parity row: the owner of the tier the fixture already
/// reaches.
fn owner_of_required_tier(required: EvidenceTier) -> WasmOwner {
    match required {
        EvidenceTier::OutcomeChecked | EvidenceTier::Invoked => WasmOwner::WasmHost,
        EvidenceTier::Validated => WasmOwner::WasmEncoding,
        EvidenceTier::Mir => WasmOwner::SharedMir,
        EvidenceTier::Frontend | EvidenceTier::Discovered => WasmOwner::Frontend,
        EvidenceTier::Emitted | EvidenceTier::Linked => WasmOwner::WasmEncoding,
    }
}

/// Earliest owner for a gap row, by the boundary at which the fixture is
/// blocked (planning-index blocked-unit routing).
fn owner_of_boundary(current: EvidenceTier, result: &WasmE2eResult) -> WasmOwner {
    match current {
        EvidenceTier::Discovered => WasmOwner::Frontend,
        EvidenceTier::Frontend => WasmOwner::SharedMir,
        EvidenceTier::Mir | EvidenceTier::Emitted => WasmOwner::WasmEncoding,
        EvidenceTier::Validated => {
            if result.stubless_bucket == Some(WasmInstantiationBucket::MissingImport) {
                WasmOwner::CpuAbi
            } else {
                WasmOwner::WasmHost
            }
        }
        EvidenceTier::Linked => WasmOwner::CpuAbi,
        EvidenceTier::Invoked | EvidenceTier::OutcomeChecked => WasmOwner::WasmHost,
    }
}

fn owner_for(
    treatment: WasmTreatment,
    current: EvidenceTier,
    required: EvidenceTier,
    result: &WasmE2eResult,
    path: &str,
) -> WasmOwner {
    match treatment {
        WasmTreatment::Deferred => DEFERRED_POLICY
            .iter()
            .find_map(|(deferred_path, owner, ..)| (*deferred_path == path).then_some(*owner))
            .unwrap_or(WasmOwner::WasmHost),
        WasmTreatment::ContractReject | WasmTreatment::Na => WasmOwner::Frontend,
        WasmTreatment::Shared | WasmTreatment::DeclarationOnly => {
            if current >= required {
                owner_of_required_tier(required)
            } else {
                owner_of_boundary(current, result)
            }
        }
    }
}

fn blocker_for(
    treatment: WasmTreatment,
    current: EvidenceTier,
    required: EvidenceTier,
    result: &WasmE2eResult,
    path: &str,
) -> &'static str {
    match treatment {
        WasmTreatment::Deferred => DEFERRED_POLICY
            .iter()
            .find_map(|(deferred_path, _, blocker, ..)| {
                (*deferred_path == path).then_some(*blocker)
            })
            .unwrap_or("deferred_pending"),
        WasmTreatment::ContractReject | WasmTreatment::Na => NO_BLOCKER,
        WasmTreatment::Shared | WasmTreatment::DeclarationOnly => {
            if current >= required {
                NO_BLOCKER
            } else {
                match current {
                    EvidenceTier::Discovered => "frontend_analysis_failed",
                    EvidenceTier::Frontend => "mir_lowering_failed",
                    EvidenceTier::Mir => "wasm_emission_unsupported",
                    EvidenceTier::Emitted => "wasm_validation_failed",
                    EvidenceTier::Validated => {
                        if result.stubless_bucket == Some(WasmInstantiationBucket::MissingImport) {
                            "runtime_import_unresolved"
                        } else {
                            "host_instantiation_blocked"
                        }
                    }
                    EvidenceTier::Linked => "runtime_import_unresolved",
                    EvidenceTier::Invoked => "output_mismatch",
                    EvidenceTier::OutcomeChecked => NO_BLOCKER,
                }
            }
        }
    }
}

fn boundary_reason(current: EvidenceTier, result: &WasmE2eResult) -> String {
    match current {
        EvidenceTier::Discovered => {
            "frontend analysis rejected the fixture (shared frontend boundary)".to_owned()
        }
        EvidenceTier::Frontend => {
            "MIR lowering rejected the fixture (shared MIR boundary)".to_owned()
        }
        EvidenceTier::Mir => "Wasm emission rejected the live shape".to_owned(),
        EvidenceTier::Emitted => "external wasm validation rejected the module".to_owned(),
        EvidenceTier::Validated => {
            if result.stubless_bucket == Some(WasmInstantiationBucket::MissingImport) {
                "one or more runtime imports remain unresolved at host link".to_owned()
            } else {
                "module validates but host instantiation is blocked".to_owned()
            }
        }
        EvidenceTier::Linked => "host link/invoke lifecycle is not yet provided".to_owned(),
        EvidenceTier::Invoked => {
            "host ran the entry but output did not match the sibling .expected".to_owned()
        }
        EvidenceTier::OutcomeChecked => String::new(),
    }
}

fn reason_for(
    path: &str,
    treatment: WasmTreatment,
    current: EvidenceTier,
    outcome: RowOutcome,
    result: &WasmE2eResult,
) -> Option<String> {
    match treatment {
        WasmTreatment::Deferred => DEFERRED_POLICY
            .iter()
            .find_map(|(deferred_path, _, _, reason)| {
                (*deferred_path == path).then_some((*reason).to_owned())
            }),
        WasmTreatment::ContractReject => {
            let issue = rust_oracle(Path::new(path))
                .expected_compile_issue()
                .unwrap_or("compile_contract");
            Some(format!("language contract rejects this program: {issue}"))
        }
        WasmTreatment::Na => Some(
            "fixture belongs to a lane outside the MIR-backed wasm-host subset (wrong-lane contract)"
                .to_owned(),
        ),
        WasmTreatment::Shared | WasmTreatment::DeclarationOnly => match outcome {
            RowOutcome::Gap => Some(boundary_reason(current, result)),
            RowOutcome::Parity => None,
            _ => None,
        },
    }
}

// ---------------------------------------------------------------------------
// Measurement and regeneration
// ---------------------------------------------------------------------------

/// Map the existing Wasm harness tier onto the ledger's evidence tier.
fn evidence_tier_from_wasm_tier(tier: WasmTier) -> EvidenceTier {
    match tier {
        WasmTier::SourceReadable => EvidenceTier::Discovered,
        WasmTier::FrontendAnalyzed => EvidenceTier::Frontend,
        WasmTier::MirLowered => EvidenceTier::Mir,
        WasmTier::WasmEmitted => EvidenceTier::Emitted,
        WasmTier::CompileValid => EvidenceTier::Validated,
        WasmTier::Runnable => EvidenceTier::Invoked,
        WasmTier::OutputChecked => EvidenceTier::OutcomeChecked,
    }
}

fn rust_class_for(path: &Path) -> RustClass {
    match rust_oracle(path) {
        RustOracleOutcome::RunSuccess { .. } => RustClass::RunSuccess,
        RustOracleOutcome::DeclarationOnly { .. } => RustClass::DeclarationOnly,
        RustOracleOutcome::ExpectedRuntimeFailure { .. } => RustClass::ExpectedRuntimeFailure,
        RustOracleOutcome::ExpectedNonzeroExit { .. } => RustClass::ExpectedNonzeroExit,
        RustOracleOutcome::ExpectedCompileFailure { .. } => RustClass::ExpectedCompileFailure,
        RustOracleOutcome::ExplicitWrongLane { .. } => RustClass::WrongLane,
    }
}

/// Build one ledger row from a live classification result.
fn row_from_measurement(result: &WasmE2eResult) -> WasmLedgerRow {
    let path = corpus_relative_key(&result.path);
    let digest = file_digest(&result.path);
    let rust_class = rust_class_for(Path::new(&path));
    let treatment = treatment_for(&path, rust_class);
    let current = evidence_tier_from_wasm_tier(result.tier);
    let required = required_tier_for(rust_class);
    let outcome = outcome_for(treatment, current, required);
    let owner = owner_for(treatment, current, required, result, &path);
    let blocker = blocker_for(treatment, current, required, result, &path);
    let reason = reason_for(&path, treatment, current, outcome, result);
    WasmLedgerRow {
        path,
        digest,
        rust_class,
        treatment,
        current_tier: current,
        required_tier: required,
        outcome,
        owner,
        blocker: blocker.to_owned(),
        reason,
    }
}

/// Run the existing frontend/MIR/Wasm probes over every canonical corpus file
/// and build the complete row set (deterministic: rows sorted by path).
pub(crate) fn measure_current_rows() -> Vec<WasmLedgerRow> {
    let corpus = crate::paths::corpus_dir();
    let files = collect_exempla_files(&corpus);
    let session: Session = wasm_session();
    let temp_root = make_temp_root();
    let mut rows = Vec::with_capacity(files.len());
    for (idx, file) in files.iter().enumerate() {
        let result = classify_wasm_exemplum(&session, file, idx, &temp_root);
        rows.push(row_from_measurement(&result));
    }
    rows.sort_by(|left, right| left.path.cmp(&right.path));
    rows
}

/// Absolute path to the checked-in ledger.
pub(crate) fn ledger_path() -> PathBuf {
    crate::paths::faberlang_home()
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.."))
        .join(LEDGER_REL_PATH)
}

/// Deterministic TOML text for the complete ledger.
pub(crate) fn generate_ledger_text(rows: &[WasmLedgerRow]) -> Result<String, String> {
    let file = LedgerFile {
        schema_version: LEDGER_SCHEMA_VERSION,
        rows: rows.to_vec(),
    };
    let body =
        toml::to_string_pretty(&file).map_err(|err| format!("cannot serialize ledger: {err}"))?;
    Ok(format!(
        "# Wasm host-parity target ledger — Stage 1 baseline (measured artifact).\n\
         # Generated deterministically by the exempla wasm-ledger checker\n\
         # (crates/exempla/src/exempla_e2e/wasm_ledger.rs). Measured gap evidence,\n\
         # not an accepted-failure allowlist. Corpus-relative path is the row\n\
         # identity; `digest` is sha256 of the `.fab` source bytes. Regenerate with\n\
         # `cargo test -p exempla --test e2e_harness wasm_ledger_regenerate -- --ignored`.\n\
         #\n\
         # Outcome classes: parity | gap | deferred | contract-reject | n/a. Only\n\
         # product-contract reasons may use deferred/contract-reject/n-a.\n\
         # Regenerating this file and re-checking must produce no diff.\n\
         {body}"
    ))
}

/// Parse ledger text (comments stripped) into the typed document.
pub(crate) fn parse_ledger_text(text: &str) -> Result<LedgerFile, String> {
    let body: String = text
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    toml::from_str(&body).map_err(|err| format!("ledger parse error: {err}"))
}

// ---------------------------------------------------------------------------
// Checker
// ---------------------------------------------------------------------------

/// Snapshot of the live canonical corpus used by the checker.
#[derive(Debug, Clone)]
struct CorpusSnapshot {
    /// Discovered `.fab` paths (corpus-relative, sorted).
    files: BTreeSet<String>,
    /// Source digest per corpus-relative path.
    digests: BTreeMap<String, String>,
    /// `[[files]]` paths from the generated corpus index (`index.toml`).
    index_files: BTreeSet<String>,
}

fn file_digest(path: &Path) -> String {
    let bytes = fs::read(path).unwrap_or_default();
    let hash = Sha256::digest(&bytes);
    format!("sha256:{hash:x}")
}

fn corpus_snapshot() -> CorpusSnapshot {
    let corpus = crate::paths::corpus_dir();
    let mut files = BTreeSet::new();
    let mut digests = BTreeMap::new();
    for file in collect_exempla_files(&corpus) {
        let key = corpus_relative_key(&file);
        digests.insert(key.clone(), file_digest(&file));
        files.insert(key);
    }
    let index_files = read_index_files(&corpus);
    CorpusSnapshot {
        files,
        digests,
        index_files,
    }
}

fn read_index_files(corpus: &Path) -> BTreeSet<String> {
    let index_path = corpus.join("index.toml");
    let Ok(text) = fs::read_to_string(&index_path) else {
        return BTreeSet::new();
    };
    // NOTE: parse through Deserialize (`toml::from_str`); `toml::Value`'s own
    // `FromStr` parses a bare value expression, not a document.
    let Ok(value) = toml::from_str::<toml::Value>(&text) else {
        return BTreeSet::new();
    };
    let mut paths = BTreeSet::new();
    if let Some(files) = value.get("files").and_then(toml::Value::as_array) {
        for entry in files {
            if let Some(path) = entry.get("path").and_then(toml::Value::as_str) {
                paths.insert(path.to_owned());
            }
        }
    }
    paths
}

fn valid_path(path: &str) -> bool {
    path.ends_with(".fab") && !path.starts_with('/') && !path.contains("..") && !path.contains('\\')
}

/// Validate a parsed ledger against the live corpus. Returns every violation;
/// an empty list means the ledger is clean.
pub(crate) fn check_ledger_rows(rows: &[WasmLedgerRow]) -> Vec<String> {
    let snapshot = corpus_snapshot();
    let mut errors = Vec::new();

    // Join: every canonical corpus path joins exactly one row, and every row
    // maps to a discovered corpus path.
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    for row in rows {
        *seen.entry(row.path.as_str()).or_default() += 1;
    }
    for path in &snapshot.files {
        if seen.get(path.as_str()).copied().unwrap_or(0) == 0 {
            errors.push(format!("missing row for corpus path `{path}`"));
        } else if seen[path.as_str()] > 1 {
            errors.push(format!("duplicate rows for corpus path `{path}`"));
        }
    }
    for (path, count) in &seen {
        if !snapshot.files.contains(*path) {
            errors.push(format!("orphan row for unknown path `{path}`"));
        } else if *count > 1 {
            errors.push(format!("duplicate rows for corpus path `{path}`"));
        }
    }
    if snapshot.index_files.is_empty() {
        errors.push(
            "corpus index.toml has no [[files]] entries (denominator unverifiable)".to_owned(),
        );
    } else {
        for path in snapshot.index_files.difference(&snapshot.files) {
            errors.push(format!(
                "corpus index lists `{path}` but no such .fab was discovered"
            ));
        }
        for path in snapshot.files.difference(&snapshot.index_files) {
            errors.push(format!(
                "discovered `.fab` `{path}` missing from corpus index"
            ));
        }
    }

    for row in rows {
        if !valid_path(&row.path) {
            errors.push(format!(
                "row `{}` has an invalid corpus-relative path",
                row.path
            ));
        }
        let rust_class = rust_class_for(Path::new(&row.path));
        let treatment = treatment_for(&row.path, rust_class);
        let required = required_tier_for(rust_class);

        if row.rust_class != rust_class {
            errors.push(format!(
                "row `{}` rust_class `{:?}` != shared oracle `{:?}`",
                row.path, row.rust_class, rust_class
            ));
        }
        if row.treatment != treatment {
            errors.push(format!(
                "row `{}` treatment `{:?}` != policy `{:?}` (rust_class `{:?}`)",
                row.path, row.treatment, treatment, rust_class
            ));
        }
        if row.required_tier != required {
            errors.push(format!(
                "row `{}` required_tier `{:?}` != policy `{:?}`",
                row.path, row.required_tier, required
            ));
        }
        if let Some(expected) = snapshot.digests.get(&row.path) {
            if *expected != row.digest {
                errors.push(format!(
                    "row `{}` digest is stale (file changed; regenerate the ledger)",
                    row.path
                ));
            }
        } else {
            errors.push(format!("row `{}` has no corpus file to digest", row.path));
        }

        // Outcome must match the tier comparison (shared/declaration-only rows)
        // or the policy class (policy rows).
        let expected_outcome = outcome_for(row.treatment, row.current_tier, row.required_tier);
        if row.outcome != expected_outcome {
            errors.push(format!(
                "row `{}` outcome `{:?}` != derived `{:?}` (current {:?}, required {:?})",
                row.path, row.outcome, expected_outcome, row.current_tier, row.required_tier
            ));
        }

        // Blockers: required and known for gap/deferred; "none" otherwise.
        match row.outcome {
            RowOutcome::Gap | RowOutcome::Deferred => {
                if row.blocker.is_empty() || row.blocker == NO_BLOCKER {
                    errors.push(format!(
                        "row `{}` is `{:?}` but has no blocker (ownerless-gap)",
                        row.path, row.outcome
                    ));
                } else if !KNOWN_BLOCKERS.contains(&row.blocker.as_str()) {
                    errors.push(format!(
                        "row `{}` has obsolete/unknown blocker `{}`",
                        row.path, row.blocker
                    ));
                }
            }
            RowOutcome::Parity | RowOutcome::ContractReject | RowOutcome::Na => {
                if row.blocker != NO_BLOCKER {
                    errors.push(format!(
                        "row `{}` is `{:?}` but has blocker `{}` (expected none)",
                        row.path, row.outcome, row.blocker
                    ));
                }
            }
        }

        // Policy rows must carry a policy reason; gap rows carry a boundary
        // reason.
        match row.outcome {
            RowOutcome::ContractReject | RowOutcome::Deferred | RowOutcome::Na => {
                if row.reason.as_deref().unwrap_or("").is_empty() {
                    errors.push(format!(
                        "row `{}` is `{:?}` without a policy reason",
                        row.path, row.outcome
                    ));
                }
            }
            RowOutcome::Gap => {
                if row.reason.as_deref().unwrap_or("").is_empty() {
                    errors.push(format!(
                        "row `{}` is a gap without a boundary reason",
                        row.path
                    ));
                }
            }
            RowOutcome::Parity => {}
        }
    }

    errors
}

/// Full check of the committed ledger file against the live corpus.
pub(crate) fn check_committed_ledger() -> Result<CheckSummary, String> {
    let path = ledger_path();
    let text = fs::read_to_string(&path)
        .map_err(|err| format!("cannot read ledger {}: {err}", path.display()))?;
    let file = parse_ledger_text(&text)?;
    if file.schema_version != LEDGER_SCHEMA_VERSION {
        return Err(format!(
            "ledger schema_version {} != expected {}",
            file.schema_version, LEDGER_SCHEMA_VERSION
        ));
    }
    let errors = check_ledger_rows(&file.rows);
    if !errors.is_empty() {
        return Err(format!(
            "ledger check failed ({} violations):\n{}",
            errors.len(),
            errors.join("\n")
        ));
    }
    Ok(summarize(&file.rows))
}

/// Regeneration must be idempotent: measure the live corpus and require the
/// result to match the committed ledger byte for byte.
pub(crate) fn verify_committed_is_fresh() -> Result<CheckSummary, String> {
    let path = ledger_path();
    let committed = fs::read_to_string(&path)
        .map_err(|err| format!("cannot read ledger {}: {err}", path.display()))?;
    let rows = measure_current_rows();
    let regenerated = generate_ledger_text(&rows)?;
    if committed != regenerated {
        let committed_lines: Vec<&str> = committed.lines().collect();
        let regenerated_lines: Vec<&str> = regenerated.lines().collect();
        let diffs = committed_lines
            .iter()
            .zip(regenerated_lines.iter())
            .enumerate()
            .filter(|(_, (a, b))| a != b)
            .take(10)
            .map(|(idx, (a, b))| format!("line {}: committed `{}` vs fresh `{}`", idx + 1, a, b))
            .collect::<Vec<_>>();
        return Err(format!(
            "ledger is stale vs live measurement ({} rows); first diffs:\n{}",
            rows.len(),
            diffs.join("\n")
        ));
    }
    Ok(summarize(&rows))
}

/// Regenerate the ledger file from a fresh live measurement.
pub(crate) fn regenerate_ledger_file() -> Result<CheckSummary, String> {
    let rows = measure_current_rows();
    let text = generate_ledger_text(&rows)?;
    let path = ledger_path();
    fs::write(&path, text).map_err(|err| format!("cannot write ledger: {err}"))?;
    Ok(summarize(&rows))
}

/// Outcome counts for receipts and reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CheckSummary {
    pub(crate) rows: usize,
    pub(crate) parity: usize,
    pub(crate) gap: usize,
    pub(crate) deferred: usize,
    pub(crate) contract_reject: usize,
    pub(crate) na: usize,
}

fn summarize(rows: &[WasmLedgerRow]) -> CheckSummary {
    let mut summary = CheckSummary {
        rows: rows.len(),
        parity: 0,
        gap: 0,
        deferred: 0,
        contract_reject: 0,
        na: 0,
    };
    for row in rows {
        match row.outcome {
            RowOutcome::Parity => summary.parity += 1,
            RowOutcome::Gap => summary.gap += 1,
            RowOutcome::Deferred => summary.deferred += 1,
            RowOutcome::ContractReject => summary.contract_reject += 1,
            RowOutcome::Na => summary.na += 1,
        }
    }
    summary
}

#[cfg(test)]
#[path = "wasm_ledger_test.rs"]
mod tests;
