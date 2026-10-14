//! THU-U-06 — deny promotion smoke for WARN016/WARN017.
//!
//! The WARN016/WARN017 catalog codes (U-03) now fire at finalize (U-05). This
//! unit proves the **pre-existing** deny machinery promotes them: deny-all
//! fails a WARN016/017 source, per-code deny promotes only the named code, and
//! `is_known_diagnostic_code` still rejects unknown codes. No product code is
//! touched — this unit proves the driver seam, it does not fix it.
//!
//! Closeout filter: `cargo test -p radix --lib type_hole_union_deny`.

use super::compile;
use crate::codegen::Target;
use crate::diagnostics::is_known_diagnostic_code;
use crate::driver::{AnalyzeRequest, Config, ModuleCompileRequest, Session, WarnPolicy, analyze_source};
use crate::{CompileResult, Diagnostic};

/// Emits both shape warnings: `_` solved to a multi-member union (WARN016) and
/// `∪` solved to a single type (WARN017).
const WARN016_WARN017_SOURCE: &str = r"
typus Valor = numerus ∪ textus
incipit {
    fixum _ v ← 1 ∷ Valor
    fixum ∪ only ← 1
    nota v
    nota only
}
";

fn compile_with_policy(policy: WarnPolicy) -> CompileResult {
    let config = Config::default()
        .with_target(Target::HirRust)
        .with_warn_policy(policy)
        .with_latin_locale();
    let session = Session::new(config);
    compile(&session, "test.fab", WARN016_WARN017_SOURCE, ModuleCompileRequest::default())
}

fn has_code_promoted(result: &CompileResult, code: &str) -> bool {
    result
        .diagnostics
        .iter()
        .any(|diag| diag.code == Some(code) && diag.is_error())
}

fn has_code_as_warning(result: &CompileResult, code: &str) -> bool {
    result
        .diagnostics
        .iter()
        .any(|diag| diag.code == Some(code) && !diag.is_error())
}

#[test]
fn baseline_source_emits_both_hole_warnings() {
    // Sanity on the analysis path (stops before codegen): the fixture really
    // carries WARN016 + WARN017 as warnings, which the deny tests promote.
    let session = Session::new(
        Config::default()
            .with_target(Target::HirRust)
            .with_latin_locale(),
    );
    let diagnostics = match analyze_source(&session, "test.fab", WARN016_WARN017_SOURCE, AnalyzeRequest::default()) {
        Ok(unit) => unit.diagnostics,
        Err(diags) => diags,
    };
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == Some("WARN016") && !d.is_error()),
        "fixture must emit WARN016 as a warning: {diagnostics:?}"
    );
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == Some("WARN017") && !d.is_error()),
        "fixture must emit WARN017 as a warning: {diagnostics:?}"
    );
    assert!(
        !diagnostics.iter().any(Diagnostic::is_error),
        "fixture must not hard-error: {diagnostics:?}"
    );
}

#[test]
fn deny_all_warnings_fails_warn016_warn017_source() {
    let result = compile_with_policy(WarnPolicy { deny_all_warnings: true, deny_codes: vec![] });
    assert!(
        !result.success(),
        "deny-all-warnings must fail the build: {:?}",
        result.diagnostics
    );
    assert!(
        has_code_promoted(&result, "WARN016"),
        "WARN016 must be promoted to error under deny-all: {:?}",
        result.diagnostics
    );
    assert!(
        has_code_promoted(&result, "WARN017"),
        "WARN017 must be promoted to error under deny-all: {:?}",
        result.diagnostics
    );
}

#[test]
fn deny_warn016_promotes_only_warn016() {
    let result = compile_with_policy(WarnPolicy { deny_all_warnings: false, deny_codes: vec!["WARN016".to_string()] });
    assert!(
        !result.success(),
        "denied WARN016 must fail the build: {:?}",
        result.diagnostics
    );
    assert!(
        has_code_promoted(&result, "WARN016"),
        "WARN016 must be promoted: {:?}",
        result.diagnostics
    );
    assert!(
        has_code_as_warning(&result, "WARN017"),
        "WARN017 must stay a warning when only WARN016 is denied: {:?}",
        result.diagnostics
    );
}

#[test]
fn deny_warn017_promotes_only_warn017() {
    let result = compile_with_policy(WarnPolicy { deny_all_warnings: false, deny_codes: vec!["WARN017".to_string()] });
    assert!(
        !result.success(),
        "denied WARN017 must fail the build: {:?}",
        result.diagnostics
    );
    assert!(
        has_code_promoted(&result, "WARN017"),
        "WARN017 must be promoted: {:?}",
        result.diagnostics
    );
    assert!(
        has_code_as_warning(&result, "WARN016"),
        "WARN016 must stay a warning when only WARN017 is denied: {:?}",
        result.diagnostics
    );
}

#[test]
fn is_known_diagnostic_code_still_rejects_unknown_codes() {
    assert!(is_known_diagnostic_code("WARN016"), "WARN016 is a catalog code");
    assert!(is_known_diagnostic_code("WARN017"), "WARN017 is a catalog code");
    assert!(!is_known_diagnostic_code("WARN099"), "unknown code must be rejected");
    assert!(!is_known_diagnostic_code("NOTACODE"), "garbage code must be rejected");
}
