//! Elementwise list operators: `xs + ys`, `xs * 2`, `-xs`, `xs ≺ ys` and the
//! rest of the array lifts.
//!
//! The oracle's rules (`radix-mir-runner` `array_lift`): two list operands
//! must hold the same number of live elements and pair by position, a scalar
//! operand broadcasts, each element is computed with the scalar operator's own
//! semantics (integers exactly, floats by IEEE, comparisons by the scalar
//! comparison) and then stored under the result element type's slot policy, so
//! a range check happens once per element at the store.

use super::{
    BlockArg, Decline, FunctionLower, InstBuilder, IntCC, Lowered, MirBinOp, MirCollectionOp,
    MirOperand, MirPlace, MirType, NumericWidth, Primitive, Rep, Type, Value, abi, rep_of, resolve,
    types,
};

/// Which elementwise operator a list operation applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Lift {
    Binary(MirBinOp),
    TrueDiv,
    Negate,
    Not,
}

/// The elementwise operator behind a list operation, if it is one.
pub(crate) fn list_lift(op: MirCollectionOp) -> Option<Lift> {
    Some(match op {
        MirCollectionOp::ArrayAdd => Lift::Binary(MirBinOp::Add),
        MirCollectionOp::ArraySub => Lift::Binary(MirBinOp::Sub),
        MirCollectionOp::ArrayMul => Lift::Binary(MirBinOp::Mul),
        MirCollectionOp::ArrayDiv => Lift::Binary(MirBinOp::Div),
        MirCollectionOp::ArrayMod => Lift::Binary(MirBinOp::Mod),
        MirCollectionOp::ArrayTrueDiv => Lift::TrueDiv,
        MirCollectionOp::ArrayNeg => Lift::Negate,
        MirCollectionOp::ArrayNot => Lift::Not,
        MirCollectionOp::ArrayAnd => Lift::Binary(MirBinOp::And),
        MirCollectionOp::ArrayOr => Lift::Binary(MirBinOp::Or),
        MirCollectionOp::ArrayLt => Lift::Binary(MirBinOp::Lt),
        MirCollectionOp::ArrayGt => Lift::Binary(MirBinOp::Gt),
        MirCollectionOp::ArrayLtEq => Lift::Binary(MirBinOp::LtEq),
        MirCollectionOp::ArrayGtEq => Lift::Binary(MirBinOp::GtEq),
        MirCollectionOp::ArrayValueEq => Lift::Binary(MirBinOp::Eq),
        MirCollectionOp::ArrayValueNotEq => Lift::Binary(MirBinOp::NotEq),
        _ => return None,
    })
}

/// The message of a length mismatch between two list operands.
fn mismatch_message(lift: Lift) -> &'static str {
    match lift {
        Lift::Binary(MirBinOp::Add) => "lista add length mismatch",
        Lift::Binary(MirBinOp::Sub) => "lista sub length mismatch",
        Lift::Binary(MirBinOp::Mul) => "lista mul length mismatch",
        Lift::Binary(MirBinOp::Div) => "lista div length mismatch",
        Lift::Binary(MirBinOp::Mod) => "lista mod length mismatch",
        Lift::TrueDiv => "lista true div length mismatch",
        Lift::Binary(MirBinOp::Lt) => "lista lt length mismatch",
        Lift::Binary(MirBinOp::Gt) => "lista gt length mismatch",
        Lift::Binary(MirBinOp::LtEq) => "lista lt-eq length mismatch",
        Lift::Binary(MirBinOp::GtEq) => "lista gt-eq length mismatch",
        Lift::Binary(MirBinOp::Eq) => "lista value-eq length mismatch",
        Lift::Binary(MirBinOp::NotEq) => "lista value-not-eq length mismatch",
        Lift::Binary(MirBinOp::And) => "lista and length mismatch",
        Lift::Binary(MirBinOp::Or) => "lista or length mismatch",
        Lift::Not => "lista not length mismatch",
        _ => "lista neg length mismatch",
    }
}

/// One operand of a lift: a list handle (read per element) or a scalar.
struct Operand {
    value: Value,
    /// The representation of the element, or of the scalar.
    rep: Rep,
    list: bool,
}

impl FunctionLower<'_, '_> {
    pub(super) fn list_lift(
        &mut self,
        destination: Option<&MirPlace>,
        lift: Lift,
        args: &[MirOperand],
    ) -> Lowered<()> {
        let Some(destination) = destination else {
            return Err(Decline::new("list lift without destination"));
        };
        let arity = if matches!(lift, Lift::Negate | Lift::Not) {
            1
        } else {
            2
        };
        if args.len() != arity {
            return Err(Decline::new("list lift arity"));
        }
        let table = self.state.input.types;
        let result_element = self.element_type(self.place_type(destination)?)?;
        if matches!(
            resolve(table, result_element.semantic_id()),
            Type::ModularWord(_) | Type::SaturatingWord(_)
        ) {
            return Err(Decline::new("list lift into a modular or saturating word"));
        }
        let result_rep = rep_of(table, result_element)?;
        let mut operands = Vec::with_capacity(arity);
        for arg in args {
            let (value, rep) = self.scalar(arg)?;
            let (rep, is_list) = if rep == Rep::List {
                let element = self.element_type(self.operand_type(arg)?)?;
                (rep_of(table, element)?, true)
            } else {
                (rep, false)
            };
            if !matches!(rep, Rep::Int | Rep::Float | Rep::Bool) {
                return Err(Decline::new(format!("list lift over {rep:?} elements")));
            }
            operands.push(Operand {
                value,
                rep,
                list: is_list,
            });
        }
        let mut length: Option<Value> = None;
        for operand in operands.iter().filter(|operand| operand.list) {
            let live = self
                .call_runtime(&abi::LIST_LEN, &[operand.value])?
                .ok_or_else(|| Decline::new("length result"))?;
            match length {
                None => length = Some(live),
                Some(first) => {
                    let differ = self.builder.ins().icmp(IntCC::NotEqual, first, live);
                    self.trap_if(differ, mismatch_message(lift))?;
                }
            }
        }
        let Some(length) = length else {
            return Err(Decline::new("list lift without a list operand"));
        };
        let result = self
            .call_runtime(&abi::LIST_NEW, &[])?
            .ok_or_else(|| Decline::new("list new result"))?;
        let header = self.builder.create_block();
        self.builder.append_block_param(header, types::I64);
        let body = self.builder.create_block();
        let exit = self.builder.create_block();
        let start = self.builder.ins().iconst(types::I64, 0);
        self.builder.ins().jump(header, &[BlockArg::Value(start)]);
        self.builder.switch_to_block(header);
        let index = self.builder.block_params(header)[0];
        let more = self
            .builder
            .ins()
            .icmp(IntCC::SignedLessThan, index, length);
        self.builder.ins().brif(more, body, &[], exit, &[]);
        self.builder.switch_to_block(body);
        let mut picked = Vec::with_capacity(arity);
        for operand in &operands {
            picked.push(if operand.list {
                let bits = self
                    .call_runtime(&abi::LIST_GET, &[operand.value, index])?
                    .ok_or_else(|| Decline::new("list get result"))?;
                (self.element_value(bits, operand.rep), operand.rep)
            } else {
                (operand.value, operand.rep)
            });
        }
        let (computed, computed_rep) = self.lift_element(lift, &picked)?;
        let stored = self.lift_store(computed, computed_rep, result_element, result_rep)?;
        let tag = self
            .builder
            .ins()
            .iconst(types::I32, Self::element_tag(result_rep)?);
        let bits = self.element_bits(stored, result_rep);
        self.call_runtime(&abi::LIST_PUSH, &[result, tag, bits])?;
        let next = self.builder.ins().iadd_imm_s(index, 1);
        self.builder.ins().jump(header, &[BlockArg::Value(next)]);
        self.builder.switch_to_block(exit);
        self.define_result(destination, Some(result), Rep::List)
    }

    /// One element: the scalar operator's own semantics.
    fn lift_element(&mut self, lift: Lift, picked: &[(Value, Rep)]) -> Lowered<(Value, Rep)> {
        match (lift, picked) {
            (Lift::Negate, [(value, Rep::Int)]) => {
                let zero = self.builder.ins().iconst(types::I64, 0);
                Ok((self.checked_sub(zero, *value)?, Rep::Int))
            }
            (Lift::Negate, [(value, Rep::Float)]) => {
                Ok((self.builder.ins().fneg(*value), Rep::Float))
            }
            (Lift::Not, [(value, Rep::Bool)]) => {
                let one = self.builder.ins().iconst(types::I8, 1);
                Ok((self.builder.ins().bxor(*value, one), Rep::Bool))
            }
            (Lift::TrueDiv, [(a, a_rep), (b, b_rep)]) => {
                if *a_rep == Rep::Int && *b_rep == Rep::Int {
                    let zero = self.builder.ins().iconst(types::I64, 0);
                    let is_zero = self.builder.ins().icmp(IntCC::Equal, *b, zero);
                    self.trap_if(is_zero, "numerus division failed")?;
                }
                let a = self.lift_float(*a, *a_rep)?;
                let b = self.lift_float(*b, *b_rep)?;
                Ok((self.builder.ins().fdiv(a, b), Rep::Float))
            }
            (Lift::Binary(MirBinOp::And), [(a, Rep::Bool), (b, Rep::Bool)]) => {
                Ok((self.builder.ins().band(*a, *b), Rep::Bool))
            }
            (Lift::Binary(MirBinOp::Or), [(a, Rep::Bool), (b, Rep::Bool)]) => {
                Ok((self.builder.ins().bor(*a, *b), Rep::Bool))
            }
            (
                Lift::Binary(op @ (MirBinOp::Eq | MirBinOp::NotEq)),
                [(a, Rep::Bool), (b, Rep::Bool)],
            ) => {
                let cc = if op == MirBinOp::Eq {
                    IntCC::Equal
                } else {
                    IntCC::NotEqual
                };
                Ok((self.builder.ins().icmp(cc, *a, *b), Rep::Bool))
            }
            (Lift::Binary(op), [(a, Rep::Int), (b, Rep::Int)]) => {
                self.int_binary(op, *a, *b, false)
            }
            (Lift::Binary(op), [(a, a_rep), (b, b_rep)])
                if matches!(a_rep, Rep::Int | Rep::Float)
                    && matches!(b_rep, Rep::Int | Rep::Float) =>
            {
                let a = self.lift_float(*a, *a_rep)?;
                let b = self.lift_float(*b, *b_rep)?;
                if op == MirBinOp::Mod {
                    // The floor remainder, as the scalar operator.
                    return Ok((self.float_floor_rem(a, b)?, Rep::Float));
                }
                // The operands arrive from typed list cells and the result
                // rounds at `lift_store`'s element cell, so the per-cell
                // width needs no operand rounding here.
                self.float_binary(op, a, b, false)
            }
            _ => Err(Decline::new("list lift operand representations")),
        }
    }

    fn lift_float(&mut self, value: Value, rep: Rep) -> Lowered<Value> {
        match rep {
            Rep::Float => Ok(value),
            Rep::Int => Ok(self.builder.ins().fcvt_from_sint(types::F64, value)),
            _ => Err(Decline::new("list lift float operand")),
        }
    }

    /// The element store: the result element type's slot policy.
    fn lift_store(
        &mut self,
        value: Value,
        have: Rep,
        element: MirType,
        want: Rep,
    ) -> Lowered<Value> {
        if have != want {
            return Err(Decline::new("list lift result representation"));
        }
        match want {
            Rep::Int => {
                let site = radix_hir::HirStoreSite {
                    kind: radix_hir::HirStoreKind::Element,
                    slot: None,
                    inferred_from: None,
                    inferred_text: None,
                };
                self.int_store(&site, element, value)
            }
            Rep::Float => {
                let narrow = matches!(
                    resolve(self.state.input.types, element.semantic_id()),
                    Type::SizedNumeric(Primitive::Fractus, width)
                        if width.is_float() && *width != NumericWidth::F64
                );
                if narrow {
                    let reduced = self.builder.ins().fdemote(types::F32, value);
                    Ok(self.builder.ins().fpromote(types::F64, reduced))
                } else {
                    Ok(value)
                }
            }
            _ => Ok(value),
        }
    }
}

#[cfg(test)]
#[path = "lift_test.rs"]
mod lift_test;
