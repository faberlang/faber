//! Lists (goal unit C5): `list<int>`, `list<string>` and `list<bool>` as
//! pointers to arena-allocated `faber_list_<elem>` values, so every bind
//! aliases (semantics R1). Methods resolve through the shared receiver
//! intrinsic catalog, by what they do, not by their spelling.

use crate::Emitter;
use crate::error::unsupported;
use crate::expr::{Operand, Value, wrap};
use crate::helpers::{Helper, ListOp};
use crate::ty::{Elem, Inner, Ty, classify};
use radix_codegen_shared::CodegenError;
use radix_hir::{HirArrayElement, HirCallArg, HirExpression};
use radix_intrinsics::{IntrinsicMirKind, IntrinsicReturnKind};
use radix_lexer::Symbol;
use radix_types::MirCollectionOp;
use std::fmt::Write as _;

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

    /// The capacity of a bounded list type (`list<int, size 8>`), peeling aliases.
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

    /// Whether a type is a bounded list (`list<T, size N>`), peeling aliases.
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
        if elements
            .iter()
            .any(|element| matches!(element, HirArrayElement::Spread(_)))
        {
            return self.list_spread_literal(elements, elem);
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

    /// A list literal carrying a `spread`: the length is known only at run
    /// time, so the list is built by pushing each element (a spread list
    /// appends all of its elements) in source order into one statement
    /// expression. The result is growable, as the runner's is.
    fn list_spread_literal(
        &mut self,
        elements: &[HirArrayElement],
        elem: Elem,
    ) -> Result<Operand, CodegenError> {
        let suffix = elem.suffix();
        self.support.need(Helper::List(elem, ListOp::New));
        self.support.need(Helper::List(elem, ListOp::Push));
        let target = format!("faber_t{}", self.temps);
        self.temps += 1;
        let mut body =
            format!("faber_list_{suffix} *{target} = faber_list_{suffix}_new(faber_a, 0); ");
        for element in elements {
            match element {
                HirArrayElement::Expr(value) => {
                    let item = self.element(value, elem)?;
                    write!(
                        body,
                        "faber_list_{suffix}_push(faber_a, {target}, {}); ",
                        item.text
                    )
                    .ok();
                }
                HirArrayElement::Spread(value) => {
                    let list = self.expr(value)?;
                    let Value::List(inner) = list.value else {
                        return Err(unsupported("spread of a value that is not a list"));
                    };
                    if inner != elem {
                        return Err(unsupported("spread of a list of another element class"));
                    }
                    write!(
                        body,
                        "faber_list_{suffix}_push_all(faber_a, {target}, {}); ",
                        list.text
                    )
                    .ok();
                }
                HirArrayElement::Infer(_) => {
                    return Err(unsupported("spread or inferred list element"));
                }
            }
        }
        let text = format!("(__extension__ ({{ {body}{target}; }}))");
        Ok(self.list_operand(text, elem).with_effect(true))
    }

    /// One element value of class `elem`: an exact value that arrives with
    /// no store node narrows like any unwrapped store.
    pub(crate) fn element(
        &mut self,
        value: &HirExpression,
        elem: Elem,
    ) -> Result<Operand, CodegenError> {
        let operand = self.expr(value)?;
        let operand = match elem {
            Elem::Int => self.fit(operand)?,
            // A modular word reduces modulo 2^W at the element store, exactly
            // as a scalar store does; a value that already carries the class
            // is left alone.
            Elem::Modular(width) => self.settle(operand, Ty::Modular(width))?,
            // A raw-bits literal may still be exact; settle it into the
            // unsigned carrier at its declared width before the list store.
            Elem::Bits(width) => self.settle(operand, Ty::Bits(width))?,
            _ => operand,
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
        Ok(self.list_at_operands(list, position, elem))
    }

    /// The checked element read from already-evaluated list and position
    /// operands. Shared by the plain read and the `x![i]` assertion read.
    pub(crate) fn list_at_operands(
        &mut self,
        list: Operand,
        position: Operand,
        elem: Elem,
    ) -> Operand {
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
        Operand::new(text, Value::from(elem.ty())).with_effect(effect)
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
        result_ty: Option<radix_types::TypeId>,
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
            IntrinsicMirKind::HigherOrderAll
            | IntrinsicMirKind::HigherOrderAny
            | IntrinsicMirKind::HigherOrderFiltrata
            | IntrinsicMirKind::HigherOrderMap
            | IntrinsicMirKind::HigherOrderReduce
            | IntrinsicMirKind::HigherOrderCumulata => {
                self.list_higher_order(list, elem, entry.mir_kind, result_ty, args)
            }
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
            IntrinsicMirKind::Collection(op) => self.list_query(list, elem, op, entry, args),
            // `ordina`: the in-place counterpart of `ordinata` (`Sort`), which
            // the catalog marks codegen-only (it has no MIR kind).
            _ if name == "ordina" && args.is_empty() => {
                self.support.need(Helper::List(elem, ListOp::At));
                Ok(Operand::new(
                    format!("faber_list_{}_sort({})", elem.suffix(), list.text),
                    Value::Void,
                )
                .with_effect(true))
            }
            _ => Err(unsupported("list method outside the subset")),
        }
    }

    /// A higher-order list method — `filter`, `map`, `reduce`, `cumulate`,
    /// `all`, `any` — over a non-capturing predicate or reducer. The body is
    /// one GNU statement expression holding a `for` over the list's items;
    /// each item is passed to the function pointer (with the hidden arena
    /// when the program allocates). `reduce`/`cumulate` carry the accumulator
    /// through the loop; `all`/`any` short-circuit.
    #[expect(clippy::too_many_lines)]
    fn list_higher_order(
        &mut self,
        list: Operand,
        elem: Elem,
        op: IntrinsicMirKind,
        result_ty: Option<radix_types::TypeId>,
        args: &[HirCallArg],
    ) -> Result<Operand, CodegenError> {
        let list_text = list.text;
        let suffix = elem.suffix();
        let list_ptr = elem.list_c_pointer();
        let source = elem.c_type();
        let arena = if self.arena { "faber_a, " } else { "" };
        if args.is_empty() || args.len() > 2 {
            return Err(unsupported("higher-order list method with the wrong arity"));
        }
        for arg in args {
            if arg.name.is_some() || arg.spread || arg.marker.is_some() {
                return Err(unsupported("named, spread, or marked argument"));
            }
        }
        let function = self.expr(&args[0].expr)?;
        let Value::Func(id) = function.value else {
            return Err(unsupported(
                "higher-order argument that is not a function value",
            ));
        };
        let params = match self.types.get(radix_types::TypeId(id)) {
            radix_types::Type::Func(sig) => sig.params.len(),
            _ => return Err(unsupported("higher-order argument without a signature")),
        };
        let call_result = result_ty.map(|ty| classify(self.types, ty)).transpose()?;
        self.support.need_ints();
        let (result_class, text) = match op {
            IntrinsicMirKind::HigherOrderAll | IntrinsicMirKind::HigherOrderAny => {
                if params != 1 {
                    return Err(unsupported("predicate that does not take one argument"));
                }
                let (init, set, negate) = if op == IntrinsicMirKind::HigherOrderAll {
                    ("true", "false", "!")
                } else {
                    ("false", "true", "")
                };
                let call = format!("{}({arena}faber_ho_e)", function.text);
                let text = format!(
                    "(__extension__ ({{ bool faber_ho_r = {init}; {list_ptr} faber_ho_src = {list_text}; \
                     for (int64_t faber_ho_i = 0; faber_ho_i < (faber_ho_src)->len; faber_ho_i++) {{ \
                     {source} faber_ho_e = (faber_ho_src)->items[faber_ho_i]; \
                     if ({negate}{call}) {{ faber_ho_r = {set}; break; }} }} faber_ho_r; }}))"
                );
                (Ty::Bool, text)
            }
            IntrinsicMirKind::HigherOrderFiltrata => {
                if params != 1 {
                    return Err(unsupported("predicate that does not take one argument"));
                }
                let call = format!("{}({arena}faber_ho_e)", function.text);
                self.support.need(Helper::List(elem, ListOp::New));
                self.support.need(Helper::List(elem, ListOp::Push));
                let text = format!(
                    "(__extension__ ({{ {list_ptr} faber_ho_src = {list_text}; {list_ptr} faber_ho_out = faber_list_{suffix}_new(faber_a, 0); \
                     for (int64_t faber_ho_i = 0; faber_ho_i < (faber_ho_src)->len; faber_ho_i++) {{ \
                     {source} faber_ho_e = (faber_ho_src)->items[faber_ho_i]; \
                     if ({call}) {{ faber_list_{suffix}_push(faber_a, faber_ho_out, faber_ho_e); }} }} faber_ho_out; }}))"
                );
                (Ty::List(elem), text)
            }
            IntrinsicMirKind::HigherOrderMap => {
                if params != 1 {
                    return Err(unsupported("mapper that does not take one argument"));
                }
                let Some(Ty::List(mapped)) = call_result else {
                    return Err(unsupported("mapper result that is not a list"));
                };
                let mapped_suffix = mapped.suffix();
                let mapped_ptr = mapped.list_c_pointer();
                let mapped_source = mapped.c_type();
                let call = format!(
                    "{}({arena}(faber_ho_src)->items[faber_ho_i])",
                    function.text
                );
                self.support.need(Helper::List(mapped, ListOp::New));
                self.support.need(Helper::List(mapped, ListOp::Push));
                let text = format!(
                    "(__extension__ ({{ {list_ptr} faber_ho_src = {list_text}; {mapped_ptr} faber_ho_out = faber_list_{mapped_suffix}_new(faber_a, 0); \
                     for (int64_t faber_ho_i = 0; faber_ho_i < (faber_ho_src)->len; faber_ho_i++) {{ \
                     {mapped_source} faber_ho_e = {call}; \
                     faber_list_{mapped_suffix}_push(faber_a, faber_ho_out, faber_ho_e); }} faber_ho_out; }}))"
                );
                (Ty::List(mapped), text)
            }
            IntrinsicMirKind::HigherOrderReduce | IntrinsicMirKind::HigherOrderCumulata => {
                if params != 2 || args.len() != 2 {
                    return Err(unsupported("reducer that does not take two arguments"));
                }
                let cumulate = op == IntrinsicMirKind::HigherOrderCumulata;
                // `reduce` yields the accumulator itself; `cumulate` yields a
                // new list of the running accumulator, so its result type is
                // `list<A>` and its accumulator element class is `A`.
                let (a_class, result_class) = match call_result {
                    Some(Ty::List(element)) if cumulate => (element.ty(), Ty::List(element)),
                    Some(class) if !cumulate => (class, class),
                    _ => return Err(unsupported("reduction result without a class")),
                };
                let a_source = self.c_type(a_class)?;
                let initial = self.expr(&args[1].expr)?;
                let accum = self.settle(initial, a_class)?;
                let call = format!(
                    "{}({arena}faber_ho_acc, (faber_ho_src)->items[faber_ho_i])",
                    function.text
                );
                let text = if cumulate {
                    let Ty::List(element) = result_class else {
                        return Err(unsupported("cumulate result that is not a list"));
                    };
                    let element_suffix = element.suffix();
                    let element_ptr = element.list_c_pointer();
                    self.support.need(Helper::List(element, ListOp::New));
                    self.support.need(Helper::List(element, ListOp::Push));
                    format!(
                        "(__extension__ ({{ {list_ptr} faber_ho_src = {list_text}; {a_source} faber_ho_acc = {}; {element_ptr} faber_ho_out = faber_list_{element_suffix}_new(faber_a, 0); \
                         for (int64_t faber_ho_i = 0; faber_ho_i < (faber_ho_src)->len; faber_ho_i++) {{ \
                         faber_ho_acc = {call}; faber_list_{element_suffix}_push(faber_a, faber_ho_out, faber_ho_acc); }} faber_ho_out; }}))",
                        accum.text
                    )
                } else {
                    format!(
                        "(__extension__ ({{ {list_ptr} faber_ho_src = {list_text}; {a_source} faber_ho_acc = {}; \
                         for (int64_t faber_ho_i = 0; faber_ho_i < (faber_ho_src)->len; faber_ho_i++) {{ \
                         faber_ho_acc = {call}; }} faber_ho_acc; }}))",
                        accum.text
                    )
                };
                (result_class, text)
            }
            _ => return Err(unsupported("list method outside the subset")),
        };
        self.need_class(result_class);
        Ok(Operand::new(text, Value::from(result_class)).with_effect(true))
    }

    /// `sort via asc|desc ex xs` (values) or `de m` (a map's keys): a new
    /// list, ordered by value. Equal elements are identical, so stability
    /// cannot show; floats order by IEEE totalOrder, text by code point.
    pub(crate) fn sort_values(
        &mut self,
        direction: radix_hir::HirSortDirection,
        source: &HirExpression,
        keys: bool,
    ) -> Result<Operand, CodegenError> {
        let list = self.expr(source)?;
        // `sort … ex m` orders the map's values and `de m` its keys, as the
        // runner does; the side's list is materialized first.
        let (list_text, elem) = match list.value {
            Value::List(elem) | Value::Set(elem) => (list.text.clone(), elem),
            Value::Map(k, v) => {
                let (side, elem) = if keys { ("keys", k) } else { ("values", v) };
                self.support.need(Helper::List(elem, ListOp::New));
                (
                    format!(
                        "faber_map_{}_{side}(faber_a, {})",
                        Self::map_suffix(k, v),
                        list.text
                    ),
                    elem,
                )
            }
            _ => return Err(unsupported("sort of a value that is not a list")),
        };
        self.support.need(Helper::List(elem, ListOp::New));
        let sorted = format!("faber_list_{}_sorted(faber_a, {list_text})", elem.suffix());
        let text = match direction {
            radix_hir::HirSortDirection::Ascending => sorted,
            radix_hir::HirSortDirection::Descending => {
                format!("faber_list_{}_inverted(faber_a, {sorted})", elem.suffix())
            }
        };
        Ok(self.list_operand(text, elem).with_effect(list.effect))
    }

    /// The rest of the list methods this target has: reads of an end or an
    /// element (an optional), membership, copies, ordering, the ends removed.
    fn list_query(
        &mut self,
        list: Operand,
        elem: Elem,
        op: MirCollectionOp,
        entry: &radix_intrinsics::MethodIntrinsic,
        args: &[HirCallArg],
    ) -> Result<Operand, CodegenError> {
        let inner = Inner::from_ty(elem.ty())
            .ok_or_else(|| unsupported("list element without an optional form"))?;
        // An element argument or an index argument, evaluated after the list.
        let mut operands = vec![list];
        match (op, args) {
            (MirCollectionOp::Contains | MirCollectionOp::AppendImmutable, [arg]) => {
                operands.push(self.element(&arg.expr, elem)?);
            }
            (
                MirCollectionOp::Index
                | MirCollectionOp::Take
                | MirCollectionOp::TakeLast
                | MirCollectionOp::DropFirst,
                [arg],
            ) => {
                let index = self.expr(&arg.expr)?;
                if !matches!(index.value, Value::Int | Value::Exact) {
                    return Err(unsupported("list position that is not an integer"));
                }
                operands.push(index);
            }
            (MirCollectionOp::Slice, [from, to]) => {
                for arg in [from, to] {
                    let index = self.expr(&arg.expr)?;
                    if !matches!(index.value, Value::Int | Value::Exact) {
                        return Err(unsupported("list position that is not an integer"));
                    }
                    operands.push(index);
                }
            }
            (_, []) => {}
            _ => return Err(unsupported("list method outside the subset")),
        }
        // A method that changes the list is an effect: where it runs
        // relative to its siblings shows in what they read.
        let effect = operands.iter().any(|operand| operand.effect)
            || matches!(
                op,
                MirCollectionOp::RemoveFirst
                    | MirCollectionOp::RemoveLast
                    | MirCollectionOp::ReverseInPlace
            );
        let prelude = self.sequence(&mut operands);
        self.support.need(Helper::List(elem, ListOp::At));
        self.need_class(Ty::Opt(inner));
        let operand = self.list_query_read(elem, inner, op, entry, &operands)?;
        let text = wrap(&prelude, &operand.text);
        let value = operand.value;
        let text = if value == Value::Truth && !prelude.is_empty() {
            format!("({text})")
        } else {
            text
        };
        Ok(Operand::new(text, value).with_effect(effect))
    }

    /// Emit optional end, indexed, removed, and extremum reads.
    fn list_query_read(
        &mut self,
        elem: Elem,
        inner: Inner,
        op: MirCollectionOp,
        entry: &radix_intrinsics::MethodIntrinsic,
        operands: &[Operand],
    ) -> Result<Operand, CodegenError> {
        let suffix = elem.suffix();
        let o = |at: usize| operands[at].text.clone();
        Ok(match op {
            MirCollectionOp::First => {
                let slot = format!("faber_o{}", self.temps);
                let call = format!("faber_list_{suffix}_get({}, 0, &{slot})", o(0));
                self.optional_call(elem, inner, &slot, &call)
            }
            MirCollectionOp::Last => {
                let slot = format!("faber_o{}", self.temps);
                let call = format!(
                    "faber_list_{suffix}_get({0}, ({0})->len - 1, &{slot})",
                    o(0)
                );
                self.optional_call(elem, inner, &slot, &call)
            }
            MirCollectionOp::Index if entry.return_kind == IntrinsicReturnKind::OptionInner => {
                let slot = format!("faber_o{}", self.temps);
                let call = format!("faber_list_{suffix}_get({}, {}, &{slot})", o(0), o(1));
                self.optional_call(elem, inner, &slot, &call)
            }
            MirCollectionOp::RemoveFirst => {
                let slot = format!("faber_o{}", self.temps);
                let call = format!("faber_list_{suffix}_remove_first({}, &{slot})", o(0));
                self.optional_call(elem, inner, &slot, &call)
            }
            MirCollectionOp::RemoveLast => {
                let slot = format!("faber_o{}", self.temps);
                let call = format!("faber_list_{suffix}_remove_last({}, &{slot})", o(0));
                self.optional_call(elem, inner, &slot, &call)
            }
            MirCollectionOp::ArgMax | MirCollectionOp::ArgMin if elem == Elem::Int => {
                let which = if op == MirCollectionOp::ArgMax {
                    "argmax"
                } else {
                    "argmin"
                };
                self.support.need_ints();
                let n = self.temps;
                self.temps += 1;
                let t = format!("faber_o{n}");
                let text = format!(
                    "(__extension__ ({{ int64_t {t} = 0; bool {t}h = faber_list_i64_{which}({}, &{t}); ((faber_opt_i64){{{t}h, {t}}}); }}))",
                    o(0)
                );
                self.need_class(Ty::Opt(Inner::Int));
                Operand::new(text, Value::Opt(Inner::Int))
            }
            _ => return self.list_query_transform(elem, op, operands),
        })
    }

    /// Emit membership, ordering, slicing, and aggregate list queries.
    fn list_query_transform(
        &mut self,
        elem: Elem,
        op: MirCollectionOp,
        operands: &[Operand],
    ) -> Result<Operand, CodegenError> {
        let suffix = elem.suffix();
        let o = |at: usize| operands[at].text.clone();
        Ok(match op {
            MirCollectionOp::Contains => Operand::new(
                format!("(faber_list_{suffix}_contains({}, {}))", o(0), o(1)),
                Value::Truth,
            ),
            MirCollectionOp::ReverseInPlace => Operand::new(
                format!("faber_list_{suffix}_reverse({})", o(0)),
                Value::Void,
            ),
            MirCollectionOp::Reverse => {
                self.support.need(Helper::List(elem, ListOp::New));
                self.list_operand(
                    format!("faber_list_{suffix}_inverted(faber_a, {})", o(0)),
                    elem,
                )
            }
            MirCollectionOp::Sort => {
                self.support.need(Helper::List(elem, ListOp::New));
                self.list_operand(
                    format!("faber_list_{suffix}_sorted(faber_a, {})", o(0)),
                    elem,
                )
            }
            MirCollectionOp::AppendImmutable => {
                self.support.need(Helper::List(elem, ListOp::New));
                self.list_operand(
                    format!("faber_list_{suffix}_added(faber_a, {}, {})", o(0), o(1)),
                    elem,
                )
            }
            MirCollectionOp::Slice => {
                self.support.need(Helper::List(elem, ListOp::New));
                self.list_operand(
                    format!(
                        "faber_list_{suffix}_slice(faber_a, {}, {}, {})",
                        o(0),
                        o(1),
                        o(2)
                    ),
                    elem,
                )
            }
            MirCollectionOp::Take => {
                self.support.need(Helper::List(elem, ListOp::New));
                self.list_operand(
                    format!("faber_list_{suffix}_slice(faber_a, {}, 0, {})", o(0), o(1)),
                    elem,
                )
            }
            MirCollectionOp::DropFirst => {
                self.support.need(Helper::List(elem, ListOp::New));
                self.list_operand(
                    format!(
                        "faber_list_{suffix}_slice(faber_a, {}, {}, ({})->len)",
                        o(0),
                        o(1),
                        o(0)
                    ),
                    elem,
                )
            }
            MirCollectionOp::TakeLast => {
                self.support.need(Helper::List(elem, ListOp::New));
                self.list_operand(
                    format!("faber_list_{suffix}_take_last(faber_a, {}, {})", o(0), o(1)),
                    elem,
                )
            }
            MirCollectionOp::Sum if elem == Elem::Int => {
                self.support.need(Helper::Types);
                self.support.need(Helper::Add);
                Operand::new(format!("faber_list_i64_sum({})", o(0)), Value::Exact)
            }
            _ => return Err(unsupported("list method outside the subset")),
        })
    }

    /// An optional element read: `call` fills the slot named `slot` and says
    /// whether there was one.
    pub(crate) fn optional_call(
        &mut self,
        elem: Elem,
        inner: Inner,
        slot: &str,
        call: &str,
    ) -> Operand {
        self.temps += 1;
        let c_type = elem.c_type();
        let text = match inner.scalar_struct() {
            Some(name) => format!(
                "(__extension__ ({{ {c_type} {slot} = 0; bool {slot}h = {call}; (({name}){{{slot}h, {slot}}}); }}))"
            ),
            None => {
                format!("(__extension__ ({{ {c_type} {slot} = NULL; (void){call}; {slot}; }}))")
            }
        };
        Operand::new(text, Value::Opt(inner))
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
            self.text_literal(&tokens.true_)?,
            self.text_literal(&tokens.false_)?,
        );
        Ok(format!(
            "faber_list_bool_show(faber_a, {text}, {yes}, {no})"
        ))
    }
}
