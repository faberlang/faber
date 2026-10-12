//! Elementwise operator lift over tensors (gpu-reset rule 15, K4).
//!
//! `MirIntrinsic::TensorLift(op)` applies the scalar operator per element with
//! the scalar operator's own semantics: integers compute exactly and each
//! result is stored under the result element type's slot policy (ruling
//! 19(g)), floats round at the element width, comparisons follow the scalar
//! comparison (NaN is unequal to everything and orders nowhere), `÷` converts
//! integers first and traps an integer zero divisor. A scalar operand
//! broadcasts; two tensor operands must have identical shapes. Both operands
//! of `And`/`Or` and all three of `Select` are already evaluated values, so
//! nothing short-circuits.

use super::exact_int::{exact_neg, int_value};
use super::value::{
    Value, compare_values, effective_width, ieee_maximum, ieee_minimum, numeric_binop, round_float,
    values_equal,
};
use super::{Runner, RunnerError};
use radix_lexer::Span;
use radix_mir::{MirBinOp, MirLiftOp, MirOperand, MirType};
use radix_types::{NumericWidth, Type, TypeId};

const ISSUE_SHAPE_MISMATCH: &str = "runner_tensor_lift_shape_mismatch";
const DIVISION_FAILED: &str = "numerus division failed";

impl<H: super::Host + ?Sized> Runner<'_, H> {
    /// Evaluate one lifted operator over its operands.
    pub(crate) fn eval_tensor_lift(
        &mut self,
        op: MirLiftOp,
        args: &[MirOperand],
        return_ty: MirType,
        span: Span,
    ) -> Result<Value, RunnerError> {
        if args.len() != op.arity() {
            return Err(RunnerError::internal(span, "tensor lift arity"));
        }
        let operands = args
            .iter()
            .map(|arg| self.eval_operand(arg, span))
            .collect::<Result<Vec<_>, _>>()?;
        if matches!(op, MirLiftOp::WholeEq | MirLiftOp::WholeNotEq) {
            let same = self.tensors_whole_equal(&operands, span)?;
            return Ok(Value::Bool(same == (op == MirLiftOp::WholeEq)));
        }
        let shape = lift_shape(&operands, span)?;
        let count = lift_count(&operands, span)?;
        let element = self.lift_result_element(return_ty);
        let mut data = Vec::with_capacity(count);
        for index in 0..count {
            let mut picked = Vec::with_capacity(operands.len());
            for value in &operands {
                picked.push(match value {
                    Value::Tensor(cell) => {
                        self.tensor_read_at(cell, index, span)?.ok_or_else(|| {
                            RunnerError::internal(span, "tensor lift layout is invalid")
                        })?
                    }
                    other => other.clone(),
                });
            }
            data.push(self.lift_element(op, &picked, element, span)?);
        }
        super::runtime::tensor_value_typed(self.types, data, element, shape, span)
    }

    /// `≡` over two whole tensors: the same shape and every element pair `≡`.
    /// A shape difference is `false`, never an error; NaN is not `≡` to
    /// anything, so a tensor holding one is not `≡` to itself.
    fn tensors_whole_equal(&mut self, operands: &[Value], span: Span) -> Result<bool, RunnerError> {
        let [Value::Tensor(left), Value::Tensor(right)] = operands else {
            return Err(RunnerError::internal(
                span,
                "tensor equality needs two tensor operands",
            ));
        };
        if left.borrow().shape != right.borrow().shape {
            return Ok(false);
        }
        let count = lift_count(operands, span)?;
        for index in 0..count {
            let l = self.tensor_read_at(left, index, span)?;
            let r = self.tensor_read_at(right, index, span)?;
            let (Some(l), Some(r)) = (l, r) else {
                return Err(RunnerError::internal(
                    span,
                    "tensor equality layout is invalid",
                ));
            };
            if !values_equal(&l, &r) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// The element type of the tensor result type.
    fn lift_result_element(&self, ty: MirType) -> Option<TypeId> {
        match self.types.get(ty.semantic_id()) {
            Type::Tensor(element, _) => Some(*element),
            _ => None,
        }
    }

    pub(crate) fn lift_element(
        &self,
        op: MirLiftOp,
        v: &[Value],
        element: Option<TypeId>,
        span: Span,
    ) -> Result<Value, RunnerError> {
        let width = element
            .and_then(|ty| effective_width(self.types, ty))
            .unwrap_or(NumericWidth::I64);
        let computed = match op {
            MirLiftOp::Add => numeric_binop(
                v[0].clone(),
                v[1].clone(),
                MirBinOp::Add,
                |a, b| a + b,
                width,
                span,
            )?,
            MirLiftOp::Sub => numeric_binop(
                v[0].clone(),
                v[1].clone(),
                MirBinOp::Sub,
                |a, b| a - b,
                width,
                span,
            )?,
            MirLiftOp::Mul => numeric_binop(
                v[0].clone(),
                v[1].clone(),
                MirBinOp::Mul,
                |a, b| a * b,
                width,
                span,
            )?,
            MirLiftOp::Div => numeric_binop(
                v[0].clone(),
                v[1].clone(),
                MirBinOp::Div,
                |a, b| a / b,
                width,
                span,
            )?,
            MirLiftOp::Mod => numeric_binop(
                v[0].clone(),
                v[1].clone(),
                MirBinOp::Mod,
                |a, b| a % b,
                width,
                span,
            )?,
            MirLiftOp::TrueDiv => true_divide(&v[0], &v[1], width, span)?,
            MirLiftOp::Neg => negate(&v[0], width, span)?,
            MirLiftOp::Not => Value::Bool(!v[0].as_bool(span)?),
            MirLiftOp::Eq => Value::Bool(values_equal(&v[0], &v[1])),
            MirLiftOp::NotEq => Value::Bool(!values_equal(&v[0], &v[1])),
            MirLiftOp::Lt => compare_values(v[0].clone(), v[1].clone(), MirBinOp::Lt, span)?,
            MirLiftOp::Gt => compare_values(v[0].clone(), v[1].clone(), MirBinOp::Gt, span)?,
            MirLiftOp::LtEq => compare_values(v[0].clone(), v[1].clone(), MirBinOp::LtEq, span)?,
            MirLiftOp::GtEq => compare_values(v[0].clone(), v[1].clone(), MirBinOp::GtEq, span)?,
            MirLiftOp::And => Value::Bool(v[0].as_bool(span)? && v[1].as_bool(span)?),
            MirLiftOp::Or => Value::Bool(v[0].as_bool(span)? || v[1].as_bool(span)?),
            MirLiftOp::Coalesce => {
                if matches!(v[0], Value::Nil) {
                    v[1].clone()
                } else {
                    v[0].clone()
                }
            }
            MirLiftOp::Max => extremum(&v[0], &v[1], true, span)?,
            MirLiftOp::Min => extremum(&v[0], &v[1], false, span)?,
            MirLiftOp::Select => {
                if v[0].as_bool(span)? {
                    v[1].clone()
                } else {
                    v[2].clone()
                }
            }
            MirLiftOp::WholeEq | MirLiftOp::WholeNotEq => {
                return Err(RunnerError::internal(
                    span,
                    "whole-tensor equality is not an elementwise lift",
                ));
            }
        };
        let Some(element) = element else {
            return Ok(computed);
        };
        let computed = self.apply_declared_float_width(computed, element);
        self.store_tensor_element(element, computed, span)
    }
}

/// The shared shape of the tensor operands; two tensors must agree.
fn lift_shape(operands: &[Value], span: Span) -> Result<Vec<i64>, RunnerError> {
    let mut shape: Option<Vec<i64>> = None;
    for operand in operands {
        let Value::Tensor(cell) = operand else {
            continue;
        };
        let borrowed = cell.borrow();
        match &shape {
            Some(seen) if *seen != borrowed.shape => {
                return Err(RunnerError::internal_with_issue(
                    span,
                    format!(
                        "tensor lift shapes {:?} and {:?} differ",
                        seen, borrowed.shape
                    ),
                    ISSUE_SHAPE_MISMATCH,
                ));
            }
            Some(_) => {}
            None => shape = Some(borrowed.shape.clone()),
        }
    }
    shape.ok_or_else(|| RunnerError::internal(span, "tensor lift has no tensor operand"))
}

/// The element count every tensor operand must back.
fn lift_count(operands: &[Value], span: Span) -> Result<usize, RunnerError> {
    let mut count: Option<usize> = None;
    for operand in operands {
        let Value::Tensor(cell) = operand else {
            continue;
        };
        let tensor = cell.borrow();
        if tensor.has_contiguous_backing_mismatch() {
            return Err(RunnerError::internal(
                span,
                "tensor lift backing-length mismatch",
            ));
        }
        let len = tensor
            .logical_len()
            .ok_or_else(|| RunnerError::internal(span, "tensor lift shape overflow"))?;
        if count.is_some_and(|seen| seen != len) {
            return Err(RunnerError::internal(
                span,
                "tensor lift backing-length mismatch",
            ));
        }
        count = Some(len);
    }
    count.ok_or_else(|| RunnerError::internal(span, "tensor lift has no tensor operand"))
}

#[allow(clippy::cast_precision_loss)] // integer to float widening is the intent
fn as_f64(value: &Value, span: Span) -> Result<f64, RunnerError> {
    match value {
        Value::Float(f) => Ok(*f),
        Value::Int(v) => Ok(*v as f64),
        Value::UInt(v) => Ok(*v as f64),
        _ => Err(RunnerError::internal(
            span,
            "tensor lift numeric operand mismatch",
        )),
    }
}

/// `÷`: integer operands convert to the result float first and an integer
/// zero divisor traps; float operands divide by IEEE rules.
fn true_divide(
    left: &Value,
    right: &Value,
    width: NumericWidth,
    span: Span,
) -> Result<Value, RunnerError> {
    let divisor_is_zero = int_value(right) == Some(0);
    if int_value(left).is_some() && divisor_is_zero {
        return Err(RunnerError::internal(span, DIVISION_FAILED));
    }
    let quotient = as_f64(left, span)? / as_f64(right, span)?;
    Ok(Value::Float(if width.is_float() {
        round_float(quotient, width)
    } else {
        quotient
    }))
}

fn negate(value: &Value, width: NumericWidth, span: Span) -> Result<Value, RunnerError> {
    if let Some(result) = exact_neg(value, width, span) {
        return result;
    }
    match value {
        Value::Float(f) => Ok(Value::Float(-f)),
        _ => Err(RunnerError::internal(
            span,
            "tensor lift numeric operand mismatch",
        )),
    }
}

/// `⤒`/`⤓` on one element pair: exact for integers, IEEE `maximum`/`minimum`
/// for floats.
fn extremum(left: &Value, right: &Value, larger: bool, span: Span) -> Result<Value, RunnerError> {
    if let (Some(a), Some(b)) = (int_value(left), int_value(right)) {
        let pick_left = if larger { a >= b } else { a <= b };
        return Ok(if pick_left {
            left.clone()
        } else {
            right.clone()
        });
    }
    let (a, b) = (as_f64(left, span)?, as_f64(right, span)?);
    Ok(Value::Float(if larger {
        ieee_maximum(a, b)
    } else {
        ieee_minimum(a, b)
    }))
}
