//! Fail-closed gate for the list window `xs[lo‥hi]` (ruling B10 of need
//! 6a13d711).
//!
//! A range index on a list is a window: a new list holding the range, fully in
//! bounds or a failure. It lowers to no HIR kind and no MIR op of its own (it is
//! `Index(list, interval)` in HIR and `Collection(Index)` with an interval
//! argument in MIR), so no exhaustive match forces an emitter to refuse it, and
//! a backend would print a scalar element read or fail with an unnamed error.
//! The Faber source emitter (which prints the window back), the MIR runner
//! (which executes it), the clif backend and the Go, TypeScript, Swift and
//! Python emitters (which copy the range behind a checked helper) implement the
//! form, as does the Rust emitter (which is checked in [`is_list_window`]).
//! A receiver is a list window in either shape, the unbounded `lista<T>`
//! ([`Type::Array`]) and the bounded `lista<T, N>` ([`Type::BoundedArray`]);
//! every emitter listed here dispatches both, so admission is per target, not
//! per receiver shape.
//! This gate rejects any module that takes one before an emitter runs, naming
//! the target, until a target's own emission lands and adds itself to
//! [`target_admits_list_window`]. LLVM text, LLVM host, Wasm text and sexp remain
//! intentionally fail-closed; their durable per-target follow-up references
//! (`7797c432-list-window-llvm-text`, `7797c432-list-window-llvm-host`,
//! `7797c432-list-window-wasm-text`, `7797c432-list-window-sexp`) are recorded
//! beside the admission list. The
//! octeti window, plain or under an endian conversion `buf[lo‥hi] ↦ W via Be`,
//! indexes a byte receiver (`lista<b8>`, the word `octeti`), not a list, and is
//! untouched.

use crate::CodegenError;
use radix_diagnostics::DiagnosticArg;
use radix_hir::visit::{HirVisitor, walk_expr, walk_program};
use radix_hir::{HirExpression, HirExpressionKind, HirModule};
use radix_target::Target;
use radix_types::{BitsWidth, Type, TypeId, TypeTable};

const ISSUE: &str = "list_window_unsupported_on_target";

/// Targets that carry a list window faithfully. The Faber re-emitter keeps the
/// authored index; the MIR runner executes it and does not reach codegen; the
/// clif backend lowers it to a runtime slice; the Go, TypeScript, Swift and
/// Python emitters copy the range, striding by an optional step, for both the
/// unbounded and the bounded list receiver. LLVM, sexp and
/// wasm still refuse it. These fail-closed residuals have durable per-target
/// follow-up references under need: `7797c432-list-window-llvm-text`
/// (`MirLlvm`), `7797c432-list-window-llvm-host` (`MirLlvmHost`),
/// `7797c432-list-window-wasm-text` (`MirWasm`), and
/// `7797c432-list-window-sexp` (`MirSexp`). Keep each target refused here until
/// its own emission and target-specific proof are complete.
fn target_admits_list_window(target: Target) -> bool {
    matches!(
        target,
        Target::HirFaber
            | Target::MirStepper
            | Target::MirClif
            | Target::HirGo
            | Target::HirTypeScript
            | Target::HirSwift
            | Target::HirPython
    )
}

/// Targets whose emitters carry the byte window `bytes[lo‥hi]` of a
/// `lista<b8>` (the window the retired `octeti` word had; need): a
/// fresh byte list copied from the range, in bounds or a failure.
fn target_admits_byte_window(target: Target) -> bool {
    matches!(
        target,
        Target::HirRust
            | Target::HirGo
            | Target::HirTypeScript
            | Target::HirSwift
            | Target::HirPython
    )
}

/// The type behind aliases and access markers.
fn resolved(types: &TypeTable, ty: TypeId) -> &Type {
    let mut ty = types.peel_access_marker(ty).1;
    while let Type::Alias(_, inner) = types.get(ty) {
        ty = *inner;
    }
    types.get(ty)
}

/// Whether `expr` is `list[range]`.
fn is_list_window(types: &TypeTable, expr: &HirExpression, target: Target) -> bool {
    let HirExpressionKind::Index(object, index) = &expr.kind else {
        return false;
    };
    let (Some(object_ty), Some(index_ty)) = (object.ty, index.ty) else {
        return false;
    };
    if !matches!(resolved(types, index_ty), Type::Intervallum(..)) {
        return false;
    }
    // The Rust emitter copies the window of any list or bounded list.
    if matches!(target, Target::HirRust)
        && matches!(
            resolved(types, object_ty),
            Type::Array(_) | Type::BoundedArray { .. }
        )
    {
        return false;
    }
    match resolved(types, object_ty) {
        Type::Array(element) => {
            !(target_admits_byte_window(target)
                && matches!(resolved(types, *element), Type::Bits(BitsWidth::B8)))
        }
        // A bounded list `lista<T, N>` is the same window over the same
        // carrier: its capacity bounds construction, never a range.
        Type::BoundedArray { .. } => true,
        _ => false,
    }
}

/// # Errors
///
/// Returns a [`CodegenError`] (`list_window_unsupported_on_target`) when the
/// module takes a list window and `target` has no emission for it.
pub fn reject_list_window(
    hir: &HirModule,
    types: &TypeTable,
    target: Target,
) -> Result<(), CodegenError> {
    struct Gate<'a> {
        types: &'a TypeTable,
        target: Target,
        found: bool,
    }
    impl HirVisitor for Gate<'_> {
        fn visit_expr(&mut self, expr: &HirExpression) {
            if self.found {
                return;
            }
            self.found = is_list_window(self.types, expr, self.target);
            if !self.found {
                walk_expr(self, expr);
            }
        }
    }
    if target_admits_list_window(target) {
        return Ok(());
    }
    let mut gate = Gate {
        types,
        target,
        found: false,
    };
    walk_program(&mut gate, hir);
    if !gate.found {
        return Ok(());
    }
    Err(CodegenError {
        message: format!(
            "the list window `xs[lo‥hi]` is not supported on the {target:?} target: \
             only the Faber source emitter and the MIR runner implement it"
        ),
        args: vec![
            DiagnosticArg::new("issue", ISSUE),
            DiagnosticArg::new("target", format!("{target:?}")),
        ],
    })
}

#[cfg(test)]
#[path = "list_window_gate_test.rs"]
mod tests;
