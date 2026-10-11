//! Async / cursor / flux lifecycle lowering (M-006).
//!
//! This module owns the yield/await surface that has a MIR runtime contract.
//! Source `cede` lowers through HIR `Yield` into a `MirIntrinsic::Cede` call
//! that preserves the typed operand produced by semantic analysis. The await
//! expression forms (`tacebit`/`reddet`) lower through the shared promissum
//! payload-carrier fact: an awaited call already produces the success payload,
//! so awaiting evaluates the call and either discards the payload (`tacebit`)
//! or materializes it for the enclosing return (`reddet`).

#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for lowering
use super::*;

impl FunctionBuilder<'_> {
    /// Lower `cede` into the async yield intrinsic for the expression's typed value.
    pub(super) fn lower_cede_expr(&mut self, inner: &HirExpression, expr: &HirExpression) -> Option<MirOperand> {
        let value = self.lower_expr_value(inner)?;
        let ty = self.expr_ty(expr)?;
        Some(self.runtime_call_value(MirIntrinsic::Cede, vec![value], ty, expr.span))
    }

    /// `tacebit future` — await a promise to completion and discard the success value.
    ///
    /// The awaited call lowers to the shared promissum payload carrier, so the
    /// call itself already produced the success payload. This evaluates the
    /// future for its effects and yields the vacuum carrier.
    pub(super) fn lower_tacebit_expr(&mut self, inner: &HirExpression, _expr: &HirExpression) -> Option<MirOperand> {
        self.lower_expr_value(inner)?;
        Some(MirOperand::Constant(MirConstant::Unit))
    }

    /// `reddet future` — await a promise and return its resolved value.
    ///
    /// The awaited call lowers to a promissum-typed carrier holding the success
    /// payload. Materialize the payload into a payload-typed temp so the
    /// enclosing `redde` return terminator validates against the function's
    /// declared return type (the payload, not the promise).
    pub(super) fn lower_reddet_expr(&mut self, inner: &HirExpression, expr: &HirExpression) -> Option<MirOperand> {
        let value = self.lower_expr_value(inner)?;
        let ty = self.expr_ty(expr)?;
        if self.is_vacuum(ty) {
            return Some(MirOperand::Constant(MirConstant::Unit));
        }
        Some(self.assign_temp(MirValueKind::Operand(value), ty, expr.span))
    }
}
