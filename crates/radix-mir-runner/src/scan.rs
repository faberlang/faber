//! CPU-device meaning of the typed scan collective. Host reference lowering
//! remains an ordinary ordered loop; this path evaluates its captured helpers.
use super::{Runner, RunnerError, Value};
use radix_lexer::Span;
use radix_mir::{MirConstant, MirFunctionId, MirOperand, MirReduceOp, MirScanOp, MirScanTotal};
use radix_types::{NumericWidth, Primitive, Type};
use std::collections::BTreeMap;

type Helper = (Vec<Value>, MirFunctionId);

impl<H: super::Host + ?Sized> Runner<'_, H> {
    pub(crate) fn eval_scan(
        &mut self,
        op: &MirScanOp,
        args: &[MirOperand],
        span: Span,
    ) -> Result<Value, RunnerError> {
        let invalid = || RunnerError::internal(span, "scan intrinsic contract");
        if op.total == MirScanTotal::Exclusive
            && matches!(op.op, MirReduceOp::Max | MirReduceOp::Min)
        {
            return Err(RunnerError::unsupported(
                span,
                "scan_exclusive_extremum_unsupported",
            ));
        }
        let Value::Tensor(source) = self.eval_operand(args.first().ok_or_else(invalid)?, span)?
        else {
            return Err(invalid());
        };
        let shape = source.borrow().shape.clone();
        if shape.len() != usize::from(op.rank) {
            return Err(invalid());
        }
        let extents = shape
            .iter()
            .map(|n| usize::try_from(*n).map_err(|_| invalid()))
            .collect::<Result<Vec<_>, _>>()?;
        let length = extents
            .iter()
            .try_fold(1usize, |n, e| n.checked_mul(*e))
            .ok_or_else(invalid)?;
        let mut offset = 1;
        let term = self.scan_helper(args, &mut offset, op.term_captures, span)?;
        let mut keys = Vec::new();
        if let Some(group) = &op.group {
            for axis in &group.axes {
                keys.push(self.scan_helper(args, &mut offset, axis.captures, span)?);
            }
        }
        if offset != args.len() {
            return Err(invalid());
        }
        let float = matches!(
            self.types.get(op.result_element_ty.semantic_id()),
            Type::SizedNumeric(Primitive::Fractus, NumericWidth::F32)
        );
        let identity = scan_identity(op.op, float);
        let mut states: BTreeMap<Vec<i128>, Value> = BTreeMap::new();
        let mut output = vec![Value::Nil; length];
        let mut key = Vec::with_capacity(keys.len());
        for position in 0..length {
            let linear = if op.reverse {
                length - position - 1
            } else {
                position
            };
            let mut remainder = linear;
            let mut coordinates = vec![Value::Int(0); extents.len()];
            for axis in (0..extents.len()).rev() {
                coordinates[axis] =
                    Value::Int(i64::try_from(remainder % extents[axis]).map_err(|_| invalid())?);
                remainder /= extents[axis];
            }
            let element = self
                .tensor_read_at(&source, linear, span)?
                .ok_or_else(invalid)?;
            let mut arguments = term.0.clone();
            arguments.extend(coordinates.iter().cloned());
            arguments.push(element);
            let value = self.invoke_runtime_values(term.1, arguments, span)?;
            key.clear();
            if let Some(group) = &op.group {
                for (helper, axis) in keys.iter().zip(&group.axes) {
                    let mut arguments = helper.0.clone();
                    arguments.extend(coordinates.iter().cloned());
                    let index = match self.invoke_runtime_values(helper.1, arguments, span)? {
                        Value::Int(value) => i128::from(value),
                        Value::UInt(value) => i128::from(value),
                        _ => return Err(invalid()),
                    };
                    if index
                        .checked_sub(axis.min)
                        .and_then(|n| u64::try_from(n).ok())
                        .is_none_or(|n| n >= axis.extent)
                    {
                        return Err(RunnerError::internal(
                            span,
                            radix_runtime_contract::tensor::ERR_ACCIPE_INVALID_INDEX,
                        ));
                    }
                    key.push(index);
                }
            }
            let previous = states.get_mut(key.as_slice());
            let previous_value = previous.as_deref().cloned().or_else(|| identity.clone());
            if op.total == MirScanTotal::Exclusive {
                output[linear] = previous_value.clone().ok_or_else(invalid)?;
            }
            let next = match previous_value {
                Some(previous) => {
                    scan_combine(op.op, previous, value, float).ok_or_else(invalid)?
                }
                None => value,
            };
            if op.total == MirScanTotal::Inclusive {
                output[linear] = next.clone();
            }
            if let Some(state) = previous {
                *state = next;
            } else {
                states.insert(key.clone(), next);
            }
        }
        super::runtime::tensor_value_typed(
            self.types,
            output,
            Some(op.result_element_ty.semantic_id()),
            shape,
            span,
        )
    }

    fn scan_helper(
        &mut self,
        args: &[MirOperand],
        offset: &mut usize,
        count: u32,
        span: Span,
    ) -> Result<Helper, RunnerError> {
        let end = offset
            .checked_add(count as usize)
            .ok_or_else(|| RunnerError::internal(span, "scan helper arity"))?;
        let operands = args
            .get(*offset..end)
            .ok_or_else(|| RunnerError::internal(span, "scan helper captures"))?;
        let mut captures = Vec::with_capacity(operands.len());
        for operand in operands {
            captures.push(self.eval_operand(operand, span)?);
        }
        let Some(MirOperand::Constant(MirConstant::Function(function))) = args.get(end) else {
            return Err(RunnerError::internal(span, "scan helper function"));
        };
        *offset = end + 1;
        Ok((captures, *function))
    }
}

fn scan_identity(op: MirReduceOp, float: bool) -> Option<Value> {
    Some(match op {
        MirReduceOp::Sum if float => Value::Float(0.0),
        MirReduceOp::Sum | MirReduceOp::Count => Value::Int(0),
        MirReduceOp::Product if float => Value::Float(1.0),
        MirReduceOp::Product => Value::Int(1),
        MirReduceOp::All => Value::Bool(true),
        MirReduceOp::Any => Value::Bool(false),
        _ => return None,
    })
}

fn scan_combine(op: MirReduceOp, previous: Value, next: Value, float: bool) -> Option<Value> {
    match (op, previous, next) {
        (MirReduceOp::Count, Value::Int(count), Value::Bool(term)) => {
            Some(Value::Int(count.checked_add(i64::from(term))?))
        }
        (MirReduceOp::All, Value::Bool(a), Value::Bool(b)) => Some(Value::Bool(a && b)),
        (MirReduceOp::Any, Value::Bool(a), Value::Bool(b)) => Some(Value::Bool(a || b)),
        (op, previous, next) => super::filum_grid::combine(op, previous, next, float),
    }
}
