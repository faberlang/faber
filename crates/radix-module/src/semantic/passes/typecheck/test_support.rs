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
//! Some assertion helpers are intentionally provided for future tests;
//! `#[allow(dead_code)]` avoids noise until callers materialize.

#![allow(dead_code)]

use crate::codegen::Target;
use crate::diagnostics::Diagnostic;
use crate::driver::{self, AnalyzedModule};
use crate::hir::HirStatementKind;
use crate::semantic::{SemanticError, TypeId};
use crate::{CompileResult, Compiler, Config};

/// File name used for diagnostic identity in compile/analyse calls.
const TEST_NAME: &str = "test.fab";

// ---------------------------------------------------------------------------
// Compile path (full pipeline: parse → lower → typecheck → codegen)
// ---------------------------------------------------------------------------

/// Run the full compiler pipeline with the default Rust target.
pub fn compile_and_get_result(source: &str) -> CompileResult {
    Compiler::new(Config::default().with_target(Target::HirRust)).compile_str(TEST_NAME, source)
}

/// Run the full compiler pipeline for a specific backend target.
pub fn compile_for_target_and_get_result(source: &str, target: Target) -> CompileResult {
    Compiler::new(Config::default().with_target(target)).compile_str(TEST_NAME, source)
}

/// Run the full compiler pipeline and return raw diagnostics.
pub fn compile_and_get_diagnostics(source: &str) -> Vec<Diagnostic> {
    compile_and_get_result(source).diagnostics
}

// ---------------------------------------------------------------------------
// Analysis path (parse → lower → typecheck; no codegen)
// ---------------------------------------------------------------------------

/// Run analysis (typecheck only, no codegen) and return diagnostics.
///
/// Diagnostics are collected from both the `Ok(AnalyzedModule.diagnostics)` and
/// `Err(Vec<Diagnostic>)` arms so callers always receive a flat list.
pub fn analyze_and_get_diagnostics(source: &str) -> Vec<Diagnostic> {
    let session = driver::Session::new(Config::default().with_target(Target::HirRust));
    match driver::analyze_source(&session, TEST_NAME, source) {
        Ok(unit) => unit.diagnostics,
        Err(diagnostics) => diagnostics,
    }
}

/// Analyze and return the unit when typecheck produced no errors.
pub fn analyze_ok(source: &str) -> AnalyzedModule {
    let session = driver::Session::new(Config::default().with_target(Target::HirRust));
    let unit = driver::analyze_source(&session, TEST_NAME, source).expect("analyze");
    assert!(
        !unit.diagnostics.iter().any(Diagnostic::is_error),
        "diagnostics: {:?}",
        unit.diagnostics
    );
    unit
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

/// Compile with the default Rust target and assert success (no error diagnostics).
pub fn assert_accepts(source: &str) {
    let result = compile_and_get_result(source);
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
}

/// Compile for a specific target and assert success.
pub fn assert_accepts_for_target(source: &str, target: Target) {
    let result = compile_for_target_and_get_result(source, target);
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
}

/// Compile with the default Rust target, assert failure, and assert at least
/// one ERROR-level diagnostic carries the given issue tag.
///
/// Does **not** match warnings — only diagnostics where `is_error()` is true.
pub fn assert_rejects_with_issue(source: &str, issue: &str) {
    let result = compile_and_get_result(source);
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

/// Compile for a specific target, assert failure, and assert an ERROR
/// diagnostic carries the given issue tag.
pub fn assert_rejects_with_issue_for_target(source: &str, target: Target, issue: &str) {
    let result = compile_for_target_and_get_result(source, target);
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

/// Compile with the default Rust target and assert at least one non-error
/// (warning) diagnostic carries the given issue tag.
pub fn assert_warns_with_issue(source: &str, issue: &str) {
    let result = compile_and_get_result(source);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diag| !diag.is_error() && diag.issue() == Some(issue)),
        "expected warning with issue {issue:?}, got: {:?}",
        result.diagnostics
    );
}

// ---------------------------------------------------------------------------
// Analysis-path assertions
// ---------------------------------------------------------------------------

/// Run analysis-only and assert no error diagnostics.
pub fn assert_analysis_accepts(source: &str) {
    let diagnostics = analyze_and_get_diagnostics(source);
    assert!(!diagnostics.iter().any(Diagnostic::is_error), "diagnostics: {diagnostics:?}");
}

/// Run analysis-only, assert at least one error diagnostic, and assert an
/// error diagnostic carries the given issue tag.
pub fn assert_analysis_rejects_with_issue(source: &str, issue: &str) {
    let diagnostics = analyze_and_get_diagnostics(source);
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

// ---------------------------------------------------------------------------
// SemanticError helpers (for direct typecheck() callers)
// ---------------------------------------------------------------------------

/// Check that a [`SemanticError`] uses structured args (no legacy message) and
/// carries the given issue tag.
///
/// This is the canonical replacement for the per-file `structured_issue`
/// helper that was duplicated in `typecheck_test.rs`.
pub fn semantic_error_has_issue(err: &SemanticError, issue: &str) -> bool {
    crate::test_support::args_have_no_legacy_message(&err.args)
        && err
            .args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == issue)
}
