//! Fail-closed gate for the K4 tensor lift on HIR-backed targets.
//!
//! The elementwise lift of scalar operators over tensors (`a + 1.0`, `a > b`,
//! `non m`, `m ✓ a ✗ b`, `t vel 0`, `a ⤒ b`, mixed-width tensor pairs) lowers
//! to one MIR runtime call whose reference executor is the MIR runner. The
//! HIR emitters have no lift of their own: left alone they would print the
//! scalar operator between tensor values, which is a wrong program, not a
//! missing one. This gate rejects those expressions before any HIR emitter
//! runs, naming the operator and the target, so no backend can miscompile
//! them silently. Tensor `+ -` against a tensor of the same element type and
//! the `⊙ ⊘` products keep their emitters.

use crate::CodegenError;
use radix_diagnostics::DiagnosticArg;
use radix_hir::visit::{HirVisitor, walk_expr, walk_program};
use radix_hir::{HirBinOp, HirExpression, HirExpressionKind, HirModule, HirUnOp};
use radix_lexer::Interner;
use radix_target::Target;
use radix_types::{Type, TypeId, TypeTable};

const ISSUE: &str = "tensor_lift_unsupported_on_target";

/// # Errors
///
/// Returns a [`CodegenError`] naming the first lifted tensor expression.
pub fn reject_tensor_lift(
    hir: &HirModule,
    types: &TypeTable,
    interner: &Interner,
    target: Target,
) -> Result<(), CodegenError> {
    if matches!(target, Target::HirFaber) {
        return Ok(());
    }
    struct Gate<'a> {
        types: &'a TypeTable,
        interner: &'a Interner,
        found: Option<String>,
    }
    impl HirVisitor for Gate<'_> {
        fn visit_expr(&mut self, expr: &HirExpression) {
            if self.found.is_some() {
                return;
            }
            self.found = lifted_form(self.types, self.interner, expr);
            if self.found.is_none() {
                walk_expr(self, expr);
            }
        }
    }
    let mut gate = Gate {
        types,
        interner,
        found: None,
    };
    walk_program(&mut gate, hir);
    match gate.found {
        None => Ok(()),
        Some(label) => Err(CodegenError {
            message: format!(
                "tensor elementwise {label} is not supported on the {target:?} target: \
                 the lifted operator has only the MIR runner as its executor"
            ),
            args: vec![
                DiagnosticArg::new("issue", ISSUE),
                DiagnosticArg::new("op", label),
                DiagnosticArg::new("target", format!("{target:?}")),
            ],
        }),
    }
}

/// The tensor of a type, aliases and access markers peeled.
fn tensor_element(types: &TypeTable, ty: Option<TypeId>) -> Option<TypeId> {
    let mut ty = types.peel_access_marker(ty?).1;
    while let Type::Alias(_, inner) = types.get(ty) {
        ty = *inner;
    }
    match types.get(ty) {
        Type::Tensor(element, _) => Some(*element),
        _ => None,
    }
}

/// The label of a lifted form, when `expr` is one.
fn lifted_form(types: &TypeTable, interner: &Interner, expr: &HirExpression) -> Option<String> {
    match &expr.kind {
        HirExpressionKind::Binary(op, lhs, rhs) => binary_label(
            types,
            *op,
            tensor_element(types, lhs.ty),
            tensor_element(types, rhs.ty),
        ),
        HirExpressionKind::Unary(op, operand) => {
            tensor_element(types, operand.ty)?;
            match op {
                HirUnOp::Neg => Some("-".to_owned()),
                HirUnOp::Not => Some("non".to_owned()),
                HirUnOp::BitNot => None,
            }
        }
        HirExpressionKind::Si { cond, .. } => {
            tensor_element(types, cond.ty)?;
            Some("✓ ✗".to_owned())
        }
        HirExpressionKind::MethodCall(receiver, name, _, args, _) => {
            let extremum = matches!(interner.resolve(*name), "maximus" | "minimus");
            (extremum
                && tensor_element(types, receiver.ty).is_some()
                && args
                    .first()
                    .is_some_and(|arg| tensor_element(types, arg.expr.ty).is_some()))
            .then(|| "⤒ ⤓".to_owned())
        }
        _ => None,
    }
}

fn binary_label(
    types: &TypeTable,
    op: HirBinOp,
    left: Option<TypeId>,
    right: Option<TypeId>,
) -> Option<String> {
    let spelling = match op {
        HirBinOp::Add => "+",
        HirBinOp::Sub => "-",
        HirBinOp::Mul => "*",
        HirBinOp::Div => "/",
        HirBinOp::Mod => "%",
        HirBinOp::TrueDiv => "÷",
        HirBinOp::Eq | HirBinOp::StrictEq => "≡",
        HirBinOp::NotEq | HirBinOp::StrictNotEq => "≠",
        HirBinOp::ApproxEq => "≅",
        HirBinOp::ApproxNotEq => "≇",
        HirBinOp::FuzzyEq => "≈",
        HirBinOp::FuzzyNotEq => "≉",
        HirBinOp::Lt => "<",
        HirBinOp::Gt => ">",
        HirBinOp::LtEq => "≤",
        HirBinOp::GtEq => "≥",
        HirBinOp::And => "et",
        HirBinOp::Or => "aut",
        HirBinOp::Coalesce => "vel",
        HirBinOp::Hadamard => "⊙",
        HirBinOp::HadamardDiv => "⊘",
        _ => return None,
    };
    match (left, right) {
        (None, None) => None,
        // Two tensors: the retained `+ - ⊙ ⊘` emitters need identical elements.
        (Some(l), Some(r)) => {
            let retained = matches!(
                op,
                HirBinOp::Add | HirBinOp::Sub | HirBinOp::Hadamard | HirBinOp::HadamardDiv
            );
            (!retained || !types.equals(l, r)).then(|| spelling.to_owned())
        }
        // A tensor against a scalar: every lifted spelling; `⊙ ⊘` never take one.
        _ => Some(spelling.to_owned()),
    }
}
