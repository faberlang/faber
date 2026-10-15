//! Lists (goal unit C5): `list<int>`, `list<string>` and `list<bool>` as
//! pointers to arena-allocated `faber_list_<elem>` values, so every bind
//! aliases (semantics R1). Methods resolve through the shared receiver
//! intrinsic catalog, by what they do, not by their spelling.

use crate::Emitter;
use crate::error::unsupported;
use crate::expr::{Operand, Value, wrap};
use crate::helpers::{Helper, ListOp};
use crate::ty::{Elem, Ty, classify};
use radix_codegen_shared::CodegenError;
use radix_hir::{HirArrayElement, HirCallArg, HirExpression};
use radix_intrinsics::IntrinsicMirKind;
use radix_lexer::Symbol;
use radix_types::MirCollectionOp;

impl Emitter<'_> {
    /// The list class of an expression's own type.
    fn list_elem(&self, expr: &HirExpression) -> Result<Elem, CodegenError> {
        let Some(ty) = expr.ty else {
            return Err(unsupported("list without a resolved type"));
        };
        match classify(self.types, ty)? {
            Ty::List(elem) => Ok(elem),
            _ => Err(unsupported("collection that is not a list")),
        }
    }

    /// The capacity of a bounded list type (`list<int, 8>`), peeling aliases.
    /// `None` for an unbounded list, or a capacity not known at compile time.
    fn list_capacity(&self, ty: radix_types::TypeId) -> Option<u64> {
        match self.types.get(ty) {
            radix_types::Type::Alias(_, inner) => self.list_capacity(*inner),
            radix_types::Type::BoundedArray { capacity, .. } => {
                match self.types.get_index(*capacity) {
                    radix_types::IndexExpr::Literal(value) => Some(*value),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Whether a type is a bounded list (`list<T, N>`), peeling aliases.
    pub(crate) fn is_bounded_type(&self, ty: radix_types::TypeId) -> bool {
        match self.types.get(ty) {
            radix_types::Type::Alias(_, inner) => self.is_bounded_type(*inner),
            radix_types::Type::BoundedArray { .. } => true,
            _ => false,
        }
    }

    /// Whether an expression's value is a bounded list the runtime must mark
    /// `fixed`: its type says so, and no ascription to `list<T>` is in force.
    fn is_fixed(&self, expr: &HirExpression) -> bool {
        !self.unbounded_literals && expr.ty.is_some_and(|ty| self.is_bounded_type(ty))
    }

    fn list_operand(&mut self, text: String, elem: Elem) -> Operand {
        self.need_class(Ty::List(elem));
        Operand::new(text, Value::List(elem))
    }

    /// `empty`: a new list with no storage yet.
    pub(crate) fn empty_list(&mut self, expr: &HirExpression) -> Result<Operand, CodegenError> {
        let elem = self.list_elem(expr)?;
        self.support.need(Helper::List(elem, ListOp::New));
        let text = if self.is_fixed(expr) {
            let capacity = expr.ty.and_then(|ty| self.list_capacity(ty));
            let Some(capacity) = capacity else {
                return Err(unsupported("bounded list with a capacity not known here"));
            };
            format!(
                "faber_list_{}_new_fixed(faber_a, {capacity})",
                elem.suffix()
            )
        } else {
            format!("faber_list_{}_new(faber_a, 0)", elem.suffix())
        };
        Ok(self.list_operand(text, elem))
    }

    /// `[a, b, c]`: the elements, evaluated in order, copied in one call.
    pub(crate) fn list_literal(
        &mut self,
        expr: &HirExpression,
        elements: &[HirArrayElement],
    ) -> Result<Operand, CodegenError> {
        let elem = self.list_elem(expr)?;
        if elements.is_empty() {
            return self.empty_list(expr);
        }
        let mut operands = Vec::with_capacity(elements.len());
        for element in elements {
            let HirArrayElement::Expr(value) = element else {
                return Err(unsupported("spread or inferred list element"));
            };
            let operand = self.element(value, elem)?;
            operands.push(operand);
        }
        let effect = operands.iter().any(|operand| operand.effect);
        let prelude = self.sequence(&mut operands);
        let items = operands
            .iter()
            .map(|operand| operand.text.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        self.support.need(Helper::List(elem, ListOp::Of));
        let of = if self.is_fixed(expr) {
            "of_fixed"
        } else {
            "of"
        };
        let call = format!(
            "faber_list_{n}_{of}(faber_a, {count}, ({t}[]){{{items}}})",
            n = elem.suffix(),
            count = operands.len(),
            t = elem.c_type(),
        );
        let operand = self.list_operand(wrap(&prelude, &call), elem);
        Ok(operand.with_effect(effect))
    }

    /// One element value of class `elem`: an exact value that arrives with
    /// no store node narrows like any unwrapped store.
    fn element(&mut self, value: &HirExpression, elem: Elem) -> Result<Operand, CodegenError> {
        let operand = self.expr(value)?;
        let operand = if elem == Elem::Int {
            self.fit(operand)?
        } else {
            operand
        };
        if operand.value != Value::from(elem.ty()) {
            return Err(unsupported("value stored without a store check"));
        }
        Ok(operand)
    }

    /// `xs[i]`: a checked read.
    pub(crate) fn list_read(
        &mut self,
        base: &HirExpression,
        index: &HirExpression,
    ) -> Result<Operand, CodegenError> {
        let list = self.expr(base)?;
        if list.value == Value::Text {
            return self.text_index(list, index);
        }
        let Value::List(elem) = list.value else {
            return Err(unsupported("index access on a value that is not a list"));
        };
        let position = self.expr(index)?;
        if !matches!(position.value, Value::Int | Value::Exact) {
            return Err(unsupported("list index that is not an integer"));
        }
        let effect = list.effect || position.effect;
        let mut operands = [list, position];
        let prelude = self.sequence(&mut operands);
        let [list, position] = operands;
        self.support.need(Helper::List(elem, ListOp::At));
        let call = format!(
            "faber_list_{}_at({}, {})",
            elem.suffix(),
            list.text,
            position.text
        );
        let text = wrap(&prelude, &call);
        // A boolean element is a truth operand and carries its parentheses.
        let text = if elem == Elem::Bool {
            format!("({text})")
        } else {
            text
        };
        Ok(Operand::new(text, Value::from(elem.ty())).with_effect(effect))
    }

    /// `xs[i] ← value` as a statement line.
    pub(crate) fn list_write(
        &mut self,
        base: &HirExpression,
        index: &HirExpression,
        value: &HirExpression,
    ) -> Result<String, CodegenError> {
        let list = self.expr(base)?;
        let Value::List(elem) = list.value else {
            return Err(unsupported(
                "indexed assignment on a value that is not a list",
            ));
        };
        let position = self.expr(index)?;
        if !matches!(position.value, Value::Int | Value::Exact) {
            return Err(unsupported("list index that is not an integer"));
        }
        let item = self.element(value, elem)?;
        // Which of the place and the value Faber evaluates first is not
        // pinned for this target yet; two sides with calls are rejected
        // rather than guessed.
        if [&list, &position, &item]
            .iter()
            .filter(|operand| operand.effect)
            .count()
            > 1
        {
            return Err(unsupported(
                "indexed assignment with calls on more than one side",
            ));
        }
        self.support.need(Helper::List(elem, ListOp::Set));
        Ok(format!(
            "faber_list_{}_set({}, {}, {});",
            elem.suffix(),
            list.text,
            position.text,
            item.text
        ))
    }

    /// A method on a list, resolved through the receiver intrinsic catalog.
    pub(crate) fn list_method(
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
        let list = self.expr(receiver)?;
        let Value::List(elem) = list.value else {
            return Err(unsupported("method call on a value that is not a list"));
        };
        if args
            .iter()
            .any(|arg| arg.name.is_some() || arg.spread || arg.marker.is_some())
        {
            return Err(unsupported("named, spread, or marked argument"));
        }
        match entry.mir_kind {
            IntrinsicMirKind::Collection(MirCollectionOp::Length) => {
                let text = format!("({})->len", list.text);
                // A pure length read is a 64-bit fast-path leaf (C4).
                let narrow = (!list.effect).then(|| crate::expr::Narrow::leaf(&text));
                let mut operand = Operand::new(text, Value::Int).with_effect(list.effect);
                operand.narrow = narrow;
                Ok(operand)
            }
            IntrinsicMirKind::Collection(MirCollectionOp::IsEmpty) => Ok(Operand::new(
                format!("(({})->len == 0)", list.text),
                Value::Truth,
            )
            .with_effect(list.effect)),
            IntrinsicMirKind::Collection(MirCollectionOp::Append) => {
                let [arg] = args else {
                    return Err(unsupported("list append without one value"));
                };
                let item = self.element(&arg.expr, elem)?;
                let effect = list.effect || item.effect;
                let mut operands = [list, item];
                let prelude = self.sequence(&mut operands);
                let [list, item] = operands;
                self.support.need(Helper::List(elem, ListOp::Push));
                let call = format!(
                    "faber_list_{}_push(faber_a, {}, {})",
                    elem.suffix(),
                    list.text,
                    item.text
                );
                Ok(Operand::new(wrap(&prelude, &call), Value::Void).with_effect(effect))
            }
            _ => Err(unsupported("list method outside the subset")),
        }
    }

    /// How `print` and a template hole show a list, as the runner does.
    pub(crate) fn show_list(&mut self, text: &str, elem: Elem) -> Result<String, CodegenError> {
        self.support.need(Helper::List(elem, ListOp::Show));
        if elem != Elem::Bool {
            return Ok(format!(
                "faber_list_{}_show(faber_a, {text})",
                elem.suffix()
            ));
        }
        let tokens = self.names.display_tokens();
        let (yes, no) = (
            crate::expr::c_string(&tokens.true_)?,
            crate::expr::c_string(&tokens.false_)?,
        );
        Ok(format!(
            "faber_list_bool_show(faber_a, {text}, {yes}, {no})"
        ))
    }
}
