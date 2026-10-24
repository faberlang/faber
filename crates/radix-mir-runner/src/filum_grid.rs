//! The whole-grid filum reduction (`FilumReduce`, form `Grid`, gpu-reset
//! C4-8) run as one sequential fold.
//!
//! A device emitter splits the node into a grid-stride stage and a combine
//! stage. The runner reads the same node as its meaning: the term of every
//! source element in row-major order, folded left to right with the
//! operator; an empty grid yields the `coalesce` identity. This is the form
//! the CPU device runs (the kernel's device-form MIR is one whole call).
//!
//! Scalar operators include `sum` and `product` (a binary32 term rounds each
//! partial, as the device partials do), `max` / `min` (the first extreme wins
//! a tie; a number beats NaN), and `argmax` / `argmin` (the row-major index of
//! that winner). Grouped sum/count fold into a typed bucket tensor; each key
//! axis keeps the same fatal out-of-range index behavior as a tensor access.

use super::{Runner, RunnerError, Value};
use radix_lexer::Span;
use radix_mir::{MirConstant, MirFilumReduceOp, MirOperand, MirReduceOp};
use radix_runtime_contract::tensor::ERR_ACCIPE_INVALID_INDEX;
use radix_types::{NumericWidth, Primitive, Type, TypeId};

impl<H: super::Host + ?Sized> Runner<'_, H> {
    /// Evaluate a scalar grid-form `FilumReduce` call: `args` are the source tensor,
    /// one extent per axis, the optional identity, then a function term's
    /// captures and its function constant.
    pub(crate) fn eval_filum_grid_reduce(
        &mut self,
        op: &MirFilumReduceOp,
        args: &[MirOperand],
        return_ty: radix_mir::MirType,
        span: Span,
    ) -> Result<Value, RunnerError> {
        if op.group.is_some() {
            return self.eval_filum_grouped_reduce(op, args, return_ty, span);
        }
        let rank = op.coordinates.len();
        let term_operands = op.term_captures.map_or(0, |captures| captures as usize + 1);
        let has_identity = args.len() == 2 + rank + term_operands;
        let (Some(source), true) = (args.first(), args.len() >= 1 + rank + term_operands) else {
            return Err(RunnerError::internal(span, "filum grid reduce arity"));
        };
        let Value::Tensor(tensor) = self.eval_operand(source, span)? else {
            return Err(RunnerError::internal(
                span,
                "filum grid reduce source is not a tensor",
            ));
        };
        let mut extents = Vec::with_capacity(rank);
        for operand in &args[1..=rank] {
            let extent = match self.eval_operand(operand, span)? {
                Value::Int(value) => usize::try_from(value).ok(),
                Value::UInt(value) => usize::try_from(value).ok(),
                _ => None,
            }
            .ok_or_else(|| RunnerError::internal(span, "filum grid reduce extent"))?;
            extents.push(extent);
        }
        let identity = if has_identity {
            Some(self.eval_operand(&args[1 + rank], span)?)
        } else {
            None
        };
        let term = match op.term_captures {
            Some(count) => {
                let start = 1 + rank + usize::from(has_identity);
                let mut captures = Vec::with_capacity(count as usize);
                for operand in &args[start..start + count as usize] {
                    captures.push(self.eval_operand(operand, span)?);
                }
                let Some(MirOperand::Constant(MirConstant::Function(function))) = args.last()
                else {
                    return Err(RunnerError::internal(
                        span,
                        "filum grid reduce function term has no function constant",
                    ));
                };
                Some((captures, *function))
            }
            None if op.term_body.is_empty() => None,
            None => {
                return Err(RunnerError::unsupported(
                    span,
                    "intrinsic FilumReduce(grid, flat term body)",
                ));
            }
        };
        let round_f32 = matches!(
            self.types.get(op.term_ty.semantic_id()),
            Type::SizedNumeric(Primitive::Fractus, NumericWidth::F32)
        );
        let total = extents
            .iter()
            .try_fold(1_usize, |acc, extent| acc.checked_mul(*extent))
            .ok_or_else(|| RunnerError::internal(span, "filum grid reduce extent overflow"))?;
        let arg = match op.op {
            MirReduceOp::ArgMax => Some(MirReduceOp::Max),
            MirReduceOp::ArgMin => Some(MirReduceOp::Min),
            _ => None,
        };
        let mut coordinate = vec![0_usize; rank];
        let mut folded: Option<Value> = None;
        let mut winner = 0_usize;
        for linear in 0..total {
            let mut rest = linear;
            for axis in (0..rank).rev() {
                coordinate[axis] = rest % extents[axis];
                rest /= extents[axis];
            }
            let element = self.tensor_read_at(&tensor, linear, span)?.ok_or_else(|| {
                RunnerError::internal(span, "filum grid reduce reads outside its source")
            })?;
            let value = match &term {
                None => element,
                Some((captures, function)) => {
                    let mut arguments = captures.clone();
                    for axis in &coordinate {
                        let axis = i64::try_from(*axis).map_err(|_| {
                            RunnerError::internal(span, "filum grid reduce coordinate")
                        })?;
                        arguments.push(Value::Int(axis));
                    }
                    arguments.push(element);
                    self.invoke_runtime_values(*function, arguments, span)?
                }
            };
            let unsupported = || {
                RunnerError::unsupported(
                    span,
                    &format!("intrinsic FilumReduce(grid, {})", op.op.name()),
                )
            };
            folded = Some(match folded {
                None => round_term(value, round_f32),
                Some(previous) => {
                    let combined =
                        combine(arg.unwrap_or(op.op), previous.clone(), value, round_f32)
                            .ok_or_else(unsupported)?;
                    // The arg form moves its index only when the extreme moves.
                    if arg.is_some() && !same_value(&previous, &combined) {
                        winner = linear;
                    }
                    combined
                }
            });
        }
        if arg.is_some() && folded.is_some() {
            let index = i64::try_from(winner)
                .map_err(|_| RunnerError::internal(span, "filum grid reduce index"))?;
            return Ok(Value::Int(index));
        }
        folded
            .or(identity)
            .ok_or_else(|| RunnerError::internal(span, "empty filum grid reduce has no identity"))
    }

    fn eval_filum_grouped_reduce(
        &mut self,
        op: &MirFilumReduceOp,
        args: &[MirOperand],
        return_ty: radix_mir::MirType,
        span: Span,
    ) -> Result<Value, RunnerError> {
        let group = op
            .group
            .as_ref()
            .ok_or_else(|| RunnerError::internal(span, "grouped filum metadata is missing"))?;
        if !matches!(op.op, MirReduceOp::Sum | MirReduceOp::Count)
            || group.keys.is_empty()
            || group.keys.len() != group.dimensions.len()
        {
            return Err(RunnerError::unsupported(
                span,
                &format!("intrinsic FilumReduce(grid, grouped {})", op.op.name()),
            ));
        }
        let rank = op.coordinates.len();
        let term_operands = op.term_captures.map_or(0, |captures| captures as usize + 1);
        let group_operands = group
            .keys
            .iter()
            .try_fold(0_usize, |count, key| {
                count.checked_add(usize::try_from(key.captures).ok()?.checked_add(1)?)
            })
            .ok_or_else(|| RunnerError::internal(span, "filum grouped helper operand overflow"))?;
        let base = 1 + rank;
        let group_start = args
            .len()
            .checked_sub(group_operands)
            .ok_or_else(|| RunnerError::internal(span, "filum grouped reduce arity"))?;
        let identity_operands = group_start
            .checked_sub(base + term_operands)
            .ok_or_else(|| RunnerError::internal(span, "filum grouped reduce helper arity"))?;
        if identity_operands > 1 {
            return Err(RunnerError::internal(
                span,
                "filum grouped reduce identity arity",
            ));
        }
        if identity_operands == 1 {
            return Err(RunnerError::internal(
                span,
                "grouped sum/count carries an explicit identity",
            ));
        }
        let Some(source) = args.first() else {
            return Err(RunnerError::internal(span, "filum grouped reduce source"));
        };
        let Value::Tensor(tensor) = self.eval_operand(source, span)? else {
            return Err(RunnerError::internal(
                span,
                "filum grouped source is not a tensor",
            ));
        };
        let mut extents = Vec::with_capacity(rank);
        for operand in &args[1..=rank] {
            let extent = match self.eval_operand(operand, span)? {
                Value::Int(value) => usize::try_from(value).ok(),
                Value::UInt(value) => usize::try_from(value).ok(),
                _ => None,
            }
            .ok_or_else(|| RunnerError::internal(span, "filum grouped source extent"))?;
            extents.push(extent);
        }

        let term = match op.term_captures {
            Some(count) => {
                let count = usize::try_from(count)
                    .map_err(|_| RunnerError::internal(span, "filum term capture count"))?;
                let mut captures = Vec::with_capacity(count);
                for operand in &args[base..base + count] {
                    captures.push(self.eval_operand(operand, span)?);
                }
                let Some(MirOperand::Constant(MirConstant::Function(function))) =
                    args.get(base + count)
                else {
                    return Err(RunnerError::internal(
                        span,
                        "filum grouped term function constant is missing",
                    ));
                };
                Some((captures, *function))
            }
            None if op.term_body.is_empty() => None,
            None => {
                return Err(RunnerError::unsupported(
                    span,
                    "intrinsic FilumReduce(grid, flat term body)",
                ));
            }
        };

        let mut helpers = Vec::with_capacity(group.keys.len());
        let mut position = group_start;
        for key in &group.keys {
            let captures_count = usize::try_from(key.captures)
                .map_err(|_| RunnerError::internal(span, "filum group capture count"))?;
            let end = position
                .checked_add(captures_count)
                .ok_or_else(|| RunnerError::internal(span, "filum group capture range"))?;
            let mut captures = Vec::with_capacity(captures_count);
            for operand in &args[position..end] {
                captures.push(self.eval_operand(operand, span)?);
            }
            let Some(MirOperand::Constant(MirConstant::Function(function))) = args.get(end) else {
                return Err(RunnerError::internal(
                    span,
                    "filum group key function constant is missing",
                ));
            };
            helpers.push((captures, *function));
            position = end
                .checked_add(1)
                .ok_or_else(|| RunnerError::internal(span, "filum group helper position"))?;
        }
        if position != args.len() {
            return Err(RunnerError::internal(
                span,
                "filum grouped helper operands were not consumed",
            ));
        }

        let (result_element, result_shape) = match self.types.get(return_ty.semantic_id()) {
            Type::Tensor(element, shape) => {
                let return_shape = radix_types::i64s_from_literal_index(*shape, self.types)
                    .ok_or_else(|| {
                        RunnerError::internal(span, "filum grouped return shape is not literal")
                    })?;
                let group_shape = group
                    .dimensions
                    .iter()
                    .map(|extent| i64::try_from(*extent).ok())
                    .collect::<Option<Vec<_>>>()
                    .ok_or_else(|| RunnerError::internal(span, "filum group shape extent"))?;
                if group_shape != return_shape || *element != group.result_element_ty.semantic_id()
                {
                    return Err(RunnerError::internal(
                        span,
                        "filum grouped result type differs from its group shape",
                    ));
                }
                (*element, group_shape)
            }
            _ => {
                return Err(RunnerError::internal(
                    span,
                    "filum grouped result is not a tensor",
                ));
            }
        };
        let dimensions = group
            .dimensions
            .iter()
            .map(|extent| {
                usize::try_from(*extent)
                    .map_err(|_| RunnerError::internal(span, "filum group extent exceeds usize"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let bucket_count = dimensions
            .iter()
            .try_fold(1_usize, |count, extent| count.checked_mul(*extent))
            .ok_or_else(|| RunnerError::internal(span, "filum group bucket count overflow"))?;
        let zero = reduction_zero(self.types, result_element, span)?;
        let mut buckets = vec![zero; bucket_count];
        let total = extents
            .iter()
            .try_fold(1_usize, |count, extent| count.checked_mul(*extent))
            .ok_or_else(|| RunnerError::internal(span, "filum grid reduce extent overflow"))?;
        let round_f32 = matches!(
            self.types.get(op.term_ty.semantic_id()),
            Type::SizedNumeric(Primitive::Fractus, NumericWidth::F32)
        );
        let mut coordinate = vec![0_usize; rank];
        for linear in 0..total {
            let mut rest = linear;
            for axis in (0..rank).rev() {
                coordinate[axis] = rest % extents[axis];
                rest /= extents[axis];
            }
            let element = self.tensor_read_at(&tensor, linear, span)?.ok_or_else(|| {
                RunnerError::internal(span, "filum grid reduce reads outside its source")
            })?;
            let coordinates = coordinate
                .iter()
                .map(|coordinate| {
                    i64::try_from(*coordinate)
                        .map(Value::Int)
                        .map_err(|_| RunnerError::internal(span, "filum grid reduce coordinate"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let term_value = match &term {
                None => element,
                Some((captures, function)) => {
                    let mut arguments = captures.clone();
                    arguments.extend(coordinates.iter().cloned());
                    arguments.push(element);
                    self.invoke_runtime_values(*function, arguments, span)?
                }
            };
            let mut bucket = 0_usize;
            for (axis, ((captures, function), extent)) in
                helpers.iter().zip(&dimensions).enumerate()
            {
                let mut arguments = captures.clone();
                arguments.extend(coordinates.iter().cloned());
                let key = self.invoke_runtime_values(*function, arguments, span)?;
                let index = match key {
                    Value::Int(value) => usize::try_from(value).ok(),
                    Value::UInt(value) => usize::try_from(value).ok(),
                    _ => None,
                }
                .filter(|index| *index < *extent)
                .ok_or_else(|| RunnerError::internal(span, ERR_ACCIPE_INVALID_INDEX))?;
                bucket = bucket
                    .checked_mul(*extent)
                    .and_then(|bucket| bucket.checked_add(index))
                    .ok_or_else(|| {
                        RunnerError::internal(span, "filum group bucket index overflow")
                    })?;
                if axis >= group.dimensions.len() {
                    return Err(RunnerError::internal(span, "filum group helper axis"));
                }
            }
            let previous = buckets
                .get(bucket)
                .cloned()
                .ok_or_else(|| RunnerError::internal(span, "filum group bucket outside result"))?;
            let combined = match op.op {
                MirReduceOp::Sum => combine(MirReduceOp::Sum, previous, term_value, round_f32)
                    .ok_or_else(|| {
                        RunnerError::unsupported(span, "intrinsic FilumReduce(grid, sum)")
                    })?,
                MirReduceOp::Count => match term_value {
                    Value::Bool(true) => count_one(previous).ok_or_else(|| {
                        RunnerError::unsupported(span, "intrinsic FilumReduce(grid, count)")
                    })?,
                    Value::Bool(false) => previous,
                    _ => {
                        return Err(RunnerError::internal(
                            span,
                            "filum count term is not bivalens",
                        ));
                    }
                },
                _ => unreachable!("grouped operator was checked above"),
            };
            buckets[bucket] = combined;
        }
        crate::runtime::tensor_value_typed(
            self.types,
            buckets,
            Some(result_element),
            result_shape,
            span,
        )
    }
}

fn reduction_zero(
    types: &radix_types::TypeTable,
    element: TypeId,
    span: Span,
) -> Result<Value, RunnerError> {
    match types.get(element) {
        Type::Primitive(Primitive::Fractus) | Type::SizedNumeric(Primitive::Fractus, _) => {
            Ok(Value::Float(0.0))
        }
        Type::SizedNumeric(
            Primitive::Numerus,
            NumericWidth::U8 | NumericWidth::U16 | NumericWidth::U32 | NumericWidth::U64,
        ) => Ok(Value::UInt(0)),
        Type::Primitive(Primitive::Numerus) | Type::SizedNumeric(Primitive::Numerus, _) => {
            Ok(Value::Int(0))
        }
        _ => Err(RunnerError::internal(
            span,
            "grouped sum/count result element is not numeric",
        )),
    }
}

fn count_one(value: Value) -> Option<Value> {
    match value {
        Value::Int(value) => Some(Value::Int(value.checked_add(1)?)),
        Value::UInt(value) => Some(Value::UInt(value.checked_add(1)?)),
        _ => None,
    }
}

/// Whether two folded scalars are the same value, bit for bit.
fn same_value(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Float(a), Value::Float(b)) => a.to_bits() == b.to_bits(),
        (Value::Int(a), Value::Int(b)) => a == b,
        (Value::UInt(a), Value::UInt(b)) => a == b,
        _ => false,
    }
}

fn round_term(value: Value, round_f32: bool) -> Value {
    match value {
        Value::Float(v) if round_f32 => Value::Float(f64::from(v as f32)),
        other => other,
    }
}

/// Fold `next` into `previous`; `None` for an operator or operand kind the
/// sequential form does not cover.
pub(super) fn combine(
    op: MirReduceOp,
    previous: Value,
    next: Value,
    round_f32: bool,
) -> Option<Value> {
    Some(match (op, previous, next) {
        (MirReduceOp::Sum, Value::Float(a), Value::Float(b)) => {
            round_term(Value::Float(a + b), round_f32)
        }
        (MirReduceOp::Product, Value::Float(a), Value::Float(b)) => {
            round_term(Value::Float(a * b), round_f32)
        }
        (MirReduceOp::Sum, Value::Int(a), Value::Int(b)) => Value::Int(a.checked_add(b)?),
        (MirReduceOp::Product, Value::Int(a), Value::Int(b)) => Value::Int(a.checked_mul(b)?),
        (MirReduceOp::Sum, Value::UInt(a), Value::UInt(b)) => Value::UInt(a.checked_add(b)?),
        (MirReduceOp::Product, Value::UInt(a), Value::UInt(b)) => Value::UInt(a.checked_mul(b)?),
        (MirReduceOp::Max | MirReduceOp::Min, Value::Float(a), Value::Float(b)) => {
            let wins = if op == MirReduceOp::Max { b > a } else { b < a };
            Value::Float(if !b.is_nan() && (a.is_nan() || wins) {
                b
            } else {
                a
            })
        }
        (MirReduceOp::Max, Value::Int(a), Value::Int(b)) => Value::Int(a.max(b)),
        (MirReduceOp::Min, Value::Int(a), Value::Int(b)) => Value::Int(a.min(b)),
        (MirReduceOp::Max, Value::UInt(a), Value::UInt(b)) => Value::UInt(a.max(b)),
        (MirReduceOp::Min, Value::UInt(a), Value::UInt(b)) => Value::UInt(a.min(b)),
        _ => return None,
    })
}
