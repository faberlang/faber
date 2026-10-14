//! Shared test harness for typecheck-level integration tests.
//!
//! Consolidates the three previously-duplicated test backends:
//! - `convert_test.rs`  (Compiler::compile_str → CompileResult)
//! - `scalar_numeric_width_test.rs` (Compiler + driver::analyze_source)
//! - `typecheck_test.rs` (direct typecheck() + structured_issue on SemanticError)
//!
//! The module provides two pipeline families:
//! - **Compile path** — full compiler (parse → lower → typecheck → codegen).
//!   Use for tests that need to validate codegen success/failure or backend-
//!   specific diagnostics.
//! - **Analysis path** — `driver::analyze_source` (parse → lower → typecheck,
//!   stop before codegen). Use when codegen would reject but the typecheck
//!   result is the subject under test.
//!
//! Every source-taking helper has an `_la` sibling that installs the built-in
//! Latin reader pack explicitly; Latin test sources call that sibling.
//!
//! Some assertion helpers are intentionally provided for future tests;
//! `#[allow(dead_code)]` avoids noise until callers materialize.

#![allow(dead_code)]

use crate::codegen::Target;
use crate::diagnostics::Diagnostic;
use crate::driver::{self, AnalyzeRequest, AnalyzedModule};
use crate::hir::HirStatementKind;
use crate::semantic::{SemanticError, TypeId};
use crate::{CompileResult, Compiler, Config};

/// File name used for diagnostic identity in compile/analyse calls.
const TEST_NAME: &str = "test.fab";

fn config_for(target: Target) -> Config {
    Config::default().with_target(target)
}

fn latin_config_for(target: Target) -> Config {
    Config::default().with_target(target).with_latin_locale()
}

// ---------------------------------------------------------------------------
// Compile path (full pipeline: parse → lower → typecheck → codegen)
// ---------------------------------------------------------------------------

fn compile_with_config(config: Config, source: &str) -> CompileResult {
    Compiler::new(config).compile_str(TEST_NAME, source)
}

/// Run the full compiler pipeline with the default Rust target.
#[must_use]
pub fn compile_and_get_result(source: &str) -> CompileResult {
    compile_with_config(config_for(Target::HirRust), source)
}

/// [`compile_and_get_result`] reading the source with the Latin reader pack.
#[must_use]
pub fn compile_and_get_result_la(source: &str) -> CompileResult {
    compile_with_config(latin_config_for(Target::HirRust), source)
}

/// Run the full compiler pipeline for a specific backend target.
#[must_use]
pub fn compile_for_target_and_get_result(source: &str, target: Target) -> CompileResult {
    compile_with_config(config_for(target), source)
}

/// [`compile_for_target_and_get_result`] reading the source with the Latin pack.
#[must_use]
pub fn compile_for_target_and_get_result_la(source: &str, target: Target) -> CompileResult {
    compile_with_config(latin_config_for(target), source)
}

/// Run the full compiler pipeline and return raw diagnostics.
#[must_use]
pub fn compile_and_get_diagnostics(source: &str) -> Vec<Diagnostic> {
    compile_and_get_result(source).diagnostics
}

/// [`compile_and_get_diagnostics`] reading the source with the Latin pack.
#[must_use]
pub fn compile_and_get_diagnostics_la(source: &str) -> Vec<Diagnostic> {
    compile_and_get_result_la(source).diagnostics
}

// ---------------------------------------------------------------------------
// Analysis path (parse → lower → typecheck; no codegen)
// ---------------------------------------------------------------------------

fn analyze_with_config(config: Config, source: &str) -> Vec<Diagnostic> {
    let session = driver::Session::new(config);
    match driver::analyze_source(&session, TEST_NAME, source, AnalyzeRequest::default()) {
        Ok(unit) => unit.diagnostics,
        Err(diagnostics) => diagnostics,
    }
}

/// Run analysis (typecheck only, no codegen) and return diagnostics.
///
/// Diagnostics are collected from both the `Ok(AnalyzedModule.diagnostics)` and
/// `Err(Vec<Diagnostic>)` arms so callers always receive a flat list.
#[must_use]
pub fn analyze_and_get_diagnostics(source: &str) -> Vec<Diagnostic> {
    analyze_with_config(config_for(Target::HirRust), source)
}

/// [`analyze_and_get_diagnostics`] reading the source with the Latin pack.
#[must_use]
pub fn analyze_and_get_diagnostics_la(source: &str) -> Vec<Diagnostic> {
    analyze_with_config(latin_config_for(Target::HirRust), source)
}

fn analyze_ok_with_config(config: Config, source: &str) -> AnalyzedModule {
    let session = driver::Session::new(config);
    let unit = driver::analyze_source(&session, TEST_NAME, source, AnalyzeRequest::default()).expect("analyze");
    assert!(
        !unit.diagnostics.iter().any(Diagnostic::is_error),
        "diagnostics: {:?}",
        unit.diagnostics
    );
    unit
}

/// Analyze and return the unit when typecheck produced no errors.
pub fn analyze_ok(source: &str) -> AnalyzedModule {
    analyze_ok_with_config(config_for(Target::HirRust), source)
}

/// [`analyze_ok`] reading the source with the Latin pack.
pub fn analyze_ok_la(source: &str) -> AnalyzedModule {
    analyze_ok_with_config(latin_config_for(Target::HirRust), source)
}

/// TypeIds of `incipit` locals, in source order.
pub fn entry_local_tys(unit: &AnalyzedModule) -> Vec<TypeId> {
    let entry = unit.hir.entry.as_ref().expect("incipit");
    entry
        .statements
        .iter()
        .filter_map(|stmt| match &stmt.kind {
            HirStatementKind::Local(local) => local.ty,
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Compile-path assertions
// ---------------------------------------------------------------------------

fn assert_result_accepts(result: &CompileResult) {
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
}

/// Compile with the default Rust target and assert success (no error diagnostics).
pub fn assert_accepts(source: &str) {
    assert_result_accepts(&compile_and_get_result(source));
}

/// [`assert_accepts`] reading the source with the Latin pack.
pub fn assert_accepts_la(source: &str) {
    assert_result_accepts(&compile_and_get_result_la(source));
}

/// Compile for a specific target and assert success.
pub fn assert_accepts_for_target(source: &str, target: Target) {
    assert_result_accepts(&compile_for_target_and_get_result(source, target));
}

/// [`assert_accepts_for_target`] reading the source with the Latin pack.
pub fn assert_accepts_for_target_la(source: &str, target: Target) {
    assert_result_accepts(&compile_for_target_and_get_result_la(source, target));
}

fn assert_rejects_result_with_issue(result: &CompileResult, issue: &str) {
    assert!(!result.success(), "expected failure, got: {:?}", result.diagnostics);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diag| diag.is_error() && diag.issue() == Some(issue)),
        "expected diagnostic issue {issue:?}, got: {:?}",
        result.diagnostics
    );
}

/// Compile with the default Rust target, assert failure, and assert at least
/// one ERROR-level diagnostic carries the given issue tag.
///
/// Does **not** match warnings — only diagnostics where `is_error()` is true.
pub fn assert_rejects_with_issue(source: &str, issue: &str) {
    assert_rejects_result_with_issue(&compile_and_get_result(source), issue);
}

/// [`assert_rejects_with_issue`] reading the source with the Latin pack.
pub fn assert_rejects_with_issue_la(source: &str, issue: &str) {
    assert_rejects_result_with_issue(&compile_and_get_result_la(source), issue);
}

/// Compile for a specific target, assert failure, and assert an ERROR
/// diagnostic carries the given issue tag.
pub fn assert_rejects_with_issue_for_target(source: &str, target: Target, issue: &str) {
    assert_rejects_result_with_issue(&compile_for_target_and_get_result(source, target), issue);
}

/// [`assert_rejects_with_issue_for_target`] reading the source with the Latin pack.
pub fn assert_rejects_with_issue_for_target_la(source: &str, target: Target, issue: &str) {
    assert_rejects_result_with_issue(&compile_for_target_and_get_result_la(source, target), issue);
}

fn assert_warns_result_with_issue(result: &CompileResult, issue: &str) {
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diag| !diag.is_error() && diag.issue() == Some(issue)),
        "expected warning with issue {issue:?}, got: {:?}",
        result.diagnostics
    );
}

/// Compile with the default Rust target and assert at least one non-error
/// (warning) diagnostic carries the given issue tag.
pub fn assert_warns_with_issue(source: &str, issue: &str) {
    assert_warns_result_with_issue(&compile_and_get_result(source), issue);
}

/// [`assert_warns_with_issue`] reading the source with the Latin pack.
pub fn assert_warns_with_issue_la(source: &str, issue: &str) {
    assert_warns_result_with_issue(&compile_and_get_result_la(source), issue);
}

// ---------------------------------------------------------------------------
// Analysis-path assertions
// ---------------------------------------------------------------------------

fn assert_diagnostics_accept(diagnostics: &[Diagnostic]) {
    assert!(!diagnostics.iter().any(Diagnostic::is_error), "diagnostics: {diagnostics:?}");
}

/// Run analysis-only and assert no error diagnostics.
pub fn assert_analysis_accepts(source: &str) {
    assert_diagnostics_accept(&analyze_and_get_diagnostics(source));
}

/// [`assert_analysis_accepts`] reading the source with the Latin pack.
pub fn assert_analysis_accepts_la(source: &str) {
    assert_diagnostics_accept(&analyze_and_get_diagnostics_la(source));
}

fn assert_diagnostics_reject_with_issue(diagnostics: &[Diagnostic], issue: &str) {
    assert!(
        diagnostics.iter().any(Diagnostic::is_error),
        "expected failure, got: {diagnostics:?}"
    );
    assert!(
        diagnostics
            .iter()
            .any(|diag| diag.is_error() && diag.issue() == Some(issue)),
        "expected diagnostic issue {issue:?}, got: {diagnostics:?}",
    );
}

/// Run analysis-only, assert at least one error diagnostic, and assert an
/// error diagnostic carries the given issue tag.
pub fn assert_analysis_rejects_with_issue(source: &str, issue: &str) {
    assert_diagnostics_reject_with_issue(&analyze_and_get_diagnostics(source), issue);
}

/// [`assert_analysis_rejects_with_issue`] reading the source with the Latin pack.
pub fn assert_analysis_rejects_with_issue_la(source: &str, issue: &str) {
    assert_diagnostics_reject_with_issue(&analyze_and_get_diagnostics_la(source), issue);
}

// ---------------------------------------------------------------------------
// SemanticError helpers (for direct typecheck() callers)
// ---------------------------------------------------------------------------

/// Check that a [`SemanticError`] uses structured args (no legacy message) and
/// carries the given issue tag.
///
/// This is the canonical replacement for the per-file `structured_issue`
/// helper that was duplicated in `typecheck_test.rs`.
#[must_use]
pub fn semantic_error_has_issue(err: &SemanticError, issue: &str) -> bool {
    crate::test_support::args_have_no_legacy_message(&err.args)
        && err
            .args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == issue)
}
