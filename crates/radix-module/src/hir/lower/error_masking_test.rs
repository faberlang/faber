//! Error-masking repro tests: the masked-subset proof, flipped to the
//! complete-set contract.
//!
//! Two independent, co-occurring errors in one module mask each other: the
//! checker reports only the subset collected before the pipeline stops, so
//! users get a serial fix-up loop and defect repros false-pass. These tests
//! assert the COMPLETE expected diagnostic set for the goal's Repro A and
//! Repro B modules.
//!
//! em-u1 landed these tests RED as the mask proof. em-u2 (the pre-lowering
//! gate classification) flipped **Repro B** green: resolve-phase use-site
//! SEM001s no longer stop the pipeline before lowering, so the
//! lowering-phase SEM008s fold in and both error sets co-report. **Repro A**
//! flips green in em-u3: the analysis-entry continuation keeps the recovered
//! program's successfully-parsed items for semantic analysis, so a module
//! with a parse error co-reports its semantic diagnostics in one pass.
//!
//! Mask surfaces pinned by the em-u1 probe (see the audit note under
//! `docs/factory/error-masking/`):
//!
//! - **Repro A** (PARSE030 masks SEM001): module-drop at the analysis entry.
//!   Every production parse exit — the faber package loader
//!   (`faber/src/package/source_files.rs:179` `!parse.success()` → `None`),
//!   the radix driver (`crates/radix/src/driver/mod.rs` `parse_frontend_source`),
//!   and the check surface (`crates/radix/src/tool/commands/check.rs`) — stopped
//!   before semantic analysis on any parse error. The parser's recovered AST
//!   was never analyzed, so the module's SEM001 was never produced. em-u3's
//!   recovered-parse continuation analyzes the recovered program's
//!   successfully-parsed top-level items (items whose span contains a
//!   parse-error span are cascade-corrupted and dropped), so PARSE030 and
//!   SEM001 co-report. The faber loader branch stays a known residual.
//! - **Repro B** (SEM001 masks SEM008): the resolve pass emits SEM001
//!   (`unknown_variant`, `UndefinedVariable`) for the imported-union match
//!   arms before lowering, and the `semantic/mod.rs:389` gate ("stop before
//!   converting syntax") returns early on it. Lowering therefore never runs,
//!   and the lowering-phase SEM008 (`elige_case_value_shape` for the switch
//!   case values) is never collected. em-u2's gate classification lets
//!   use-site-only resolve errors proceed to lowering, so the SEM008s fold in
//!   before the complete-set gate stops the pipeline. The goal's control
//!   holds: removing the SEM001 construct makes SEM008 appear.

use crate::diagnostics::Diagnostic;
use std::path::{Path, PathBuf};

/// The en reader pack (the repro modules are en-locale sources: `fn`,
/// `string`, `union`, `match`, `case`, `switch`, `class`, `enum`, `variant`,
/// `∷`, `return` are all en-keywords).
fn en_locale_pack() -> crate::locale::LocalePack {
    crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
        .expect("installed en pack should load")
}

/// lib.fab for Repro A — the union the consumer imports.
const REPRO_A_LIB: &str = r"
@ public
union Kind {
    Alpha,
    Gamma { int n }
}
@ public
fn faci_ok() → Kind {
    return variant Gamma { n = 3 } ∷ Kind
}
";

/// Repro A consumer — a parse error masks a semantic error in the same module.
///
///  - independent error 1: a leading cast in expression position (`return ∷ …`)
///    fails to parse -> PARSE030 ×2 (two bad statements in `make_beta`'s body;
///    the `∷` cast operator needs an operand in every grammar state);
///  - independent error 2: match arm over the imported union -> SEM001
///    (`unknown_variant`).
///
/// Complete report must contain PARSE030 AND SEM001. Pre-fix the module was
/// dropped at the analysis entry on the first parse error; the SEM001 in
/// `describe` was masked. (The em-u1 audit's original construct — `variant
/// u.Beta {} ∷ u.Kind` — parsed with PARSE030 at audit time, but the uvf-u3
/// parser work made qualified variant construction a legal parse; the
/// leading-cast construct above is stable under both parser states.)
const REPRO_A_CONSUMER: &str = r#"
import from "./lib" * as u

# Independent error 1: leading cast in expression position — PARSE030 ×2.
fn make_beta() → u.Kind {
    return ∷ u.Kind
    return ∷ u.Kind
}

# Independent error 2: match arm over the imported union — SEM001.
fn describe(u.Kind x) → string {
    match x {
        case u.Alpha { return "alpha" }
        case _ { return "?" }
    }
}
"#;

/// lib.fab for Repro B — the union + enum + class the consumer imports.
const REPRO_B_LIB: &str = r"
@ public
union Kind {
    Alpha,
    Gamma { int n }
}
@ public
enum Mark {
    Red,
    Blue
}
@ public
class Payload {
    mut Mark mark
}
";

/// Repro B consumer — a semantic error masks another semantic error.
///
///  - independent error 1: match arm over the imported union -> SEM001
///    (`unknown_variant`);
///  - independent error 2: switch with enum-member case values -> SEM008
///    (`elige_case_value_shape`).
///
/// Complete report must contain SEM001 ×2 AND SEM008 ×2: the em-u2 gate
/// classification lets the resolve-phase use-site SEM001s proceed to
/// lowering, so the lowering-phase SEM008s in `tag_of` fold in and co-report.
const REPRO_B_CONSUMER: &str = r#"
import from "./lib" * as u

# Independent error 1: match arm over the imported union — SEM001.
fn describe(u.Kind x) → string {
    match x {
        case u.Alpha { return "alpha" }
        case u.Gamma { return x.n }
        default { return "?" }
    }
}

# Independent error 2: switch with enum-member case values — SEM008.
fn tag_of(u.Payload p) → string {
    switch p.mark {
        case u.Red { return "r" }
        case u.Blue { return "b" }
        default { return "?" }
    }
}
"#;

/// Human-readable summary of a diagnostic: `CODE:PHASE:issue`.
fn describe(diag: &Diagnostic) -> String {
    let issue = diag
        .args
        .iter()
        .find(|arg| arg.name == "issue")
        .map(|arg| arg.value.clone())
        .unwrap_or_default();
    let code = diag.code.unwrap_or("<none>");
    format!("{code}:{}:{}", diag.phase, issue)
}

fn diagnostics_summary(diagnostics: &[Diagnostic]) -> String {
    diagnostics
        .iter()
        .map(describe)
        .collect::<Vec<_>>()
        .join(" | ")
}

/// Assert a diagnostic with the given catalog code AND lowering issue string.
fn assert_diagnostic_with_issue(diagnostics: &[Diagnostic], code: &str, issue: &str) {
    assert!(
        diagnostics.iter().any(|diag| {
            diag.code == Some(code)
                && diag
                    .args
                    .iter()
                    .any(|arg| arg.name == "issue" && arg.value == issue)
        }),
        "expected {code} ({issue}) in diagnostics, got: {}",
        diagnostics_summary(diagnostics)
    );
}

/// Count diagnostics with the given catalog code AND lowering issue string.
fn count_diagnostics_with_issue(diagnostics: &[Diagnostic], code: &str, issue: &str) -> usize {
    diagnostics
        .iter()
        .filter(|diag| {
            diag.code == Some(code)
                && diag
                    .args
                    .iter()
                    .any(|arg| arg.name == "issue" && arg.value == issue)
        })
        .count()
}

/// Run the real user-visible check surface (en pack) over a two-file package
/// written into a fresh temp dir: `lib.fab` (contents `lib`) and a consumer
/// file `consumer.fab` (contents `consumer`). Returns the normalized
/// diagnostics for the consumer.
fn check_two_file_package(lib: &str, consumer: &str, consumer_path: &Path) -> crate::services::CheckDiagnosticsResult {
    std::fs::create_dir_all(consumer_path.parent().expect("consumer in a dir")).expect("create temp package");
    std::fs::write(
        consumer_path
            .parent()
            .expect("consumer in a dir")
            .join("lib.fab"),
        lib,
    )
    .expect("write lib.fab");
    std::fs::write(consumer_path, consumer).expect("write consumer");
    crate::services::check_diagnostics_for_source_with_locale_pack(
        &consumer_path.display().to_string(),
        consumer,
        false,
        Some(&en_locale_pack()),
        None,
    )
}

fn temp_consumer_dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("em_u1_{}_{}", name, std::process::id()))
}

// ---------------------------------------------------------------------------
// Repro A — parse error masks a semantic error (PARSE030 masks SEM001).
// ---------------------------------------------------------------------------

#[test]
fn repro_a_parse_error_masks_semantic_error() {
    // The COMPLETE report on the fixed tree: both error sets co-report through
    // the real check surface. `make_beta`'s leading casts report PARSE030 ×2
    // (parse phase); the recovered program's clean `describe` reports SEM001
    // ×1 (`unknown_variant` for its `case u.Alpha` arm). Exact counts prove no
    // duplicate/cascade SEM001 diagnostics from the recovered-parse
    // continuation.
    //
    // NOTE (pending residual): `describe`'s `u.Kind` param type may also
    // report `SEM002:unknown_qualified_type` on the current tree. The import
    // contract derivation (`tool/commands/compile.rs` →
    // `parse_program_for_namespace_exports`) drops the contract when the
    // *consumer* source has parse errors — a sibling module-drop surface the
    // recovered-parse continuation does not cover. That continuation is filed
    // as a known residual; when it lands, `u.Kind` resolves through its
    // file interface and the SEM002 disappears.
    let dir = temp_consumer_dir("repro_a");
    let consumer_path = dir.join("consumer_a.fab");
    let result = check_two_file_package(REPRO_A_LIB, REPRO_A_CONSUMER, &consumer_path);
    let _ = std::fs::remove_dir_all(&dir);

    assert!(!result.success, "a recovered parse must never check clean");

    let parse_count = count_diagnostics_with_issue(&result.diagnostics, "PARSE030", "expected_expression");
    assert_eq!(
        parse_count,
        2,
        "make_beta's leading casts must report PARSE030 ×2, got: {}",
        diagnostics_summary(&result.diagnostics)
    );

    let sem001_count = count_diagnostics_with_issue(&result.diagnostics, "SEM001", "unknown_variant");
    assert_eq!(
        sem001_count,
        1,
        "describe's match arm must co-report SEM001 ×1 (no cascade), got: {}",
        diagnostics_summary(&result.diagnostics)
    );

    // No duplicate/cascade diagnostics from the continuation: the corrupted
    // `make_beta` contributes no SEM001 of any other issue.
    let total_sem001 = result
        .diagnostics
        .iter()
        .filter(|diag| diag.code == Some("SEM001"))
        .count();
    assert_eq!(
        total_sem001,
        1,
        "no cascade SEM001s from the corrupted item, got: {}",
        diagnostics_summary(&result.diagnostics)
    );
}

#[test]
fn repro_a_control_without_parse_error_reports_sem001() {
    // Goal control: remove the parse-error construct and the masked semantic
    // error appears. This pins the mask (and the expected full set) for
    // Repro A: `describe` alone does report SEM001.
    let dir = temp_consumer_dir("repro_a_control");
    let consumer_path = dir.join("consumer_a.fab");
    let control = "import from \"./lib\" * as u\n\nfn describe(u.Kind x) → string {\n    match x {\n        case u.Alpha { return \"alpha\" }\n        case _ { return \"?\" }\n    }\n}\n";
    let result = check_two_file_package(REPRO_A_LIB, control, &consumer_path);
    let _ = std::fs::remove_dir_all(&dir);

    assert_diagnostic_with_issue(&result.diagnostics, "SEM001", "unknown_variant");
}

#[test]
fn repro_a_control_without_sem001_reports_parse_only() {
    // Goal control / construct isolation (other direction): remove the SEM001
    // construct (`describe`) and exactly the SEM001 drops — the recovered
    // program then contains no clean items, so the report is PARSE030 ×2 only.
    let dir = temp_consumer_dir("repro_a_control_no_sem001");
    let consumer_path = dir.join("consumer_a.fab");
    let control =
        "import from \"./lib\" * as u\n\nfn make_beta() → u.Kind {\n    return ∷ u.Kind\n    return ∷ u.Kind\n}\n";
    let result = check_two_file_package(REPRO_A_LIB, control, &consumer_path);
    let _ = std::fs::remove_dir_all(&dir);

    let parse_count = count_diagnostics_with_issue(&result.diagnostics, "PARSE030", "expected_expression");
    assert_eq!(
        parse_count,
        2,
        "make_beta's broken qualified variant construction must still report PARSE030 ×2, got: {}",
        diagnostics_summary(&result.diagnostics)
    );

    let sem001_count = count_diagnostics_with_issue(&result.diagnostics, "SEM001", "unknown_variant");
    assert_eq!(
        sem001_count,
        0,
        "removing describe must drop exactly the SEM001, got: {}",
        diagnostics_summary(&result.diagnostics)
    );

    // PARSE030 ×2 only: no error diagnostics beyond the parse set. A
    // non-error warning (e.g. WARN002 unused_import — the import is unused
    // once describe is gone) may legitimately co-report; errors may not.
    let error_count = result
        .diagnostics
        .iter()
        .filter(|diag| diag.is_error())
        .count();
    assert_eq!(
        error_count,
        2,
        "removing describe leaves exactly PARSE030 ×2 errors, got: {}",
        diagnostics_summary(&result.diagnostics)
    );
}

// ---------------------------------------------------------------------------
// Repro B — SEM001 (resolve) masks SEM008 (lowering); em-u2's gate
// classification makes both error sets co-report.
// ---------------------------------------------------------------------------

#[test]
fn repro_b_semantic_and_lowering_errors_co_report() {
    // The COMPLETE report on the fixed tree: both error sets co-report through
    // the real check surface. `describe`'s two match arms over the imported
    // union report SEM001 ×2 (resolve phase); `tag_of`'s switch case values
    // over the imported enum report SEM008 ×2 (lowering phase) — lowering runs
    // because the pre-lowering gate only stops on coherence-class errors, and
    // the complete-set gate then stops on the folded set. Exact counts prove
    // no duplicate/cascade diagnostics from the boundary change.
    let dir = temp_consumer_dir("repro_b");
    let consumer_path = dir.join("consumer_b.fab");
    let result = check_two_file_package(REPRO_B_LIB, REPRO_B_CONSUMER, &consumer_path);
    let _ = std::fs::remove_dir_all(&dir);

    let sem001_count = count_diagnostics_with_issue(&result.diagnostics, "SEM001", "unknown_variant");
    assert_eq!(
        sem001_count,
        2,
        "describe's two match arms must report SEM001 ×2, got: {}",
        diagnostics_summary(&result.diagnostics)
    );

    let sem008_count = count_diagnostics_with_issue(&result.diagnostics, "SEM008", "elige_case_value_shape");
    assert_eq!(
        sem008_count,
        2,
        "tag_of's two switch arms must co-report SEM008 ×2 (no cascade), got: {}",
        diagnostics_summary(&result.diagnostics)
    );
}

#[test]
fn repro_b_control_without_sem001_reports_sem008() {
    // Goal control / construct isolation: remove the SEM001 construct
    // (`describe`) and exactly the two SEM001s drop — the two SEM008s remain.
    let dir = temp_consumer_dir("repro_b_control");
    let consumer_path = dir.join("consumer_b.fab");
    let control = "import from \"./lib\" * as u\n\nfn tag_of(u.Payload p) → string {\n    switch p.mark {\n        case u.Red { return \"r\" }\n        case u.Blue { return \"b\" }\n        default { return \"?\" }\n    }\n}\n";
    let result = check_two_file_package(REPRO_B_LIB, control, &consumer_path);
    let _ = std::fs::remove_dir_all(&dir);

    let sem001_count = count_diagnostics_with_issue(&result.diagnostics, "SEM001", "unknown_variant");
    assert_eq!(
        sem001_count,
        0,
        "removing describe must drop exactly the SEM001s, got: {}",
        diagnostics_summary(&result.diagnostics)
    );

    let sem008_count = count_diagnostics_with_issue(&result.diagnostics, "SEM008", "elige_case_value_shape");
    assert_eq!(
        sem008_count,
        2,
        "the two SEM008s must remain, got: {}",
        diagnostics_summary(&result.diagnostics)
    );
}

#[test]
fn repro_b_control_without_sem008_reports_sem001() {
    // Goal control / construct isolation (other direction): remove the SEM008
    // construct (`tag_of`) and exactly the two SEM008s drop — `describe`'s two
    // SEM001s remain.
    let dir = temp_consumer_dir("repro_b_control_no_sem008");
    let consumer_path = dir.join("consumer_b.fab");
    let control = "import from \"./lib\" * as u\n\nfn describe(u.Kind x) → string {\n    match x {\n        case u.Alpha { return \"alpha\" }\n        case u.Gamma { return x.n }\n        default { return \"?\" }\n    }\n}\n";
    let result = check_two_file_package(REPRO_B_LIB, control, &consumer_path);
    let _ = std::fs::remove_dir_all(&dir);

    let sem001_count = count_diagnostics_with_issue(&result.diagnostics, "SEM001", "unknown_variant");
    assert_eq!(
        sem001_count,
        2,
        "describe's two match arms must still report SEM001 ×2, got: {}",
        diagnostics_summary(&result.diagnostics)
    );

    let sem008_count = count_diagnostics_with_issue(&result.diagnostics, "SEM008", "elige_case_value_shape");
    assert_eq!(
        sem008_count,
        0,
        "removing tag_of must drop exactly the SEM008s, got: {}",
        diagnostics_summary(&result.diagnostics)
    );
}

// ---------------------------------------------------------------------------
// em-u4: dedup regression — each independent error reports exactly once
// (no cascade, no double-report) per construct+issue pair. This is the final
// unit's lock on the error-masking goal done-when 4 ("no duplicate or cascade
// diagnostics from error placeholders") at the module-report level.
// ---------------------------------------------------------------------------

/// Assert no two ERROR diagnostics share the same (code, issue, span) triple.
///
/// A repeated triple is the signature of a cascade/double-report from an
/// error placeholder or from the recovered-parse continuation re-analyzing a
/// program item.
fn assert_no_duplicate_error_diagnostics(diagnostics: &[Diagnostic]) {
    let mut seen = std::collections::HashSet::new();
    for diag in diagnostics.iter().filter(|diag| diag.is_error()) {
        let issue = diag
            .args
            .iter()
            .find(|arg| arg.name == "issue")
            .map(|arg| arg.value.clone())
            .unwrap_or_default();
        let key = (diag.code, issue, diag.span.map(|span| (span.start, span.end)));
        assert!(
            seen.insert(key.clone()),
            "duplicate error diagnostic (code, issue, span) = {:?} — cascade/double-report; got: {}",
            key,
            diagnostics_summary(diagnostics)
        );
    }
}

#[test]
fn dedup_each_independent_error_reports_exactly_once() {
    // em-u4 dedup regression over both repro shapes through the real check
    // surface: (a) every independent root cause fires exactly once per
    // construct+issue pair, and (b) no two error diagnostics share a
    // (code, issue, span) triple — error placeholders and the recovered-parse
    // continuation must not cascade duplicate diagnostics.
    let dir = temp_consumer_dir("dedup");
    let a_path = dir.join("consumer_a.fab");
    let b_path = dir.join("consumer_b.fab");
    let a = check_two_file_package(REPRO_A_LIB, REPRO_A_CONSUMER, &a_path);
    let b = check_two_file_package(REPRO_B_LIB, REPRO_B_CONSUMER, &b_path);
    let _ = std::fs::remove_dir_all(&dir);

    assert_no_duplicate_error_diagnostics(&a.diagnostics);
    assert_no_duplicate_error_diagnostics(&b.diagnostics);

    // Repro A — per construct+issue pairs: PARSE030 ×2 (two broken
    // `make_beta` statements) and SEM001 ×1 (one `case u.Alpha` arm).
    assert_eq!(
        count_diagnostics_with_issue(&a.diagnostics, "PARSE030", "expected_expression"),
        2,
        "Repro A PARSE030 must fire once per broken statement, got: {}",
        diagnostics_summary(&a.diagnostics)
    );
    assert_eq!(
        count_diagnostics_with_issue(&a.diagnostics, "SEM001", "unknown_variant"),
        1,
        "Repro A SEM001 must fire exactly once for the match arm, got: {}",
        diagnostics_summary(&a.diagnostics)
    );

    // Repro B — per construct+issue pairs: SEM001 ×2 (two match arms) and
    // SEM008 ×2 (two switch case values).
    assert_eq!(
        count_diagnostics_with_issue(&b.diagnostics, "SEM001", "unknown_variant"),
        2,
        "Repro B SEM001 must fire once per match arm, got: {}",
        diagnostics_summary(&b.diagnostics)
    );
    assert_eq!(
        count_diagnostics_with_issue(&b.diagnostics, "SEM008", "elige_case_value_shape"),
        2,
        "Repro B SEM008 must fire once per switch case value, got: {}",
        diagnostics_summary(&b.diagnostics)
    );
}
