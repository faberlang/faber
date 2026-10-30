//! Sets (goal unit C5): `set<int>` and friends are the list storage under the
//! set contract, so a set value is a `faber_list_<elem> *` and every bind
//! aliases (R1). `add` keeps the element unique (`faber_set_<e>_add`),
//! `delete` removes by value, and the algebra returns a new set. Element
//! order is unspecified, as the runner's (D19.2).

use crate::Emitter;
use crate::error::unsupported;
use crate::expr::{Operand, Value, wrap};
use crate::helpers::{Helper, ListOp};
use crate::ty::{Elem, Ty};
use radix_codegen_shared::CodegenError;
use radix_hir::{HirCallArg, HirExpression};
use radix_intrinsics::IntrinsicMirKind;
use radix_lexer::Symbol;
use radix_types::MirCollectionOp;

impl Emitter<'_> {
    /// `empty`: a new set with no storage yet.
    pub(crate) fn empty_set(&mut self, elem: Elem) -> Result<Operand, CodegenError> {
        self.need_class(Ty::Set(elem));
        self.support.need(Helper::List(elem, ListOp::New));
        Ok(Operand::new(
            format!("faber_list_{}_new(faber_a, 0)", self.elem_suffix(elem)?),
            Value::Set(elem),
        ))
    }

    /// The methods this target has for a set: the count, `add`, `has`,
    /// `delete`, and the algebra (`union`, `intersection`, `difference`,
    /// `difference_symmetric`, `is_subset`, `is_superset`).
    pub(crate) fn set_method(
        &mut self,
        receiver: &HirExpression,
        method: Symbol,
        args: &[HirCallArg],
    ) -> Result<Operand, CodegenError> {
        let name = self.names.resolve_symbol(method).to_owned();
        let Some(receiver_ty) = receiver.ty else {
            return Err(unsupported(
                "method call on a value without a resolved type",
            ));
        };
        let Some(entry) =
            radix_intrinsics::lookup_method_for_type_id(receiver_ty, self.types, &name, args.len())
        else {
            return Err(unsupported("method call"));
        };
        let set = self.expr(receiver)?;
        let Value::Set(elem) = set.value else {
            return Err(unsupported("method call on a value that is not a set"));
        };
        if args
            .iter()
            .any(|arg| arg.name.is_some() || arg.spread || arg.marker.is_some())
        {
            return Err(unsupported("named, spread, or marked argument"));
        }
        match entry.mir_kind {
            // `longitudo` / `vacua` read the list storage's count directly, so
            // the result is not a call: only the receiver's own effect carries.
            IntrinsicMirKind::Collection(MirCollectionOp::Length) => {
                let text = format!("({})->len", set.text);
                let narrow = (!set.effect).then(|| crate::expr::Narrow::leaf(&text));
                let mut operand = Operand::new(text, Value::Int).with_effect(set.effect);
                operand.narrow = narrow;
                Ok(operand)
            }
            IntrinsicMirKind::Collection(MirCollectionOp::IsEmpty) => Ok(Operand::new(
                format!("(({})->len == 0)", set.text),
                Value::Truth,
            )
            .with_effect(set.effect)),
            IntrinsicMirKind::Collection(MirCollectionOp::Append) => {
                let item = self.element(one_arg(args, "set add")?, elem)?;
                let call = self.set_call("add", elem, set, item)?;
                Ok(Operand::new(call, Value::Void).with_effect(true))
            }
            IntrinsicMirKind::Collection(MirCollectionOp::Contains) => {
                let item = self.element(one_arg(args, "set has")?, elem)?;
                let call = self.set_call("contains", elem, set, item)?;
                Ok(Operand::new(call, Value::Truth).with_effect(true))
            }
            IntrinsicMirKind::Collection(MirCollectionOp::Delete) => {
                let item = self.element(one_arg(args, "set delete")?, elem)?;
                let call = self.set_call("delete", elem, set, item)?;
                Ok(Operand::new(call, Value::Truth).with_effect(true))
            }
            IntrinsicMirKind::Collection(op)
                if matches!(
                    op,
                    MirCollectionOp::Union
                        | MirCollectionOp::Intersection
                        | MirCollectionOp::Difference
                        | MirCollectionOp::SymmetricDifference
                ) =>
            {
                self.set_algebra(op, elem, set, args)
            }
            IntrinsicMirKind::Collection(
                op @ (MirCollectionOp::IsSubset | MirCollectionOp::IsSuperset),
            ) => self.set_containment(op, elem, set, args),
            _ => Err(unsupported("set method outside the subset")),
        }
    }

    /// A one-argument set operation: the receiver after the arena and a value
    /// already checked against the element class.
    fn set_call(
        &mut self,
        name: &str,
        elem: Elem,
        set: Operand,
        item: Operand,
    ) -> Result<String, CodegenError> {
        self.support.need(Helper::List(elem, ListOp::New));
        let mut operands = [set, item];
        let prelude = self.sequence(&mut operands)?;
        let [set, item] = operands;
        // `contains` is the set family's indexed read; `add` allocates and
        // takes the arena; `delete` fixes its index in place. A truth result
        // is already parenthesized.
        let call = match name {
            "contains" => format!(
                "(faber_set_{}_has({}, {}))",
                self.elem_suffix(elem)?,
                set.text,
                item.text
            ),
            "add" => format!(
                "(faber_set_{}_add(faber_a, {}, {}))",
                self.elem_suffix(elem)?,
                set.text,
                item.text
            ),
            _ => format!(
                "(faber_set_{}_delete({}, {}))",
                self.elem_suffix(elem)?,
                set.text,
                item.text
            ),
        };
        Ok(wrap(&prelude, &call))
    }

    /// `union`, `intersection`, `difference`, `difference_symmetric`: a value
    /// operation over two sets that returns a new set.
    fn set_algebra(
        &mut self,
        op: MirCollectionOp,
        elem: Elem,
        set: Operand,
        args: &[HirCallArg],
    ) -> Result<Operand, CodegenError> {
        let (other, other_elem) = self.set_operand(args, "set algebra")?;
        if other_elem != elem {
            return Err(unsupported(
                "set algebra with a set of another element class",
            ));
        }
        let mut operands = [set, other];
        let prelude = self.sequence(&mut operands)?;
        let [set, other] = operands;
        self.need_class(Ty::Set(elem));
        let name =
            set_algebra_name(op).ok_or_else(|| unsupported("set method outside the subset"))?;
        let call = format!(
            "faber_set_{}_{name}(faber_a, {}, {})",
            self.elem_suffix(elem)?,
            set.text,
            other.text
        );
        Ok(Operand::new(wrap(&prelude, &call), Value::Set(elem)).with_effect(true))
    }

    /// `is_subset` / `is_superset`: one `bool` over two sets.
    fn set_containment(
        &mut self,
        op: MirCollectionOp,
        elem: Elem,
        set: Operand,
        args: &[HirCallArg],
    ) -> Result<Operand, CodegenError> {
        let (other, other_elem) = self.set_operand(args, "set containment")?;
        if other_elem != elem {
            return Err(unsupported(
                "set containment with a set of another element class",
            ));
        }
        let mut operands = [set, other];
        let prelude = self.sequence(&mut operands)?;
        let [set, other] = operands;
        // `is_superset(a, b)` is `is_subset(b, a)`.
        let (left, right) = if op == MirCollectionOp::IsSubset {
            (set.text, other.text)
        } else {
            (other.text, set.text)
        };
        let call = format!(
            "(faber_set_{}_is_subset({left}, {right}))",
            self.elem_suffix(elem)?
        );
        Ok(Operand::new(wrap(&prelude, &call), Value::Truth).with_effect(true))
    }

    /// The single set argument of `args`, evaluated to its element class.
    fn set_operand(
        &mut self,
        args: &[HirCallArg],
        what: &'static str,
    ) -> Result<(Operand, Elem), CodegenError> {
        let arg = one_arg(args, what)?;
        let other = self.expr(arg)?;
        match other.value {
            Value::Set(elem) => Ok((other, elem)),
            _ => Err(unsupported("set operation with a value that is not a set")),
        }
    }
}

/// The single argument of `args`, or a named rejection.
fn one_arg<'a>(
    args: &'a [HirCallArg],
    what: &'static str,
) -> Result<&'a HirExpression, CodegenError> {
    match args {
        [arg] => Ok(&arg.expr),
        _ => Err(unsupported(what)),
    }
}

/// The C helper suffix of a set-algebra op, `None` for the other collection
/// ops.
fn set_algebra_name(op: MirCollectionOp) -> Option<&'static str> {
    match op {
        MirCollectionOp::Union => Some("union"),
        MirCollectionOp::Intersection => Some("intersection"),
        MirCollectionOp::Difference => Some("difference"),
        MirCollectionOp::SymmetricDifference => Some("difference_symmetric"),
        _ => None,
    }
}
