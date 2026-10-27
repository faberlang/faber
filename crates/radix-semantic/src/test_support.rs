//! Shared helpers for `radix-semantic` unit tests.
//!
//! These wrap parse → [`crate::analyze`] so semantic tests do not go through
//! the driver or a backend. Pipeline-shaped proofs (source → codegen) stay in
//! `radix-module` or `mir-emit-harness`.

#![allow(dead_code)]
#![cfg(test)]

use std::collections::BTreeMap;

use radix_diagnostics::DiagnosticArg;
use radix_hir::HirStatementKind;
use radix_lexer::{Span, lex};
use radix_parser::parse;
use radix_target::Target;

use crate::file_interface::FileInterface;
use crate::locale::LocalePack;
use crate::passes::visibility_annotation::VisibilityTier;
use crate::{PassConfig, SemanticError, SemanticResult, TypeId};
use radix_lexer::Interner;

const TEST_NAME: &str = "test.fab";

/// Parse + analyze fixture. Carries the interner that semantic used.
pub struct AnalyzedFixture {
    pub result: SemanticResult,
    pub interner: Interner,
}

impl AnalyzedFixture {
    pub fn success(&self) -> bool {
        self.result.success()
    }

    pub fn is_ok(&self) -> bool {
        self.result.success()
    }

    pub fn is_err(&self) -> bool {
        !self.result.success()
    }

    pub fn err(&self) -> Option<&[SemanticError]> {
        if self.is_err() {
            Some(self.result.diagnostics.as_slice())
        } else {
            None
        }
    }
}

impl std::ops::Deref for AnalyzedFixture {
    type Target = SemanticResult;
    fn deref(&self) -> &Self::Target {
        &self.result
    }
}

impl std::ops::DerefMut for AnalyzedFixture {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.result
    }
}

/// Parse source and run the semantic pipeline for `target`.
#[must_use]
pub fn analyze_source_for_target(source: &str, target: Target) -> AnalyzedFixture {
    analyze_source_with_config(source, PassConfig::for_target(target))
}

/// Parse source and run the semantic pipeline with the historical Rust-shaped
/// pass config (borrow analysis on). No backend is invoked.
#[must_use]
pub fn analyze_source(source: &str) -> AnalyzedFixture {
    analyze_source_for_target(source, Target::HirRust)
}

/// Parse source and run analysis with an explicit [`PassConfig`].
#[must_use]
pub fn analyze_source_with_config(source: &str, config: PassConfig) -> AnalyzedFixture {
    let mut parsed = if let Some(pack) = config.locale_pack.as_ref() {
        parse(radix_lexer::lex_with_locale_pack(
            source,
            pack as &dyn radix_lexer::KeywordPack,
        ))
    } else {
        parse(lex(source))
    };
    if !parsed.errors.is_empty() {
        let diagnostics = parsed
            .errors
            .iter()
            .map(|err| crate::SemanticError {
                kind: crate::SemanticErrorKind::LoweringError,
                args: err.args.clone(),
                span: err.span,
            })
            .collect();
        return AnalyzedFixture {
            result: crate::early_semantic_result(
                crate::TypeTable::new(),
                crate::Resolver::new(),
                crate::RadixLaneMetadata::default(),
                crate::AnnotationContractMetadata::default(),
                // Parse failure: region inference never ran.
                None,
                diagnostics,
            ),
            interner: parsed.interner,
        };
    }
    let program = parsed.program.as_ref().expect("program");
    let result = crate::analyze(program, &config, &mut parsed.interner);
    AnalyzedFixture {
        result,
        interner: parsed.interner,
    }
}

/// Analyze with caller-supplied namespace exports and file interfaces.
#[must_use]
pub fn analyze_source_with_import_contract(
    source: &str,
    namespace_exports: BTreeMap<String, BTreeMap<String, VisibilityTier>>,
    file_interfaces: BTreeMap<String, FileInterface>,
) -> AnalyzedFixture {
    let mut config = PassConfig::for_target(Target::HirRust);
    config.namespace_exports = namespace_exports;
    config.file_interfaces = file_interfaces;
    analyze_source_with_config(source, config)
}

/// Analyze and assert no error diagnostics.
#[must_use]
pub fn analyze_ok(source: &str) -> AnalyzedFixture {
    let result = analyze_source(source);
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
    result
}

/// TypeIds of `incipit` locals, in source order.
pub fn entry_local_tys(result: &AnalyzedFixture) -> Vec<TypeId> {
    let hir = result.hir.as_ref().expect("hir");
    let entry = hir.entry.as_ref().expect("incipit");
    entry
        .statements
        .iter()
        .filter_map(|stmt| match &stmt.kind {
            HirStatementKind::Local(local) => local.ty,
            _ => None,
        })
        .collect()
}

/// Named `issue` argument on a semantic diagnostic, if present.
#[must_use]
pub fn issue(err: &SemanticError) -> Option<&str> {
    err.args
        .iter()
        .find(|arg| arg.name == "issue")
        .map(|arg| arg.value.as_str())
}

/// Whether `args` omit legacy prose attachments.
#[must_use]
pub fn args_have_no_legacy_message(args: &[DiagnosticArg]) -> bool {
    !args.iter().any(|arg| arg.name == "legacy_message")
}

/// Check that a [`SemanticError`] uses structured args and carries `issue`.
#[must_use]
pub fn semantic_error_has_issue(err: &SemanticError, expected: &str) -> bool {
    args_have_no_legacy_message(&err.args) && issue(err) == Some(expected)
}

/// Default zero-width span for hand-built fixtures.
#[must_use]
pub fn test_span() -> Span {
    Span::default()
}

/// Byte span covering `start..end` in hand-built fixtures.
#[must_use]
pub fn test_span_between(start: u32, end: u32) -> Span {
    Span::new(start, end)
}

/// Assert analysis succeeds (no error diagnostics).
pub fn assert_accepts(source: &str) {
    let result = analyze_source(source);
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
}

/// Assert analysis succeeds for a specific target.
pub fn assert_accepts_for_target(source: &str, target: Target) {
    let result = analyze_source_for_target(source, target);
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
}

/// Assert analysis fails for `target` with an error diagnostic carrying `issue`.
pub fn assert_rejects_with_issue_for_target(source: &str, target: Target, expected: &str) {
    let result = analyze_source_for_target(source, target);
    assert!(
        !result.is_ok(),
        "expected failure, got: {:?}",
        result.diagnostics
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|err| err.is_error() && issue(err) == Some(expected)),
        "expected diagnostic issue {expected:?}, got: {:?}",
        result.diagnostics
    );
}

/// Assert analysis fails with an error diagnostic carrying `issue`.
pub fn assert_rejects_with_issue(source: &str, expected: &str) {
    let result = analyze_source(source);
    assert!(
        !result.is_ok(),
        "expected failure, got: {:?}",
        result.diagnostics
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|err| err.is_error() && issue(err) == Some(expected)),
        "expected diagnostic issue {expected:?}, got: {:?}",
        result.diagnostics
    );
}

/// Assert analysis fails with an error diagnostic whose args equal `args`.
pub fn assert_rejects_with_args(source: &str, args: &[DiagnosticArg]) {
    let result = analyze_source(source);
    assert!(
        !result.is_ok(),
        "expected failure, got: {:?}",
        result.diagnostics
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|err| err.is_error() && err.args == args),
        "expected diagnostic with args {args:?}, got: {:?}",
        result.diagnostics
    );
}

/// Assert analysis emits a warning carrying `issue`.
pub fn assert_warns_with_issue(source: &str, expected: &str) {
    let result = analyze_source(source);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|err| !err.is_error() && issue(err) == Some(expected)),
        "expected warning with issue {expected:?}, got: {:?}",
        result.diagnostics
    );
}

/// Analysis-path alias of [`assert_accepts`].
pub fn assert_analysis_accepts(source: &str) {
    assert_accepts(source);
}

/// Analysis-path alias of [`assert_rejects_with_issue`].
pub fn assert_analysis_rejects_with_issue(source: &str, expected: &str) {
    assert_rejects_with_issue(source, expected);
}

/// Diagnostics from analysis (errors and warnings).
#[must_use]
pub fn analyze_and_get_diagnostics(source: &str) -> Vec<SemanticError> {
    analyze_source(source).diagnostics.clone()
}

/// Historical compile-path alias: analysis only, no backend.
#[must_use]
pub fn compile_and_get_result(source: &str) -> AnalyzedFixture {
    analyze_source(source)
}

/// Historical compile-path alias: analysis diagnostics only.
#[must_use]
pub fn compile_and_get_diagnostics(source: &str) -> Vec<SemanticError> {
    analyze_and_get_diagnostics(source)
}

/// Historical compile-path alias for a specific target.
#[must_use]
pub fn compile_for_target_and_get_result(source: &str, target: Target) -> AnalyzedFixture {
    analyze_source_for_target(source, target)
}

// ---------------------------------------------------------------------------
// Latin (`la`) reader-pack variants
// ---------------------------------------------------------------------------
//
// Latin test sources name their reader locale explicitly (operator ruling
// 2026-10-11): a pack-less call no longer exists. Each `_la` sibling installs
// the built-in `la` pack and then runs the same pipeline as its pack-less twin.

/// Load the built-in Latin (`la`) reader pack used by the `_la` helpers.
fn latin_pack() -> LocalePack {
    LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("la"))
        .expect("load Latin locale pack")
}

/// Parse source with the `la` pack and run the semantic pipeline for `target`.
#[must_use]
pub fn analyze_source_for_target_la(source: &str, target: Target) -> AnalyzedFixture {
    analyze_source_with_config_la(source, PassConfig::for_target(target))
}

/// Parse source with the `la` pack and the historical Rust-shaped pass config.
#[must_use]
pub fn analyze_source_la(source: &str) -> AnalyzedFixture {
    analyze_source_for_target_la(source, Target::HirRust)
}

/// Parse source with the `la` pack and an explicit [`PassConfig`].
#[must_use]
pub fn analyze_source_with_config_la(source: &str, mut config: PassConfig) -> AnalyzedFixture {
    config.locale_pack = Some(latin_pack());
    analyze_source_with_config(source, config)
}

/// Analyze with the `la` pack and caller-supplied namespace exports and file interfaces.
#[must_use]
pub fn analyze_source_with_import_contract_la(
    source: &str,
    namespace_exports: BTreeMap<String, BTreeMap<String, VisibilityTier>>,
    file_interfaces: BTreeMap<String, FileInterface>,
) -> AnalyzedFixture {
    let mut config = PassConfig::for_target(Target::HirRust);
    config.namespace_exports = namespace_exports;
    config.file_interfaces = file_interfaces;
    analyze_source_with_config_la(source, config)
}

/// Analyze with the `la` pack and assert no error diagnostics.
#[must_use]
pub fn analyze_ok_la(source: &str) -> AnalyzedFixture {
    let result = analyze_source_la(source);
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
    result
}

/// Assert `la` analysis succeeds (no error diagnostics).
pub fn assert_accepts_la(source: &str) {
    let result = analyze_source_la(source);
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
}

/// Assert `la` analysis succeeds for a specific target.
pub fn assert_accepts_for_target_la(source: &str, target: Target) {
    let result = analyze_source_for_target_la(source, target);
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
}

/// Assert `la` analysis fails for `target` with an error diagnostic carrying `issue`.
pub fn assert_rejects_with_issue_for_target_la(source: &str, target: Target, expected: &str) {
    let result = analyze_source_for_target_la(source, target);
    assert!(
        !result.is_ok(),
        "expected failure, got: {:?}",
        result.diagnostics
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|err| err.is_error() && issue(err) == Some(expected)),
        "expected diagnostic issue {expected:?}, got: {:?}",
        result.diagnostics
    );
}

/// Assert `la` analysis fails with an error diagnostic carrying `issue`.
pub fn assert_rejects_with_issue_la(source: &str, expected: &str) {
    let result = analyze_source_la(source);
    assert!(
        !result.is_ok(),
        "expected failure, got: {:?}",
        result.diagnostics
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|err| err.is_error() && issue(err) == Some(expected)),
        "expected diagnostic issue {expected:?}, got: {:?}",
        result.diagnostics
    );
}

/// Assert `la` analysis fails with an error diagnostic whose args equal `args`.
pub fn assert_rejects_with_args_la(source: &str, args: &[DiagnosticArg]) {
    let result = analyze_source_la(source);
    assert!(
        !result.is_ok(),
        "expected failure, got: {:?}",
        result.diagnostics
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|err| err.is_error() && err.args == args),
        "expected diagnostic with args {args:?}, got: {:?}",
        result.diagnostics
    );
}

/// Assert `la` analysis emits a warning carrying `issue`.
pub fn assert_warns_with_issue_la(source: &str, expected: &str) {
    let result = analyze_source_la(source);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|err| !err.is_error() && issue(err) == Some(expected)),
        "expected warning with issue {expected:?}, got: {:?}",
        result.diagnostics
    );
}

/// `la` analysis-path alias of [`assert_accepts_la`].
pub fn assert_analysis_accepts_la(source: &str) {
    assert_accepts_la(source);
}

/// `la` analysis-path alias of [`assert_rejects_with_issue_la`].
pub fn assert_analysis_rejects_with_issue_la(source: &str, expected: &str) {
    assert_rejects_with_issue_la(source, expected);
}

/// Diagnostics from `la` analysis (errors and warnings).
#[must_use]
pub fn analyze_and_get_diagnostics_la(source: &str) -> Vec<SemanticError> {
    analyze_source_la(source).diagnostics.clone()
}

/// `la` compile-path alias: analysis only, no backend.
#[must_use]
pub fn compile_and_get_result_la(source: &str) -> AnalyzedFixture {
    analyze_source_la(source)
}

/// `la` compile-path alias: analysis diagnostics only.
#[must_use]
pub fn compile_and_get_diagnostics_la(source: &str) -> Vec<SemanticError> {
    analyze_and_get_diagnostics_la(source)
}

/// `la` compile-path alias for a specific target.
#[must_use]
pub fn compile_for_target_and_get_result_la(source: &str, target: Target) -> AnalyzedFixture {
    analyze_source_for_target_la(source, target)
}

#[cfg(test)]
mod tests {
    use super::analyze_ok_la;

    #[test]
    fn la_helpers_read_latin() {
        let unit = analyze_ok_la(
            "incipit {\n    sit [caput, ceteri cauda] \u{2190} [1, 2, 3, 4, 5]\n    nota caput\n    nota cauda\n}\n",
        );
        assert!(unit.is_ok(), "diagnostics: {:?}", unit.diagnostics);
    }
}
