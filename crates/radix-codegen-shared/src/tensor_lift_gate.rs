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
//!
//! The register forms are gated the same way under their own issues,
//! `vector_lift_unsupported_on_target` and `matrix_lift_unsupported_on_target`:
//! `+ - * / % ÷` between a register and a scalar, unary `-`, `÷` between two
//! registers, and `⤒ ⤓` against a scalar or a register peer. Their per-lane
//! unrolling happens in MIR, so only the MIR-backed targets execute them;
//! register with register `+ -` and the `⊙ ⊘` products keep their emitters.

use crate::CodegenError;
use radix_diagnostics::DiagnosticArg;
use radix_hir::visit::{HirVisitor, walk_expr, walk_program};
use radix_hir::{HirBinOp, HirExpression, HirExpressionKind, HirModule, HirUnOp};
use radix_lexer::Interner;
use radix_target::Target;
use radix_types::{Type, TypeId, TypeTable};

const ISSUE: &str = "tensor_lift_unsupported_on_target";
const VECTOR_ISSUE: &str = "vector_lift_unsupported_on_target";
const MATRIX_ISSUE: &str = "matrix_lift_unsupported_on_target";

/// Which container a lifted form operates on.
#[derive(Clone, Copy)]
enum Family {
    Tensor,
    Vector,
    Matrix,
}

/// # Errors
///
/// Returns a [`CodegenError`] naming the first lifted tensor expression.
pub fn reject_tensor_lift(
    hir: &HirModule,
    types: &TypeTable,
    interner: &Interner,
    target: Target,
) -> Result<(), CodegenError> {
    struct Gate<'a> {
        types: &'a TypeTable,
        interner: &'a Interner,
        target: Target,
        found: Option<(Family, String, bool)>,
    }
    impl HirVisitor for Gate<'_> {
        fn visit_expr(&mut self, expr: &HirExpression) {
            if self.found.is_some() {
                return;
            }
            self.found = lifted_form(self.types, self.interner, self.target, expr)
                .map(|label| (Family::Tensor, label, false))
                .or_else(|| register_form(self.types, self.interner, expr));
            if self.found.is_none() {
                walk_expr(self, expr);
            }
        }
    }
    if matches!(target, Target::HirFaber) {
        return Ok(());
    }
    // The elementwise list forms only the MIR runner executes fail closed too.
    crate::lista_lift_gate::reject_lista_lift(hir, types, target)?;
    let mut gate = Gate {
        types,
        interner,
        target,
        found: None,
    };
    walk_program(&mut gate, hir);
    match gate.found {
        None => Ok(()),
        Some((Family::Tensor, label, _)) => Err(CodegenError {
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
        Some((family @ (Family::Vector | Family::Matrix), label, pair)) => {
            let (name, issue) = if matches!(family, Family::Vector) {
                ("vector", VECTOR_ISSUE)
            } else {
                ("matrix", MATRIX_ISSUE)
            };
            let operands = if pair {
                format!("between two {name}s")
            } else {
                "against a scalar".to_owned()
            };
            Err(CodegenError {
                message: format!(
                    "{name} {label} {operands} is not supported on the {target:?} target: \
                     the lanes are unrolled only on MIR-backed targets"
                ),
                args: vec![
                    DiagnosticArg::new("issue", issue),
                    DiagnosticArg::new("op", label),
                    DiagnosticArg::new("target", format!("{target:?}")),
                ],
            })
        }
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

/// The register family of a type, aliases and access markers peeled.
fn register_family(types: &TypeTable, ty: Option<TypeId>) -> Option<Family> {
    let mut ty = types.peel_access_marker(ty?).1;
    while let Type::Alias(_, inner) = types.get(ty) {
        ty = *inner;
    }
    match types.get(ty) {
        Type::Vector(..) => Some(Family::Vector),
        Type::Matrix(..) => Some(Family::Matrix),
        _ => None,
    }
}

/// The family and label of a register form lifted over a scalar or a peer,
/// when `expr` is one.
fn register_form(
    types: &TypeTable,
    interner: &Interner,
    expr: &HirExpression,
) -> Option<(Family, String, bool)> {
    let same_family = |lhs: Option<Family>, rhs: Option<Family>| {
        matches!(
            (lhs, rhs),
            (Some(Family::Vector), Some(Family::Vector))
                | (Some(Family::Matrix), Some(Family::Matrix))
        )
    };
    match &expr.kind {
        HirExpressionKind::Binary(op, lhs, rhs) => {
            let spelling = match op {
                HirBinOp::Add => "+",
                HirBinOp::Sub => "-",
                HirBinOp::Mul => "*",
                HirBinOp::Div => "/",
                HirBinOp::Mod => "%",
                HirBinOp::TrueDiv => "÷",
                _ => return None,
            };
            let (left, right) = (
                register_family(types, lhs.ty),
                register_family(types, rhs.ty),
            );
            // Register with register `+ -` keeps its emitters, and `* / %`
            // between two registers never type-check; only `÷` pairs them.
            if same_family(left, right) {
                return (*op == HirBinOp::TrueDiv)
                    .then(|| left.map(|family| (family, spelling.to_owned(), true)))
                    .flatten();
            }
            let scalar = |operand: &HirExpression| {
                tensor_element(types, operand.ty).is_none()
                    && register_family(types, operand.ty).is_none()
            };
            match (left, right) {
                (Some(family), None) if scalar(rhs) => Some((family, spelling.to_owned(), false)),
                (None, Some(family)) if scalar(lhs) => Some((family, spelling.to_owned(), false)),
                _ => None,
            }
        }
        HirExpressionKind::Unary(HirUnOp::Neg, operand) => {
            register_family(types, operand.ty).map(|family| (family, "-".to_owned(), false))
        }
        HirExpressionKind::MethodCall(receiver, name, _, args, _)
            if matches!(interner.resolve(*name), "maximus" | "minimus") =>
        {
            let family = register_family(types, receiver.ty)?;
            let peer = args.first()?;
            tensor_element(types, peer.expr.ty).is_none().then(|| {
                let pair = register_family(types, peer.expr.ty).is_some();
                (family, "⤒ ⤓".to_owned(), pair)
            })
        }
        _ => None,
    }
}

/// Whether a type is a tensor whose element type is nullable.
fn is_nullable_tensor(types: &TypeTable, ty: Option<TypeId>) -> bool {
    tensor_element(types, ty).is_some_and(|element| {
        let mut element = element;
        while let Type::Alias(_, inner) = types.get(element) {
            element = *inner;
        }
        matches!(types.get(element), Type::Option(_))
    })
}

/// The label of a lifted form, when `expr` is one.
fn lifted_form(
    types: &TypeTable,
    interner: &Interner,
    target: Target,
    expr: &HirExpression,
) -> Option<String> {
    match &expr.kind {
        // The Rust runtime carries `vel` over a nullable-element tensor (a
        // shifted edge view) with a scalar fallback as `Tensor::coalesce`.
        HirExpressionKind::Binary(HirBinOp::Coalesce, lhs, rhs)
            if matches!(target, Target::HirRust)
                && is_nullable_tensor(types, lhs.ty)
                && tensor_element(types, rhs.ty).is_none() =>
        {
            None
        }
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
