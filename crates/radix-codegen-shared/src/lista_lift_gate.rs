//! Fail-closed gate for the elementwise list forms on HIR-backed targets.
//!
//! `+ - * /` between a list and a scalar of the list's own element type, and
//! `+ -` between two lists, keep their HIR emitters. The forms added with the
//! 2026-10-04 aggregate-operator rulings have only the MIR runner as their
//! executor: `%` and `÷` over a list, unary `-` over a list, the glyph forms
//! `⊙ ⊘` over two lists, the per-element comparisons `≺ ≻ ≤ ≥ ≅ ≇` over
//! numeric lists, and a list against a scalar whose type joins the element to
//! a wider one (`list<u8> + 300`). Left alone, a HIR emitter would
//! print the scalar operator between a list and a number, which is a wrong
//! program, not a missing one. This gate rejects those expressions before any
//! HIR emitter runs, naming the operator and the target.

use crate::CodegenError;
use radix_diagnostics::DiagnosticArg;
use radix_hir::visit::{HirVisitor, walk_expr, walk_program};
use radix_hir::{HirBinOp, HirExpression, HirExpressionKind, HirModule, HirUnOp};
use radix_target::Target;
use radix_types::{Type, TypeId, TypeTable};

const ISSUE: &str = "lista_lift_unsupported_on_target";

/// # Errors
///
/// Returns a [`CodegenError`] naming the first elementwise list expression
/// that only the MIR runner executes.
pub fn reject_lista_lift(
    hir: &HirModule,
    types: &TypeTable,
    target: Target,
) -> Result<(), CodegenError> {
    struct Gate<'a> {
        types: &'a TypeTable,
        found: Option<&'static str>,
    }
    impl HirVisitor for Gate<'_> {
        fn visit_expr(&mut self, expr: &HirExpression) {
            if self.found.is_some() {
                return;
            }
            self.found = lista_form(self.types, expr);
            if self.found.is_none() {
                walk_expr(self, expr);
            }
        }
    }
    if matches!(target, Target::HirFaber) {
        return Ok(());
    }
    let mut gate = Gate { types, found: None };
    walk_program(&mut gate, hir);
    match gate.found {
        None => Ok(()),
        Some(label) => Err(CodegenError {
            message: format!(
                "list elementwise {label} is not supported on the {target:?} target: \
                 the form has only the MIR runner as its executor"
            ),
            args: vec![
                DiagnosticArg::new("issue", ISSUE),
                DiagnosticArg::new("op", label),
                DiagnosticArg::new("target", format!("{target:?}")),
            ],
        }),
    }
}

/// The element type of a list type, aliases and access markers peeled.
fn lista_element(types: &TypeTable, ty: Option<TypeId>) -> Option<TypeId> {
    let mut ty = types.peel_access_marker(ty?).1;
    while let Type::Alias(_, inner) = types.get(ty) {
        ty = *inner;
    }
    match types.get(ty) {
        Type::Array(element) | Type::BoundedArray { element, .. } => Some(*element),
        _ => None,
    }
}

/// The label of a list form only the runner executes, when `expr` is one.
fn lista_form(types: &TypeTable, expr: &HirExpression) -> Option<&'static str> {
    match &expr.kind {
        HirExpressionKind::Unary(HirUnOp::Neg, operand) => {
            lista_element(types, operand.ty).map(|_| "-")
        }
        HirExpressionKind::Binary(op, lhs, rhs) => {
            let (left, right) = (lista_element(types, lhs.ty), lista_element(types, rhs.ty));
            if left.is_none() && right.is_none() {
                return None;
            }
            let spelling = match op {
                HirBinOp::Mod => "%",
                HirBinOp::TrueDiv => "÷",
                HirBinOp::Hadamard => "⊙",
                HirBinOp::HadamardDiv => "⊘",
                // A numeric list compared per element types as a list of
                // `bivalens`; a whole-value `≅` over other lists stays one
                // `bivalens` and keeps its emitters.
                HirBinOp::Lt
                | HirBinOp::Gt
                | HirBinOp::LtEq
                | HirBinOp::GtEq
                | HirBinOp::ApproxEq
                | HirBinOp::ApproxNotEq => {
                    lista_element(types, expr.ty)?;
                    match op {
                        HirBinOp::Lt => "≺",
                        HirBinOp::Gt => "≻",
                        HirBinOp::LtEq => "≤",
                        HirBinOp::GtEq => "≥",
                        HirBinOp::ApproxEq => "≅",
                        _ => "≇",
                    }
                }
                HirBinOp::Add | HirBinOp::Sub | HirBinOp::Mul | HirBinOp::Div => {
                    // The existing emitters take a list element and a scalar of
                    // that same type; a join to a wider element is new.
                    let result = lista_element(types, expr.ty)?;
                    let operand = left.or(right)?;
                    return (!types.equals(result, operand)).then_some(match op {
                        HirBinOp::Add => "+",
                        HirBinOp::Sub => "-",
                        HirBinOp::Mul => "*",
                        _ => "/",
                    });
                }
                _ => return None,
            };
            Some(spelling)
        }
        _ => None,
    }
}
