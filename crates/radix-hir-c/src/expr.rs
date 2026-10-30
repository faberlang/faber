//! Expression emission.
//!
//! Integer arithmetic is never emitted as C `int64_t` arithmetic: signed
//! overflow is undefined behavior in C, and Faber arithmetic is exact until the
//! store. An operation over a variable is a call into the exact helpers
//! (`faber_add`, ... in `helpers.rs`), which compute in `faber_i128` inside the
//! runner's representation cap; the result has no width until a store narrows
//! it with `faber_store_i64`. Operations whose operands are all constants fold
//! at emit time with the same rules.

use crate::Emitter;
use crate::error::unsupported;
use crate::helpers::Helper;
use crate::items::Omission;
use crate::leaf_routes;
use crate::ty::{Elem, Inner, IntWidth, Ty, classify, int_width_of_bits, returns};
use radix_codegen_shared::CodegenError;
use radix_hir::{
    DefId, HirBinOp, HirConversioTarget, HirExpression, HirExpressionKind, HirLiteral,
    HirStoreKind, HirStoreSite, HirUnOp,
};
use radix_types::BitsWidth;
use std::fmt::Write as _;

/// The runner's representation cap (`exact_int.rs`, ruling 17a): every integer
/// operand and intermediate lies in `[-2^63, 2^64 - 1]`.
const CAP_MIN: i128 = -(1_i128 << 63);
const CAP_MAX: i128 = (1_i128 << 64) - 1;

/// Emit-time fold of one integer operation; `None` when it leaves `i128`.
type Fold = fn(i128, i128) -> Option<i128>;

/// What a C expression holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Value {
    /// An `int64_t`: a local, or a literal inside `i64`.
    Int,
    /// An exact `faber_i128` intermediate inside the cap. It has no width
    /// until it is stored.
    Exact,
    /// A text value (`const char *`).
    Text,
    /// A C `bool`: a comparison, a logical operator, or a boolean value. The
    /// text always carries its own parentheses.
    Truth,
    /// A call to a function with no result; only a statement holds one.
    Void,
    /// A pointer to an arena-allocated list.
    List(Elem),
    /// A pointer to an arena-allocated set (the same list storage).
    Set(Elem),
    /// A pointer to an arena-allocated map (`faber_map_<k>_<v>`).
    Map(Elem, Elem),
    /// A pointer to an arena-allocated genus.
    Genus(DefId),
    /// A binary64 value (`double`).
    Float,
    /// A binary32 value (`float`), rounded per operation and displayed at the
    /// binary32 width.
    Float32,
    /// An exact decimal (`faber_dec`: an `i128` carrier at its own scale).
    Decimal,
    /// An optional: a pointer that may be `NULL`, or a presence-flag struct.
    Opt(Inner),
    /// A bare `none` value, always `(0)`, printed as the locale's none word.
    None,
    /// A function value: a C function pointer, carrying the `Type::Func` id it
    /// was classified from.
    Func(u32),
}

impl From<Ty> for Value {
    fn from(ty: Ty) -> Self {
        match ty {
            // `u64` does not fit an `int64_t`, and `b64` fills a `uint64_t`, so
            // both read as an exact value.
            Ty::Sized(IntWidth::U64)
            | Ty::Modular(IntWidth::U64)
            | Ty::Saturating(IntWidth::U64)
            | Ty::Bits(BitsWidth::B64) => Value::Exact,
            // A narrower raw-bits cell widens to an `int64_t` like `u8`…`u32`.
            Ty::Int | Ty::Sized(_) | Ty::Modular(_) | Ty::Saturating(_) | Ty::Bits(_) => Value::Int,
            Ty::Text => Value::Text,
            Ty::Bool => Value::Truth,
            Ty::List(elem) => Value::List(elem),
            Ty::Set(elem) => Value::Set(elem),
            Ty::Map(k, v) => Value::Map(k, v),
            Ty::Genus(def_id) => Value::Genus(def_id),
            Ty::Float => Value::Float,
            Ty::Float32 => Value::Float32,
            Ty::Decimal => Value::Decimal,
            Ty::Opt(inner) => Value::Opt(inner),
            Ty::None => Value::None,
            Ty::Func(id) => Value::Func(id),
        }
    }
}

/// One emitted expression: C text, what it holds, its value when it is a
/// folded integer constant, and whether it contains a call.
pub(crate) struct Operand {
    pub(crate) text: String,
    pub(crate) value: Value,
    pub(crate) constant: Option<i128>,
    /// The expression calls a function, so where it runs relative to its
    /// siblings can show in the output (see [`Emitter::sequence`]).
    pub(crate) effect: bool,
    /// The same value computed in `int64_t` on the C4 fast path, present only
    /// for a pure integer expression whose leaves are `int64_t` values.
    pub(crate) narrow: Option<Narrow>,
    /// The cell class whose range check this value already went through (the
    /// store node typecheck put around it), so a store does not check twice.
    pub(crate) stored: Option<Ty>,
}

/// An integer expression in `int64_t` that sets `faber_o` on any overflow,
/// and the helpers its text calls. Only a consumer that declares `faber_o` and
/// can fall back to the exact form may use it (see [`Emitter::store_exact`]).
#[derive(Debug, Clone)]
pub(crate) struct Narrow {
    text: String,
    helpers: Vec<Helper>,
}

impl Narrow {
    pub(crate) fn leaf(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            helpers: Vec::new(),
        }
    }

    /// `name(left, right, &faber_o)`, or `name(left, right)` for the floor
    /// helpers, which cannot overflow.
    fn join(helper: Helper, left: &Narrow, right: &Narrow) -> Self {
        let (name, flag) = match helper {
            Helper::AddO => ("faber_add_o", true),
            Helper::SubO => ("faber_sub_o", true),
            Helper::MulO => ("faber_mul_o", true),
            Helper::FdivI64 => ("faber_fdiv_i64", false),
            _ => ("faber_fmod_i64", false),
        };
        let tail = if flag { ", &faber_o" } else { "" };
        let mut helpers = left.helpers.clone();
        helpers.extend(right.helpers.iter().copied());
        helpers.push(helper);
        Self {
            text: format!("{name}({}, {}{tail})", left.text, right.text),
            helpers,
        }
    }
}

impl Operand {
    pub(crate) fn new(text: String, value: Value) -> Self {
        Self {
            text,
            value,
            constant: None,
            effect: false,
            narrow: None,
            stored: None,
        }
    }

    pub(crate) fn with_effect(mut self, effect: bool) -> Self {
        self.effect = effect;
        self
    }

    pub(crate) fn is_integer(&self) -> bool {
        matches!(self.value, Value::Int | Value::Exact)
    }
}

/// An `int64_t` constant expression for `value`. `INT64_C` takes an unsuffixed
/// integer constant, so a negative value is a negation of one; the minimum has
/// no positive counterpart and is spelled `INT64_MIN`.
pub(crate) fn c_int64(value: i64) -> String {
    if value == i64::MIN {
        "INT64_MIN".to_owned()
    } else if value < 0 {
        format!("-INT64_C({})", value.unsigned_abs())
    } else {
        format!("INT64_C({value})")
    }
}

impl Emitter<'_> {
    pub(crate) fn expr(&mut self, expr: &HirExpression) -> Result<Operand, CodegenError> {
        match &expr.kind {
            HirExpressionKind::Path(def_id) => self.path_expression(expr, *def_id),
            HirExpressionKind::OptionalChain(subject, kind) => {
                self.optional_member(expr, subject, kind)
            }
            HirExpressionKind::NonNull(subject, kind) => {
                let unwrapped = self.unwrap(subject)?;
                match kind {
                    radix_hir::HirNonNullKind::Value => Ok(unwrapped),
                    radix_hir::HirNonNullKind::Member(name) => self.field_from(&unwrapped, *name),
                    radix_hir::HirNonNullKind::Index(index) => {
                        self.asserted_index(unwrapped, index)
                    }
                    radix_hir::HirNonNullKind::Call(_) => {
                        Err(unsupported("non-null assertion followed by a call"))
                    }
                }
            }
            HirExpressionKind::Literal(literal) => self.typed_literal(expr, literal),
            HirExpressionKind::Binary(op, left, right) => self.binary(*op, left, right, expr.ty),
            HirExpressionKind::Unary(op, operand) => self.unary(*op, operand),
            HirExpressionKind::Conversio {
                source,
                target: HirConversioTarget::Type(slot),
                implicit: Some(site),
                route: None,
                recovery: None,
                ..
            } => self.implicit_store(source, *slot, site),
            HirExpressionKind::Conversio {
                source,
                target: HirConversioTarget::Type(target),
                params,
                recovery,
                route: None,
                implicit: None,
            } => {
                // A bounded `ascii<_>` whose conversion target is still an
                // inference hole carries its solved capacity on the whole
                // expression's type.
                let target =
                    if self.is_bounded_ascii(*target) && self.ascii_capacity(*target).is_none() {
                        expr.ty.unwrap_or(*target)
                    } else {
                        *target
                    };
                self.conversion(source, target, params, recovery.as_deref())
            }
            HirExpressionKind::Conversio {
                source,
                target: HirConversioTarget::Type(_),
                recovery,
                route: Some(radix_hir::HirConversioRoute::Ordo { row, members }),
                implicit: None,
                ..
            } => self.ordo_conversion(source, *row, members, recovery.as_deref()),
            // `place ↤ source`: the value is `source` converted to the node's
            // resolved target type. A declaration (the common form) then binds
            // that value; a statement form assigns it, handled in `stmt`. When
            // the place is an already-declared local (a nested `a ↤ (b ↤ x)`),
            // the store happens here and the expression yields the stored value.
            HirExpressionKind::ConversioAssign {
                target,
                source,
                recovery,
                params,
            } => {
                let Some(ty) = expr.ty else {
                    return Err(unsupported("conversion assignment without a target type"));
                };
                let value = self.conversion(source, ty, params, recovery.as_deref())?;
                let HirExpressionKind::Path(def_id) = &target.kind else {
                    return Ok(value);
                };
                let Some(local) = self.locals.get(def_id) else {
                    // The enclosing declaration binds it; no store here.
                    return Ok(value);
                };
                let (name, class) = (local.name.clone(), local.ty);
                let value = self.settle(value, class)?;
                if value.value != Value::from(class) {
                    return Err(unsupported("value stored without a store check"));
                }
                let text = format!("(__extension__ ({{ {name} = {}; {name}; }}))", value.text);
                Ok(Operand::new(text, Value::from(class)).with_effect(true))
            }
            _ => self.compound_expression(expr),
        }
    }

    /// A path read, including function values and guard-narrowed optionals.
    fn path_expression(
        &mut self,
        expr: &HirExpression,
        def_id: DefId,
    ) -> Result<Operand, CodegenError> {
        // A named function used as a value is its C function pointer.
        if let Some(function) = self.functions.get(&def_id) {
            let name = function.name.clone();
            let Some(ty) = expr.ty else {
                return Err(unsupported("function value without a resolved type"));
            };
            let Ty::Func(id) = classify(self.types, ty)? else {
                return Err(unsupported("function value of another class"));
            };
            return Ok(Operand::new(name, Value::Func(id)));
        }
        let operand = self.path(def_id)?;
        // Typecheck narrows the type of a read the guard proved
        // present; the cell is still the optional.
        if let (Value::Opt(inner), Some(ty)) = (operand.value, expr.ty)
            && crate::ty::read_class(self.types, ty) == Some(inner.ty())
        {
            return Ok(Self::narrow_read(&operand, inner));
        }
        Ok(operand)
    }

    /// A literal interpreted at its resolved numeric or collection type.
    fn typed_literal(
        &mut self,
        expr: &HirExpression,
        literal: &HirLiteral,
    ) -> Result<Operand, CodegenError> {
        // `{ "k": v }` ascribed to a map is a JSON object: build the
        // map, not a `valor`.
        if let HirLiteral::JsonValor(value) = literal
            && let Some(Ty::Map(k, v)) = expr.ty.map(|ty| classify(self.types, ty)).transpose()?
        {
            return self.map_json_literal(k, v, value);
        }
        // An integer literal typed as a float is that float (`1` in a
        // `float` slot is `1.0`); typecheck admits no other crossing.
        let float_class = expr.ty.and_then(|ty| classify(self.types, ty).ok());
        let as_float = matches!(float_class, Some(Ty::Float | Ty::Float32));
        let as_float32 = matches!(float_class, Some(Ty::Float32));
        let as_decimal = expr
            .ty
            .is_some_and(|ty| classify(self.types, ty).is_ok_and(|class| class == Ty::Decimal));
        match literal {
            HirLiteral::Nil => self.none_value(expr),
            HirLiteral::Int(value) if as_decimal => Ok(self.decimal_int_literal(*value)),
            HirLiteral::FloatText { text, .. } if as_decimal => self.decimal_text_literal(*text),
            // An `f32` literal is the front end's folded binary32
            // value; carry it as a `float` cell.
            HirLiteral::Float(value) | HirLiteral::FloatText { value, .. } if as_float32 =>
            {
                #[expect(clippy::cast_possible_truncation)]
                Ok(self.float_literal32(*value as f32))
            }
            HirLiteral::Int(value) if as_float => {
                if as_float32 {
                    #[expect(clippy::cast_precision_loss)]
                    Ok(self.float_literal32(*value as f32))
                } else {
                    #[expect(clippy::cast_precision_loss)]
                    Ok(self.float_literal(*value as f64))
                }
            }
            _ => self.literal(literal),
        }
    }

    /// Emit compound values, collection operations, and calls.
    fn compound_expression(&mut self, expr: &HirExpression) -> Result<Operand, CodegenError> {
        match &expr.kind {
            HirExpressionKind::Scriptum(template, args) => self.template(*template, args),
            HirExpressionKind::Si {
                cond,
                then_block,
                then_catch: None,
                else_block: Some(else_block),
            } => self.conditional(expr, cond, then_block, else_block),
            HirExpressionKind::Vacua => {
                match expr.ty.map(|ty| classify(self.types, ty)).transpose()? {
                    Some(Ty::Set(elem)) => self.empty_set(elem),
                    Some(Ty::Map(k, v)) => self.empty_map(k, v),
                    _ => self.empty_list(expr),
                }
            }
            HirExpressionKind::Array(elements) => self.list_literal(expr, elements),
            HirExpressionKind::ReadLine(line) => self.read_line(expr, *line),
            HirExpressionKind::Index(base, index) => {
                match crate::map::map_class(self.types, base.ty) {
                    Some((k, v)) => self.map_read(base, index, k, v),
                    None => self.list_read(base, index),
                }
            }
            // `copy x` on a value (a bind or a return), `own x` a transfer.
            HirExpressionKind::Marked { marker, expr } => match marker {
                radix_hir::HirArgMarker::Copy => {
                    let value = self.expr(expr)?;
                    self.copy_value(value)
                }
                radix_hir::HirArgMarker::Own => self.expr(expr),
                _ => Err(unsupported("value-side copy or own marker")),
            },
            HirExpressionKind::MethodCall(receiver, method, type_args, args, arm) => {
                if !type_args.is_empty() || arm.is_some() {
                    return Err(unsupported("generic or propagating method call"));
                }
                self.method_expression(expr, receiver, *method, args)
            }
            HirExpressionKind::Sort {
                direction,
                source_mode,
                source,
                binding: None,
                body: None,
                ..
            } => self.sort_values(
                *direction,
                source,
                *source_mode == radix_hir::HirSortSource::Keys,
            ),
            HirExpressionKind::Struct(def_id, fields) => {
                let given = fields
                    .iter()
                    .map(|(symbol, value)| (*symbol, value))
                    .collect::<Vec<_>>();
                if self.variant(*def_id).is_some() {
                    return self.construct_variant(*def_id, &given);
                }
                self.construct(*def_id, &given, None)
            }
            HirExpressionKind::Verte {
                target,
                entries: Some(entries),
                ..
            } => match classify(self.types, *target)? {
                Ty::Genus(def_id) => self.construct_entries(def_id, entries),
                Ty::Map(k, v) => self.map_entries(k, v, entries),
                _ => Err(unsupported("object literal that is not a genus")),
            },
            // `value ∷ T`: the value, checked against the type it is given.
            HirExpressionKind::Verte {
                source,
                target,
                entries: None,
            } => self.ascribed_expression(source, *target),
            // `x is none` / `x not is none`.
            HirExpressionKind::TypeCheck {
                positive,
                expr: subject,
                ty,
            } if matches!(
                self.types.get(*ty),
                radix_types::Type::Primitive(radix_types::Primitive::Nihil)
            ) =>
            {
                self.none_test(*positive, subject)
            }
            HirExpressionKind::Field(base, name) => self.field_read(base, *name),
            HirExpressionKind::Call(callee, type_args, args, arm) => {
                if !type_args.is_empty() || arm.is_some() {
                    return Err(unsupported("generic or propagating call"));
                }
                self.call(callee, args)
            }
            HirExpressionKind::Clausura(params, ret, _err, body) => {
                self.closure(expr, params, *ret, body)
            }
            HirExpressionKind::Ad { route, opener } => self.ad_route(*route, opener.as_deref()),
            other => Err(unsupported(construct_name(other))),
        }
    }

    /// Emit a type-ascribed value, widening list literals when requested.
    fn ascribed_expression(
        &mut self,
        source: &HirExpression,
        target: radix_types::TypeId,
    ) -> Result<Operand, CodegenError> {
        let class = classify(self.types, target)?;
        if Self::is_none_literal(source) {
            return self.none_of(class);
        }
        // `[1, 2, 3] ∷ list<int>`: the literal's own type is bounded
        // by its length; the ascription makes it growable.
        let literal = matches!(
            source.kind,
            HirExpressionKind::Array(_) | HirExpressionKind::Vacua
        );
        let widen = literal && matches!(class, Ty::List(_)) && !self.is_bounded_type(target);
        let before = self.unbounded_literals;
        self.unbounded_literals = before || widen;
        let value = self.expr(source);
        self.unbounded_literals = before;
        self.settle(value?, class)
    }

    /// Dispatch a method call by the receiver class.
    fn method_expression(
        &mut self,
        expr: &HirExpression,
        receiver: &HirExpression,
        method: radix_lexer::Symbol,
        args: &[radix_hir::HirCallArg],
    ) -> Result<Operand, CodegenError> {
        let class = receiver.ty.map(|ty| classify(self.types, ty)).transpose()?;
        if let Some(Ty::Genus(_)) = class {
            let receiver = self.expr(receiver)?;
            return self.genus_method(receiver, method, args);
        }
        if let (Some(Ty::Float | Ty::Float32), Some(receiver_ty)) = (class, receiver.ty) {
            let receiver = self.expr(receiver)?;
            return self.float_method(receiver, receiver_ty, method, args);
        }
        if let (Some(Ty::Int | Ty::Sized(_)), Some(receiver_ty)) = (class, receiver.ty) {
            let receiver = self.expr(receiver)?;
            return self.int_method(receiver, receiver_ty, method, args);
        }
        if let (Some(Ty::Text), Some(receiver_ty)) = (class, receiver.ty) {
            let receiver = self.expr(receiver)?;
            return self.text_method(receiver, receiver_ty, method, args);
        }
        if let Some(Ty::Set(_)) = class {
            return self.set_method(receiver, method, args);
        }
        if let Some(Ty::Map(..)) = class {
            return self.map_method(receiver, method, args);
        }
        self.list_method(expr.ty, receiver, method, args)
    }

    /// `c ✓ a ✗ b`, and `if` in value position: C's `?:`, which also runs
    /// only the branch taken. Each arm is a block holding just its value.
    fn conditional(
        &mut self,
        expr: &HirExpression,
        cond: &HirExpression,
        then_block: &radix_hir::HirBlock,
        else_block: &radix_hir::HirBlock,
    ) -> Result<Operand, CodegenError> {
        fn arm(block: &radix_hir::HirBlock) -> Option<&HirExpression> {
            if block.statements.is_empty() {
                block.expr.as_deref()
            } else {
                None
            }
        }
        let (Some(then_expr), Some(else_expr)) = (arm(then_block), arm(else_block)) else {
            return Err(unsupported("conditional with statements in a branch"));
        };
        let Some(ty) = expr.ty else {
            return Err(unsupported("conditional without a resolved type"));
        };
        let class = classify(self.types, ty)?;
        let test = self.expr(cond)?;
        if test.value != Value::Truth {
            return Err(unsupported("condition that is not a boolean"));
        }
        let mut then_value = self.expr(then_expr)?;
        let mut else_value = self.expr(else_expr)?;
        then_value = self.settle(then_value, class)?;
        else_value = self.settle(else_value, class)?;
        if then_value.value != Value::from(class) || else_value.value != Value::from(class) {
            return Err(unsupported("conditional arms of different value classes"));
        }
        let effect = test.effect || then_value.effect || else_value.effect;
        Ok(Operand::new(
            format!(
                "({} ? {} : {})",
                test.text, then_value.text, else_value.text
            ),
            Value::from(class),
        )
        .with_effect(effect))
    }

    /// A call to a function this program defines. Arguments arrive wrapped in
    /// their `argument` store, so each already has its parameter's class.
    fn call(
        &mut self,
        callee: &HirExpression,
        args: &[radix_hir::HirCallArg],
    ) -> Result<Operand, CodegenError> {
        let HirExpressionKind::Path(def_id) = &callee.kind else {
            return Err(unsupported("call through a value"));
        };
        // A call through a function-value local is an indirect call.
        if let Some(local) = self.locals.get(def_id)
            && let Ty::Func(id) = local.ty
        {
            let name = local.name.clone();
            return self.call_function_value(&name, id, args);
        }
        if self.variant(*def_id).is_some() {
            return self.construct_variant_call(*def_id, args);
        }
        self.invoke(*def_id, None, args)
    }

    /// A call through a function value: the local holds a C function pointer,
    /// so the call is `name(faber_a, args…)`. Arguments are checked and
    /// narrowed exactly as a direct call's.
    fn call_function_value(
        &mut self,
        name: &str,
        id: u32,
        args: &[radix_hir::HirCallArg],
    ) -> Result<Operand, CodegenError> {
        let (params, ret_ty) = {
            let radix_types::Type::Func(sig) = self.types.get(radix_types::TypeId(id)) else {
                return Err(unsupported("function value without a signature"));
            };
            (sig.params.clone(), sig.ret)
        };
        if args.len() > params.len() {
            return Err(unsupported("call with omitted or extra arguments"));
        }
        let mut operands = Vec::with_capacity(params.len() + 1);
        for (arg, param) in args.iter().zip(&params) {
            if arg.name.is_some() || arg.spread {
                return Err(unsupported("named or spread argument"));
            }
            let class = classify(self.types, param.ty)?;
            let shared = matches!(class, Ty::List(_) | Ty::Genus(_));
            match arg.marker {
                Some(radix_hir::HirArgMarker::Ref | radix_hir::HirArgMarker::Mut) if !shared => {
                    return Err(unsupported(
                        "access marker on an argument that is not a list or genus",
                    ));
                }
                Some(radix_hir::HirArgMarker::Axis) => {
                    return Err(unsupported(
                        "access marker on an argument that is not a list or genus",
                    ));
                }
                _ => {}
            }
            let mut operand = self.expr(&arg.expr)?;
            if arg.marker == Some(radix_hir::HirArgMarker::Copy) {
                operand = self.copy_value(operand)?;
            }
            operand = self.settle(operand, class)?;
            if operand.value != Value::from(class) {
                return Err(unsupported("value stored without a store check"));
            }
            operands.push(operand);
        }
        let prelude = self.sequence(&mut operands)?;
        let mut slots = Vec::with_capacity(operands.len() + 1);
        if self.arena {
            slots.push("faber_a".to_owned());
        }
        slots.extend(operands.iter().map(|operand| operand.text.clone()));
        let call = format!("{name}({})", slots.join(", "));
        let ret = returns(self.types, ret_ty)?;
        let value = ret.map_or(Value::Void, Value::from);
        let text = wrap(&prelude, &call);
        // A boolean result carries its own parentheses like every truth operand.
        let text = if value == Value::Truth && prelude.is_empty() {
            format!("({text})")
        } else {
            text
        };
        Ok(Operand::new(text, value).with_effect(true))
    }

    /// An `ad '<route>' [opener]` stream value: the leaf wrapper for its
    /// route. The leaf gate admits the route's leaf on the target before the
    /// emitter runs; this table is the second admission, so a route whose
    /// prefix has no `runtime/c/<prefix>.h` keeps the emitter's refusal. A
    /// builtin route is never a leaf wrapper.
    fn ad_route(
        &mut self,
        route: radix_lexer::Symbol,
        opener: Option<&HirExpression>,
    ) -> Result<Operand, CodegenError> {
        let route = self.names.resolve_symbol(route).to_owned();
        let Some((prefix, _name)) = route.split_once(':') else {
            return Err(unsupported("stream route"));
        };
        let Some(symbol) = leaf_routes::route_symbol(&route) else {
            return Err(unsupported("stream route"));
        };
        if !leaf_routes::declared(prefix) {
            return Err(unsupported("stream route"));
        }
        let Some(header) = leaf_routes::declared_prefix(prefix) else {
            return Err(unsupported("stream route"));
        };
        self.support.need(Helper::LeafHeader(header));
        // The wrapper's first argument is the arena every `faber_route_*`
        // takes, so the leaf header can allocate its reply.
        self.support.need(Helper::Arena);
        let mut slots = vec!["faber_a".to_owned()];
        if let Some(opener) = opener {
            let operand = self.expr(opener)?;
            slots.push(operand.text);
        }
        Ok(Operand::new(format!("{symbol}({})", slots.join(", ")), Value::Void).with_effect(true))
    }

    /// `read line` / `read`: one stdin line without its terminator, or all of
    /// stdin to EOF. EOF is the runner's Nil, so the slot must be nullable;
    /// the helper returns NULL (none) at EOF.
    fn read_line(&mut self, expr: &HirExpression, line: bool) -> Result<Operand, CodegenError> {
        let Some(ty) = expr.ty else {
            return Err(unsupported("input read without a resolved type"));
        };
        if classify(self.types, ty)? != Ty::Opt(Inner::Text) {
            return Err(unsupported(
                "input read into a slot that is not string ∪ none",
            ));
        }
        self.support.need(Helper::Arena);
        let call = if line {
            "faber_read_line"
        } else {
            "faber_read_all"
        };
        Ok(Operand::new(format!("{call}(faber_a)"), Value::Opt(Inner::Text)).with_effect(true))
    }

    /// The address a `ref`/`mut` scalar argument passes: the caller's own cell,
    /// the pointer a borrowing parameter already holds, or a genus field. The
    /// argument must be a place this subset can address.
    fn caller_address(&mut self, expr: &HirExpression) -> Result<String, CodegenError> {
        match &expr.kind {
            HirExpressionKind::Path(def_id) => {
                let Some(local) = self.locals.get(def_id) else {
                    return Err(unsupported("ref or mut argument that is not a local"));
                };
                if self.ref_params.contains(def_id) {
                    return Ok(local.name.clone());
                }
                Ok(format!("(&{})", local.name))
            }
            HirExpressionKind::Field(base, name) => {
                let base = self.expr(base)?;
                let field = self.field_of(&base, *name)?;
                Ok(format!("(&({}->{}))", base.text, field.c_name))
            }
            _ => Err(unsupported("ref or mut argument that is not a plain local")),
        }
    }

    /// A call to a function or method the program defines. Arguments are
    /// checked and narrowed to their parameters; the receiver, when there is
    /// one, is evaluated first and passed after the arena.
    pub(crate) fn invoke(
        &mut self,
        def_id: DefId,
        receiver: Option<Operand>,
        args: &[radix_hir::HirCallArg],
    ) -> Result<Operand, CodegenError> {
        let Some(function) = self.functions.get(&def_id) else {
            if self.generics.contains(&def_id) {
                return Err(unsupported("generic function"));
            }
            return Err(unsupported("call to a function outside the program"));
        };
        let (name, params, pointees, omissions, ret, err) = (
            function.name.clone(),
            function.params.clone(),
            function.pointee.clone(),
            function.omissions.clone(),
            function.ret,
            function.err,
        );
        // A call from a function to itself is no use: a function only it
        // calls is as unused as one nothing calls.
        if self.current != Some(def_id) {
            self.called.insert(def_id);
        }
        if receiver.is_some() != function.receiver.is_some() {
            return Err(unsupported("method called without a receiver"));
        }
        if args.len() > params.len() {
            return Err(unsupported("call with omitted or extra arguments"));
        }
        let mut operands = Vec::with_capacity(params.len() + 1);
        operands.extend(receiver);
        for (index, param) in params.iter().enumerate() {
            // What fills this argument slot: a given argument, the
            // parameter's default, or none. The runner fills a default at
            // the call site (`lower_direct_call_args`), so the same
            // evaluation happens here, in the caller.
            let (expr, marker) = match args.get(index) {
                Some(arg) => {
                    if arg.name.is_some() || arg.spread {
                        return Err(unsupported("named or spread argument"));
                    }
                    (Some(&arg.expr), arg.marker)
                }
                None => match &omissions[index] {
                    Omission::Required => {
                        return Err(unsupported("call with omitted or extra arguments"));
                    }
                    Omission::Default(default) => (Some(default), None),
                    Omission::Omitted => (None, None),
                },
            };
            let pointer = pointees.get(index).copied().unwrap_or(false);
            operands.push(self.invoke_argument(*param, pointer, expr, marker)?);
        }
        let prelude = self.sequence(&mut operands)?;
        let channel = err.map(|_| {
            let name = format!("faber_t{}", self.temps);
            self.temps += 1;
            name
        });
        let mut slots = Vec::with_capacity(operands.len() + 2);
        if self.arena {
            slots.push("faber_a".to_owned());
        }
        slots.extend(operands.iter().map(|operand| operand.text.clone()));
        if let Some(channel) = &channel {
            slots.push(format!("&{channel}"));
        }
        let list = slots.join(", ");
        let value = ret.map_or(Value::Void, Value::from);
        let mut call = format!("{name}({list})");
        // A sized result is a C `intN_t`; it widens to the value form the
        // arithmetic and `print` take.
        match ret {
            Some(
                Ty::Sized(IntWidth::U64)
                | Ty::Modular(IntWidth::U64)
                | Ty::Saturating(IntWidth::U64),
            ) => call = format!("((faber_i128){call})"),
            Some(Ty::Sized(_) | Ty::Modular(_) | Ty::Saturating(_)) => {
                call = format!("((int64_t){call})");
            }
            _ => {}
        }
        let has_channel = channel.is_some();
        let text = self.invoke_channel(&prelude, call, value, channel.as_deref())?;
        // A boolean result carries its own parentheses like every truth operand.
        let text = if value == Value::Truth && prelude.is_empty() && !has_channel {
            format!("({text})")
        } else {
            text
        };
        Ok(Operand::new(text, value).with_effect(true))
    }

    /// Lower one call argument, including omitted slots and borrowed cells.
    fn invoke_argument(
        &mut self,
        param: Ty,
        pointer: bool,
        expr: Option<&HirExpression>,
        marker: Option<radix_hir::HirArgMarker>,
    ) -> Result<Operand, CodegenError> {
        let shared = matches!(param, Ty::List(_) | Ty::Genus(_));
        match marker {
            // `ref` / `mut` alias the caller's pointer: a list or a genus
            // always has one, and a scalar parameter (one this signature
            // spelled as a pointer) borrows the caller's cell. `copy`
            // (deep) and `own` (transfer) apply to any value; a scalar
            // copies by assignment either way.
            Some(radix_hir::HirArgMarker::Ref | radix_hir::HirArgMarker::Mut)
                if !shared && !pointer =>
            {
                return Err(unsupported(
                    "access marker on an argument that is not a list or genus",
                ));
            }
            Some(radix_hir::HirArgMarker::Axis) => {
                return Err(unsupported(
                    "access marker on an argument that is not a list or genus",
                ));
            }
            _ => {}
        }
        let Some(expr) = expr else {
            if pointer {
                // An omitted `optional` slot: the callee reads a none cell.
                // A compound literal's address lives for the call.
                let none = self.none_of(param)?;
                return Ok(Operand::new(format!("(&{})", none.text), Value::Void));
            }
            return self.none_of(param);
        };
        // A `ref`/`mut` scalar parameter takes the address of the caller's
        // cell (or the pointer a borrowing parameter already holds).
        if pointer {
            if matches!(
                marker,
                Some(radix_hir::HirArgMarker::Ref | radix_hir::HirArgMarker::Mut)
            ) {
                return Ok(Operand::new(self.caller_address(expr)?, Value::Void));
            }
            // A `ref` parameter called with a value has no caller cell:
            // the runner has nothing to write back, so a compound literal
            // at block scope — which lives for the whole block — is the
            // exact view.
            let class = param;
            let value = self.expr(expr)?;
            let operand = self.settle(value, class)?;
            if operand.value != Value::from(class) {
                return Err(unsupported("value stored without a store check"));
            }
            let c_type = self.c_type(class)?;
            let address = match class {
                Ty::Opt(_) => format!("(&{})", operand.text),
                _ => format!("(&(({c_type}){{ {} }}))", operand.text),
            };
            return Ok(Operand::new(address, Value::Void));
        }
        let mut operand = self.expr(expr)?;
        // `copy x` at the call site deep-duplicates before the call (R7).
        if marker == Some(radix_hir::HirArgMarker::Copy) {
            operand = self.copy_value(operand)?;
        }
        // An argument carries no store node in HIR, but the runner checks
        // it as a store and names it `argument`; the C target does the same.
        if let Ty::Sized(width) = param {
            if matches!(operand.value, Value::Int | Value::Exact) && operand.stored != Some(param) {
                operand = self.store_sized(&operand, width, "\"argument\"");
            }
        } else if operand.value == Value::Exact && param == Ty::Int {
            if operand.constant.is_some() {
                return Err(unsupported("integer constant beyond i64"));
            }
            operand.text = self.store_exact(&operand, "\"argument\"");
            operand.value = Value::Int;
            operand.narrow = None;
        }
        // A present value passed to an optional parameter is wrapped.
        if matches!(param, Ty::Opt(_)) {
            operand = self.settle(operand, param)?;
        }
        if operand.value != Value::from(param) {
            return Err(unsupported("value stored without a store check"));
        }
        Ok(operand)
    }

    /// Wrap a call with its error channel and propagation handler.
    fn invoke_channel(
        &mut self,
        prelude: &str,
        call: String,
        value: Value,
        channel: Option<&str>,
    ) -> Result<String, CodegenError> {
        if let Some(channel) = channel {
            let caller_has_channel = self
                .current
                .and_then(|current| self.functions.get(&current))
                .is_some_and(|current| current.err.is_some());
            let failure = if let Some(active_channel) = self.channels.last().cloned() {
                self.channel_handlers.insert(active_channel.clone());
                format!(
                    "if ({channel}.set) {{ {active_channel} = {channel}; goto {active_channel}_handler; }}"
                )
            } else if caller_has_channel {
                format!("if ({channel}.set) {{ *faber_e = {channel}; goto faber_fail; }}")
            } else {
                self.support.need(Helper::Trap);
                format!("if ({channel}.set) faber_trap(\"unhandled error\");")
            };
            let channel_init = format!("faber_chan {channel} = {{false, NULL}}; ");
            if value == Value::Void {
                Ok(wrap(
                    &format!("{channel_init}{prelude}"),
                    &format!("{call}; {failure}"),
                ))
            } else {
                let mut result = Operand::new(call, value);
                let declaration = self.bind(&mut result)?;
                Ok(wrap(
                    &format!("{channel_init}{prelude}{declaration}"),
                    &format!("{failure} {}", result.text),
                ))
            }
        } else {
            Ok(wrap(prelude, &call))
        }
    }

    /// C leaves the order of function arguments, and of the two operands of a
    /// helper call, unspecified; Faber evaluates left to right. When two or more
    /// operands share one evaluation and any of them calls a function, each
    /// non-constant operand is bound to a temporary in source order, and the
    /// returned prelude declares them. A call can print, and a sibling can trap,
    /// so the order shows in stdout. Without a call no operand has an effect
    /// that order could reveal: traps all end the program with status 1.
    pub(crate) fn sequence(&mut self, operands: &mut [Operand]) -> Result<String, CodegenError> {
        if operands.len() < 2 || !operands.iter().any(|operand| operand.effect) {
            return Ok(String::new());
        }
        let mut prelude = String::new();
        for operand in operands.iter_mut() {
            if operand.constant.is_none() {
                prelude.push_str(&self.bind(operand)?);
            }
        }
        Ok(prelude)
    }

    /// Bind `operand` to a fresh temporary: returns its declaration and
    /// rewrites the operand to read the temporary.
    pub(crate) fn bind(&mut self, operand: &mut Operand) -> Result<String, CodegenError> {
        let c_type = match operand.value {
            Value::None => "int ".to_owned(),
            Value::Int => "int64_t ".to_owned(),
            Value::Exact => "faber_i128 ".to_owned(),
            Value::Text => "const char *".to_owned(),
            Value::Truth => {
                self.support.need_bool();
                "bool ".to_owned()
            }
            Value::List(elem) | Value::Set(elem) => self.elem_list_c_pointer(elem)?,
            Value::Map(k, v) => self.map_c_type(k, v)?,
            Value::Float => "double ".to_owned(),
            Value::Float32 => "float ".to_owned(),
            Value::Decimal => {
                self.support.need(Helper::Decimal);
                "faber_dec ".to_owned()
            }
            Value::Opt(inner) => match self.c_type(Ty::Opt(inner)) {
                Ok(c_type) if c_type.ends_with('*') => c_type,
                Ok(c_type) => format!("{c_type} "),
                Err(error) => return Err(error),
            },
            Value::Genus(def_id) => self.c_type(Ty::Genus(def_id))?,
            // A bare `void` has no value to bind, and a function value never
            // carries an effect, so neither is given a temporary.
            Value::Void | Value::Func(_) => return Ok(String::new()),
        };
        let name = format!("faber_t{}", self.temps);
        self.temps += 1;
        let declaration = format!("{c_type}{name} = {}; ", operand.text);
        operand.text = if operand.value == Value::Truth {
            format!("({name})")
        } else {
            name
        };
        operand.narrow = None;
        Ok(declaration)
    }

    /// A folded integer constant. Inside `i64` it is an `int64_t` literal;
    /// between `2^63` and the cap it is an exact intermediate that only a
    /// print or a wider store can take.
    pub(crate) fn int(&mut self, value: i128) -> Result<Operand, CodegenError> {
        if !(CAP_MIN..=CAP_MAX).contains(&value) {
            return Err(unsupported(
                "integer constant beyond the 64-bit representation cap",
            ));
        }
        // The literal spells `INT64_C` / `INT64_MIN` wherever it lands, so it
        // needs `<inttypes.h>` itself, not only when stored or printed.
        let (text, class) = if let Ok(narrow) = i64::try_from(value) {
            self.support.need_ints();
            (c_int64(narrow), Value::Int)
        } else {
            self.support.need(Helper::Types);
            (format!("((faber_i128)UINT64_C({value}))"), Value::Exact)
        };
        let narrow = (class == Value::Int).then(|| Narrow::leaf(&text));
        Ok(Operand {
            text,
            value: class,
            constant: Some(value),
            effect: false,
            narrow,
            stored: None,
        })
    }

    /// A `d64` integer literal: an exact carrier at scale 0.
    fn decimal_int_literal(&mut self, value: u64) -> Operand {
        self.support.need(Helper::Decimal);
        let text = if let Ok(narrow) = i64::try_from(value) {
            self.support.need_ints();
            c_int64(narrow)
        } else {
            self.support.need(Helper::Types);
            format!("(faber_i128)UINT64_C({value})")
        };
        Operand::new(format!("faber_dec_int({text})"), Value::Decimal)
    }

    /// A `d64` decimal literal, carried exactly as its digit text (the
    /// frontend proves it fits the declared width and never rounds it).
    fn decimal_text_literal(
        &mut self,
        symbol: radix_lexer::Symbol,
    ) -> Result<Operand, CodegenError> {
        let raw = self.names.resolve_symbol(symbol);
        let decoded = radix_codegen_shared::unescape_faber_text(raw);
        let Some(carrier) = radix_codegen_shared::decimal::decimal_text_to_carrier(
            &decoded,
            radix_types::NumericWidth::D64,
        ) else {
            return Err(unsupported("decimal literal outside the d64 range"));
        };
        self.support.need(Helper::Decimal);
        self.support.need_ints();
        Ok(Operand::new(
            format!("faber_dec_of({})", c_int64(carrier)),
            Value::Decimal,
        ))
    }

    /// `-x` over an exact decimal.
    fn decimal_negate(&mut self, value: &Operand) -> Operand {
        self.support.need(Helper::Decimal);
        Operand::new(format!("faber_dec_neg({})", value.text), Value::Decimal)
            .with_effect(value.effect)
    }

    /// An operand as an exact decimal: a decimal is itself; an integer is a
    /// scale-0 carrier (`a + 1`, `d64 ≻ int`).
    fn as_decimal(&mut self, value: Operand) -> Result<Operand, CodegenError> {
        match value.value {
            Value::Decimal => Ok(value),
            Value::Int | Value::Exact => {
                self.support.need(Helper::Decimal);
                Ok(Operand::new(
                    format!("faber_dec_int((faber_i128){})", value.text),
                    Value::Decimal,
                )
                .with_effect(value.effect))
            }
            _ => Err(unsupported("decimal operation on a non-numeric value")),
        }
    }

    /// `+ - * / %` and the comparisons over exact decimals. The carriers are
    /// exact until a store rounds; comparison uses the runner's three-way
    /// `faber_dec_cmp`, which scales only as far as `i128` allows.
    fn decimal_binary(
        &mut self,
        op: HirBinOp,
        l: Operand,
        r: Operand,
    ) -> Result<Operand, CodegenError> {
        let l = self.as_decimal(l)?;
        let r = self.as_decimal(r)?;
        let effect = l.effect || r.effect;
        let mut operands = [l, r];
        let prelude = self.sequence(&mut operands)?;
        let [l, r] = operands;
        self.support.need(Helper::Decimal);
        let name = match op {
            HirBinOp::Add => "faber_dec_add",
            HirBinOp::Sub => "faber_dec_sub",
            HirBinOp::Mul => "faber_dec_mul",
            HirBinOp::Div => "faber_dec_div",
            HirBinOp::Mod => "faber_dec_rem",
            HirBinOp::Lt
            | HirBinOp::Gt
            | HirBinOp::LtEq
            | HirBinOp::GtEq
            | HirBinOp::Eq
            | HirBinOp::StrictEq
            | HirBinOp::ApproxEq
            | HirBinOp::NotEq
            | HirBinOp::StrictNotEq
            | HirBinOp::ApproxNotEq => {
                let glyph = match op {
                    HirBinOp::Lt => "<",
                    HirBinOp::Gt => ">",
                    HirBinOp::LtEq => "<=",
                    HirBinOp::GtEq => ">=",
                    HirBinOp::Eq | HirBinOp::StrictEq | HirBinOp::ApproxEq => "==",
                    _ => "!=",
                };
                let call = format!("(faber_dec_cmp({}, {}) {glyph} 0)", l.text, r.text);
                return Ok(Operand::new(wrap(&prelude, &call), Value::Truth).with_effect(effect));
            }
            other => return Err(unsupported(binary_name(other))),
        };
        let call = format!("{name}({}, {})", l.text, r.text);
        Ok(Operand::new(wrap(&prelude, &call), Value::Decimal).with_effect(effect))
    }

    /// `x ∈ xs` / `x ∉ xs`: the list family's `contains`, with the member on
    /// the value side. `left_is_list` says which operand is the collection.
    fn list_membership(
        &mut self,
        negated: bool,
        list_elem: Elem,
        left_is_list: bool,
        l: Operand,
        r: Operand,
    ) -> Result<Operand, CodegenError> {
        let effect = l.effect || r.effect;
        let mut operands = [l, r];
        let prelude = self.sequence(&mut operands)?;
        let [l, r] = operands;
        let (list, member) = if left_is_list { (l, r) } else { (r, l) };
        if member.value != Value::from(list_elem.ty()) {
            return Err(unsupported(
                "membership of a value of another element class",
            ));
        }
        if matches!(list_elem, Elem::Genus(_)) {
            return Err(unsupported("membership on a list of genus"));
        }
        self.need_class(Ty::List(list_elem));
        let call = format!(
            "(faber_list_{}_contains({}, {}))",
            self.elem_suffix(list_elem)?,
            list.text,
            member.text
        );
        let text = if negated { format!("(!{call})") } else { call };
        Ok(Operand::new(wrap(&prelude, &text), Value::Truth).with_effect(effect))
    }

    /// The element kind of a list-typed expression, if it is one.
    fn list_element(&self, ty: Option<radix_types::TypeId>) -> Option<Elem> {
        match classify(self.types, ty?).ok()? {
            Ty::List(elem) => Some(elem),
            _ => None,
        }
    }

    /// The lifted list operators: `xs + ys`, `2 * xs`, `xs / 10`, the
    /// per-element comparisons `xs ≺ ys` / `xs ≤ 3`, and whole-list `≡`.
    /// Called only when at least one operand is a list.
    fn list_binary(
        &mut self,
        op: HirBinOp,
        left: &HirExpression,
        right: &HirExpression,
        ty: Option<radix_types::TypeId>,
        l: Operand,
        r: Operand,
    ) -> Result<Operand, CodegenError> {
        let left_elem = self.list_element(left.ty);
        let right_elem = self.list_element(right.ty);
        // `x ∈ xs` / `x ∉ xs`: membership is the list family's `contains`, with
        // the member on the value side. The list element class picks the
        // family, so this runs before the numeric-only lift restriction.
        if matches!(op, HirBinOp::Between | HirBinOp::NotBetween) {
            let (list_elem, left_is_list) = match (left_elem, right_elem) {
                (Some(elem), None) => (elem, true),
                (None, Some(elem)) => (elem, false),
                _ => return Err(unsupported("membership without one list operand")),
            };
            return self.list_membership(op == HirBinOp::NotBetween, list_elem, left_is_list, l, r);
        }
        let Some(elem) = left_elem.or(right_elem) else {
            return Err(unsupported("elementwise operator without a list operand"));
        };
        let suffix = match elem {
            Elem::Int => "i64",
            Elem::Float => "f64",
            Elem::Float32 => "f32",
            _ => return Err(unsupported("elementwise operator on a non-numeric list")),
        };
        let result_is_list = ty.is_some_and(|ty| {
            classify(self.types, ty).is_ok_and(|class| matches!(class, Ty::List(_)))
        });
        // Whole-list equality is one `bool` over the element equality; the
        // lifted comparison is a list of `bool`.
        let whole_equality = matches!(
            op,
            HirBinOp::Eq
                | HirBinOp::StrictEq
                | HirBinOp::NotEq
                | HirBinOp::StrictNotEq
                | HirBinOp::ApproxEq
                | HirBinOp::ApproxNotEq
        ) && !result_is_list
            && left_elem.is_some()
            && right_elem.is_some();
        if whole_equality {
            self.support.need(Helper::ListLift);
            let effect = l.effect || r.effect;
            let mut operands = [l, r];
            let prelude = self.sequence(&mut operands)?;
            let [l, r] = operands;
            let call = format!("faber_list_{suffix}_equal({}, {})", l.text, r.text);
            let negate = matches!(
                op,
                HirBinOp::NotEq | HirBinOp::StrictNotEq | HirBinOp::ApproxNotEq
            );
            let text = if negate { format!("(!{call})") } else { call };
            return Ok(Operand::new(wrap(&prelude, &text), Value::Truth).with_effect(effect));
        }
        let op_name = match op {
            HirBinOp::Add => "add",
            HirBinOp::Sub => "sub",
            HirBinOp::Mul => "mul",
            HirBinOp::Div => "div",
            HirBinOp::Lt => "lt",
            HirBinOp::Gt => "gt",
            HirBinOp::LtEq => "le",
            HirBinOp::GtEq => "ge",
            HirBinOp::ApproxEq => "eq",
            HirBinOp::ApproxNotEq => "ne",
            _ => return Err(unsupported(binary_name(op))),
        };
        let side = match (left_elem.is_some(), right_elem.is_some()) {
            (true, true) => "pair",
            (true, false) => "left",
            _ => "right",
        };
        self.support.need(Helper::ListLift);
        let effect = l.effect || r.effect;
        let mut operands = [l, r];
        let prelude = self.sequence(&mut operands)?;
        let [l, r] = operands;
        let call = format!(
            "faber_list_{suffix}_{op_name}_lift_{side}(faber_a, {}, {})",
            l.text, r.text
        );
        let value = if matches!(
            op,
            HirBinOp::Lt
                | HirBinOp::Gt
                | HirBinOp::LtEq
                | HirBinOp::GtEq
                | HirBinOp::ApproxEq
                | HirBinOp::ApproxNotEq
        ) {
            Value::List(Elem::Bool)
        } else {
            Value::List(elem)
        };
        Ok(Operand::new(wrap(&prelude, &call), value).with_effect(effect))
    }

    pub(crate) fn path(&self, def_id: radix_hir::DefId) -> Result<Operand, CodegenError> {
        match self.locals.get(&def_id) {
            Some(local) => {
                // A `ref`/`mut` scalar parameter is a pointer to the caller's
                // cell; a read goes through it.
                let cell = if self.ref_params.contains(&def_id) {
                    format!("(*{})", local.name)
                } else {
                    local.name.clone()
                };
                let mut operand = Self::cell_read(&cell, local.ty);
                operand.constant = local.constant;
                Ok(operand)
            }
            None => self
                .unit_variant(def_id)
                .ok_or_else(|| unsupported("reference to a name that is not a local")),
        }
    }

    /// A read of a cell of class `ty`, written `cell` in C. A sized integer
    /// widens to the value form the arithmetic takes: `int64_t`, or an exact
    /// value for `u64`.
    pub(crate) fn cell_read(cell: &str, ty: Ty) -> Operand {
        match ty {
            // A truth operand carries its own parentheses.
            Ty::Bool => Operand::new(format!("({cell})"), Value::Truth),
            Ty::Sized(IntWidth::U64) => Operand::new(format!("((faber_i128){cell})"), Value::Exact),
            Ty::Sized(_) => {
                let text = format!("((int64_t){cell})");
                let mut operand = Operand::new(text.clone(), Value::Int);
                operand.narrow = Some(Narrow::leaf(&text));
                operand
            }
            Ty::Int => {
                let mut operand = Operand::new(cell.to_owned(), Value::Int);
                operand.narrow = Some(Narrow::leaf(cell));
                operand
            }
            // A stored decimal is a `faber_dec` pair at scale 8; a read is the
            // pair itself.
            Ty::Decimal => Operand::new(cell.to_owned(), Value::Decimal),
            // A modular or saturating word reads widened like a sized integer,
            // but with no `int64_t` fast path: its arithmetic must stay exact
            // until the store reduces or clamps.
            Ty::Modular(IntWidth::U64) | Ty::Saturating(IntWidth::U64) => {
                Operand::new(format!("((faber_i128){cell})"), Value::Exact)
            }
            Ty::Modular(_) | Ty::Saturating(_) => {
                Operand::new(format!("((int64_t){cell})"), Value::Int)
            }
            other => Operand::new(cell.to_owned(), other.into()),
        }
    }

    fn literal(&mut self, literal: &HirLiteral) -> Result<Operand, CodegenError> {
        match literal {
            HirLiteral::Int(value) => self.int(i128::from(*value)),
            // `ascii` is an ASCII-only text; the bytes are the same counted
            // UTF-8 value a `string` literal is.
            HirLiteral::String(symbol) | HirLiteral::Ascii(symbol) => {
                let raw = self.names.resolve_symbol(*symbol);
                let decoded = radix_codegen_shared::unescape_faber_text(raw);
                Ok(Operand::new(self.text_literal(&decoded)?, Value::Text))
            }
            // Every truth operand carries its own parentheses, so `if` and
            // `while` write it bare: `while (1)`, `if (a < b)`. Adding a second
            // pair would trip clang's `-Wparentheses-equality` on `((a == b))`.
            HirLiteral::Bool(value) => Ok(Operand::new(
                if *value { "(1)" } else { "(0)" }.to_owned(),
                Value::Truth,
            )),
            HirLiteral::BigInt(_) => Err(unsupported("integer literal beyond u64")),
            HirLiteral::Float(value) | HirLiteral::FloatText { value, .. } => {
                Ok(self.float_literal(*value))
            }
            HirLiteral::Octeti(_) => Err(unsupported("byte-buffer literal")),
            HirLiteral::JsonValor(_) => Err(unsupported("json literal")),
            HirLiteral::Regex(..) => Err(unsupported("regex literal")),
            // `null` against the `none` type: the only value, carried as a
            // dummy 0 that prints as the locale's none word.
            HirLiteral::Nil => Ok(Operand::new("(0)".to_owned(), Value::None)),
        }
    }

    fn unary(&mut self, op: HirUnOp, operand: &HirExpression) -> Result<Operand, CodegenError> {
        let bits_width = operand
            .ty
            .and_then(|ty| match classify(self.types, ty).ok()? {
                Ty::Bits(width) => Some(width),
                _ => None,
            });
        if let Some(width) = bits_width
            && op == HirUnOp::BitNot
        {
            let inner = self.expr(operand)?;
            let cell = int_width_of_bits(width).c_type();
            let text = format!("(({cell})(~({cell}){}))", inner.text);
            return Ok(Operand::new(text, Value::from(Ty::Bits(width))).with_effect(inner.effect));
        }
        // Negation remains refused: raw bits have no arithmetic, and the
        // existing construct name is retained for every unsupported unary op.
        crate::ty::refuse_bits_operand(self.types, operand.ty, "bitwise operator on raw bits")?;
        let wide = operand
            .ty
            .is_some_and(|ty| matches!(classify(self.types, ty), Ok(Ty::Modular(_))));
        let inner = self.expr(operand)?;
        match op {
            HirUnOp::Neg if matches!(inner.value, Value::Float | Value::Float32) => {
                Ok(Self::float_negate(&inner))
            }
            HirUnOp::Neg if inner.value == Value::Decimal => Ok(self.decimal_negate(&inner)),
            HirUnOp::Neg if !inner.is_integer() => Err(unsupported("negation of a non-integer")),
            HirUnOp::Neg => {
                if let Some(value) = inner.constant {
                    return self.int(-value);
                }
                if wide {
                    self.support.need(Helper::Wide);
                    return Ok(Operand::new(
                        format!("faber_sub_wide(0, {})", inner.text),
                        Value::Exact,
                    )
                    .with_effect(inner.effect));
                }
                self.support.need(Helper::Sub);
                let narrow = inner
                    .narrow
                    .as_ref()
                    .map(|n| Narrow::join(Helper::SubO, &Narrow::leaf("INT64_C(0)"), n));
                let mut operand =
                    Operand::new(format!("faber_sub(0, {})", inner.text), Value::Exact)
                        .with_effect(inner.effect);
                operand.narrow = narrow;
                Ok(operand)
            }
            HirUnOp::Not if inner.value == Value::Truth => {
                Ok(Operand::new(format!("(!{})", inner.text), Value::Truth)
                    .with_effect(inner.effect))
            }
            HirUnOp::Not => Err(unsupported("logical not of a non-boolean")),
            HirUnOp::BitNot => {
                if !inner.is_integer() {
                    return Err(unsupported("bitwise not of a non-integer"));
                }
                if wide {
                    self.support.need(Helper::Wide);
                    return Ok(Operand::new(
                        format!("faber_bit_not_wide({})", inner.text),
                        Value::Exact,
                    )
                    .with_effect(inner.effect));
                }
                if let Some(value) = inner.constant {
                    return self.int(!value);
                }
                self.support.need(Helper::Bitwise);
                Ok(
                    Operand::new(format!("faber_bit_not({})", inner.text), Value::Exact)
                        .with_effect(inner.effect),
                )
            }
        }
    }

    fn binary(
        &mut self,
        op: HirBinOp,
        left: &HirExpression,
        right: &HirExpression,
        ty: Option<radix_types::TypeId>,
    ) -> Result<Operand, CodegenError> {
        // `x ≡ null` and `null ≡ x` test for none like `x is none`.
        let equality = match op {
            HirBinOp::Is | HirBinOp::Eq | HirBinOp::StrictEq => Some(true),
            HirBinOp::IsNot | HirBinOp::NotEq | HirBinOp::StrictNotEq => Some(false),
            _ => None,
        };
        if let Some(is_none) = equality {
            if Self::is_none_literal(right) {
                return self.none_test(is_none, left);
            }
            if Self::is_none_literal(left) {
                return self.none_test(is_none, right);
            }
        }
        if op == HirBinOp::Coalesce {
            return self.coalesce(left, right);
        }
        // `x ∈ a‥b` / `x ∈ a…b`: range membership, before the range value is
        // evaluated as an expression.
        if matches!(op, HirBinOp::InRange | HirBinOp::NotInRange) {
            return self.range_membership(op == HirBinOp::NotInRange, left, right);
        }
        let left_bits = left.ty.and_then(|ty| match classify(self.types, ty).ok()? {
            Ty::Bits(width) => Some(width),
            _ => None,
        });
        let right_bits = right
            .ty
            .and_then(|ty| match classify(self.types, ty).ok()? {
                Ty::Bits(width) => Some(width),
                _ => None,
            });
        if left_bits.is_some() || right_bits.is_some() {
            return self.bits_binary(op, left, right, left_bits, right_bits);
        }
        let l = self.expr(left)?;
        let r = self.expr(right)?;
        // `≈` / `≉` is tolerance equality over either family, never the C `==`
        // a float operand would make it.
        if matches!(op, HirBinOp::FuzzyEq | HirBinOp::FuzzyNotEq) {
            return self.fuzzy(op, l, r);
        }
        if self.list_element(left.ty).is_some() || self.list_element(right.ty).is_some() {
            return self.list_binary(op, left, right, ty, l, r);
        }
        // `k ∈ m` / `k ∉ m`: key membership. A map operand is not a list, so
        // this runs before the scalar paths reject the op.
        if matches!(op, HirBinOp::Between | HirBinOp::NotBetween)
            && let Some((k, v)) = crate::map::map_class(self.types, left.ty)
                .or_else(|| crate::map::map_class(self.types, right.ty))
        {
            let map_is_left = crate::map::map_class(self.types, left.ty).is_some();
            return self.map_membership(op == HirBinOp::NotBetween, k, v, map_is_left, l, r);
        }
        if l.value == Value::Decimal || r.value == Value::Decimal {
            return self.decimal_binary(op, l, r);
        }
        let float_operand = matches!(l.value, Value::Float | Value::Float32)
            || matches!(r.value, Value::Float | Value::Float32);
        if float_operand || op == HirBinOp::TrueDiv {
            // The runner's `÷` is `f32` for 8- and 16-bit operands and `f64`
            // otherwise. This target has no `f32` (C7's open question), so it
            // rejects the narrow form by name rather than approximate it.
            if op == HirBinOp::TrueDiv
                && (Self::narrow_width(self.types, left.ty)
                    || Self::narrow_width(self.types, right.ty))
            {
                return Err(unsupported("true division of 8- or 16-bit operands (f32)"));
            }
            return self.float_binary(op, l, r);
        }
        let wide = ty.is_some_and(|t| matches!(classify(self.types, t), Ok(Ty::Modular(_))));
        match op {
            HirBinOp::Add if l.value == Value::Text && r.value == Value::Text => self.concat(l, r),
            HirBinOp::Add | HirBinOp::Sub | HirBinOp::Mul | HirBinOp::Div | HirBinOp::Mod => {
                self.arithmetic(op, l, r, wide)
            }
            HirBinOp::BitAnd
            | HirBinOp::BitOr
            | HirBinOp::BitXor
            | HirBinOp::Shl
            | HirBinOp::Shr => self.bitwise(op, l, r, wide),
            // `≅` / `≇` is exact value equality, including across families.
            HirBinOp::ApproxEq
            | HirBinOp::ApproxNotEq
            | HirBinOp::Lt
            | HirBinOp::Gt
            | HirBinOp::LtEq
            | HirBinOp::GtEq
            | HirBinOp::Eq
            | HirBinOp::NotEq
            | HirBinOp::StrictEq
            | HirBinOp::StrictNotEq => self.comparison(op, l, r),
            HirBinOp::And | HirBinOp::Or => Self::logical(op, &l, &r),
            other => Err(unsupported(binary_name(other))),
        }
    }

    fn bits_binary(
        &mut self,
        op: HirBinOp,
        left: &HirExpression,
        right: &HirExpression,
        left_width: Option<BitsWidth>,
        right_width: Option<BitsWidth>,
    ) -> Result<Operand, CodegenError> {
        if matches!(
            op,
            HirBinOp::Add
                | HirBinOp::Sub
                | HirBinOp::Mul
                | HirBinOp::Div
                | HirBinOp::TrueDiv
                | HirBinOp::Mod
        ) {
            crate::ty::refuse_bits_operand(self.types, left.ty, "arithmetic on raw bits")?;
            crate::ty::refuse_bits_operand(self.types, right.ty, "arithmetic on raw bits")?;
            return Err(unsupported("arithmetic on raw bits"));
        }

        let shift = matches!(op, HirBinOp::Shl | HirBinOp::Shr);
        let width = if shift {
            match (left_width, right_width) {
                (Some(width), None) => {
                    let count_is_integer = right.ty.is_some_and(|ty| {
                        matches!(
                            classify(self.types, ty),
                            Ok(Ty::Int | Ty::Sized(_) | Ty::Modular(_) | Ty::Saturating(_))
                        )
                    });
                    if !count_is_integer {
                        return Err(unsupported("bitwise operator on raw bits"));
                    }
                    width
                }
                _ => return Err(unsupported("bitwise operator on raw bits")),
            }
        } else {
            match (left_width, right_width) {
                (Some(left), Some(right)) if left == right => left,
                _ => return Err(unsupported("bitwise operator on raw bits")),
            }
        };

        let bitwise = matches!(op, HirBinOp::BitAnd | HirBinOp::BitOr | HirBinOp::BitXor);
        let comparison = matches!(
            op,
            HirBinOp::Eq
                | HirBinOp::NotEq
                | HirBinOp::StrictEq
                | HirBinOp::StrictNotEq
                | HirBinOp::Lt
                | HirBinOp::Gt
                | HirBinOp::LtEq
                | HirBinOp::GtEq
        );
        if !shift && !bitwise && !comparison {
            return Err(unsupported("bitwise operator on raw bits"));
        }

        let mut operands = [self.expr(left)?, self.expr(right)?];
        let prelude = self.sequence(&mut operands)?;
        let effect = operands.iter().any(|operand| operand.effect);
        let [l, r] = operands;
        let cell = int_width_of_bits(width).c_type();
        let body = match op {
            HirBinOp::BitAnd => format!("(({cell})({} & {}))", l.text, r.text),
            HirBinOp::BitOr => format!("(({cell})({} | {}))", l.text, r.text),
            HirBinOp::BitXor => format!("(({cell})({} ^ {}))", l.text, r.text),
            HirBinOp::Shl | HirBinOp::Shr => {
                let width_name = match width {
                    BitsWidth::B8 => "b8",
                    BitsWidth::B16 => "b16",
                    BitsWidth::B32 => "b32",
                    BitsWidth::B64 => "b64",
                };
                let direction = if op == HirBinOp::Shl { "shl" } else { "shr" };
                format!(
                    "faber_bits_{direction}_{width_name}({}, {})",
                    l.text, r.text
                )
            }
            HirBinOp::Eq | HirBinOp::StrictEq => format!("({} == {})", l.text, r.text),
            HirBinOp::NotEq | HirBinOp::StrictNotEq => format!("({} != {})", l.text, r.text),
            HirBinOp::Lt => format!("({} < {})", l.text, r.text),
            HirBinOp::Gt => format!("({} > {})", l.text, r.text),
            HirBinOp::LtEq => format!("({} <= {})", l.text, r.text),
            HirBinOp::GtEq => format!("({} >= {})", l.text, r.text),
            _ => return Err(unsupported("bitwise operator on raw bits")),
        };
        let value = if comparison {
            Value::Truth
        } else {
            Value::from(Ty::Bits(width))
        };
        Ok(Operand::new(wrap(&prelude, &body), value).with_effect(effect))
    }

    /// Whether a type classifies to an 8- or 16-bit integer, the widths the
    /// runner's `÷` computes in `f32`.
    fn narrow_width(types: &radix_types::TypeTable, ty: Option<radix_types::TypeId>) -> bool {
        matches!(
            ty.and_then(|ty| classify(types, ty).ok()),
            Some(Ty::Sized(
                IntWidth::I8 | IntWidth::I16 | IntWidth::U8 | IntWidth::U16
            ))
        )
    }

    fn arithmetic(
        &mut self,
        op: HirBinOp,
        l: Operand,
        r: Operand,
        wide: bool,
    ) -> Result<Operand, CodegenError> {
        if !l.is_integer() || !r.is_integer() {
            return Err(unsupported("arithmetic on text or a boolean"));
        }
        let (fold, helper, name): (Fold, Helper, &str) = match op {
            HirBinOp::Add => (i128::checked_add, Helper::Add, "faber_add"),
            HirBinOp::Sub => (i128::checked_sub, Helper::Sub, "faber_sub"),
            HirBinOp::Mul => (i128::checked_mul, Helper::Mul, "faber_mul"),
            HirBinOp::Div => (floor_div, Helper::Div, "faber_div"),
            HirBinOp::Mod => (floor_mod, Helper::Rem, "faber_rem"),
            other => return Err(unsupported(binary_name(other))),
        };
        let (helper, name) = if wide {
            let name = match op {
                HirBinOp::Add => "faber_add_wide",
                HirBinOp::Sub => "faber_sub_wide",
                HirBinOp::Mul => "faber_mul_wide",
                HirBinOp::Div => "faber_div_wide",
                _ => "faber_rem_wide",
            };
            (Helper::Wide, name)
        } else {
            (helper, name)
        };
        let (Some(a), Some(b)) = (l.constant, r.constant) else {
            return self.runtime_arithmetic(op, helper, name, l, r, wide);
        };
        if b == 0 {
            match op {
                HirBinOp::Div => return Err(unsupported("integer division by zero")),
                HirBinOp::Mod => return Err(unsupported("integer remainder by zero")),
                _ => {}
            }
        }
        match fold(a, b) {
            Some(value) => self.int(value),
            None => Err(unsupported(
                "integer constant beyond the 64-bit representation cap",
            )),
        }
    }

    /// `≈` / `≉`: `10^9 · |a − b| ≤ max(|a|, |b|)` between integers, the same
    /// tolerance rule between floats. Both sides are the same family.
    /// `x ∈ a‥b` (end excluded) / `x ∈ a…b` (end included): an integer is in
    /// an integer range when `a <= x` and, per the range's kind, `x` is below
    /// or at most `b`. `∉` negates the whole test.
    fn range_membership(
        &mut self,
        negate: bool,
        value: &HirExpression,
        range: &HirExpression,
    ) -> Result<Operand, CodegenError> {
        let HirExpressionKind::Intervallum {
            start,
            end,
            step,
            kind,
        } = &range.kind
        else {
            return Err(unsupported("membership over a value that is not a range"));
        };
        if step.is_some() {
            return Err(unsupported("range membership with a step"));
        }
        let mut operands = [self.expr(value)?, self.expr(start)?, self.expr(end)?];
        if operands
            .iter()
            .any(|operand| !matches!(operand.value, Value::Int | Value::Exact))
        {
            return Err(unsupported("range membership over a non-integer"));
        }
        let prelude = self.sequence(&mut operands)?;
        let effect = operands.iter().any(|operand| operand.effect);
        let [x, low, high] = operands;
        let lower = format!("({} <= {})", low.text, x.text);
        let upper = match kind {
            radix_hir::HirRangeKind::Exclusive => format!("({} < {})", x.text, high.text),
            radix_hir::HirRangeKind::Inclusive => format!("({} <= {})", x.text, high.text),
        };
        let text = format!("({lower} && {upper})");
        let text = if negate { format!("(!{text})") } else { text };
        Ok(Operand::new(wrap(&prelude, &text), Value::Truth).with_effect(effect))
    }

    fn fuzzy(
        &mut self,
        op: HirBinOp,
        mut l: Operand,
        mut r: Operand,
    ) -> Result<Operand, CodegenError> {
        let is_float = |value: Value| matches!(value, Value::Float | Value::Float32);
        let name = if l.is_integer() && r.is_integer() {
            "faber_approx_int"
        } else if is_float(l.value) && is_float(r.value) {
            if l.value == Value::Float32 {
                "faber_approx_f32"
            } else {
                "faber_approx_f64"
            }
        } else if l.is_integer() || r.is_integer() {
            // An integer against a float joins the float's width: the runner
            // converts the integer to the joined width before the rule.
            let is32 = (is_float(l.value) && l.value == Value::Float32)
                || (is_float(r.value) && r.value == Value::Float32);
            let cast = if is32 { "float" } else { "double" };
            if l.is_integer() {
                l.text = format!("(({cast}){})", l.text);
            }
            if r.is_integer() {
                r.text = format!("(({cast}){})", r.text);
            }
            if is32 {
                "faber_approx_f32"
            } else {
                "faber_approx_f64"
            }
        } else {
            return Err(unsupported(
                "approximate equality between different classes",
            ));
        };
        self.support.need(Helper::Approx);
        let effect = l.effect || r.effect;
        let mut operands = [l, r];
        let prelude = self.sequence(&mut operands)?;
        let [l, r] = operands;
        let call = format!("{name}({}, {})", l.text, r.text);
        let text = if op == HirBinOp::FuzzyNotEq {
            format!("(!{call})")
        } else {
            call
        };
        Ok(Operand::new(wrap(&prelude, &text), Value::Truth).with_effect(effect))
    }

    /// Bitwise `∧ ∨ ⊻` and the shifts `⇐ ⇒`. All of them are exact and
    /// range-checked (`faber::exact`), so there is no `int64_t` fast path:
    /// two's complement on the exact window can leave the cap. A constant pair
    /// folds with the same rules; the frontend folds most of these first.
    fn bitwise(
        &mut self,
        op: HirBinOp,
        l: Operand,
        r: Operand,
        wide: bool,
    ) -> Result<Operand, CodegenError> {
        if !l.is_integer() || !r.is_integer() {
            return Err(unsupported(binary_name(op)));
        }
        let name = match op {
            HirBinOp::BitAnd if wide => "faber_bit_and_wide",
            HirBinOp::BitOr if wide => "faber_bit_or_wide",
            HirBinOp::BitXor if wide => "faber_bit_xor_wide",
            HirBinOp::BitAnd => "faber_bit_and",
            HirBinOp::BitOr => "faber_bit_or",
            HirBinOp::BitXor => "faber_bit_xor",
            HirBinOp::Shl if wide => "faber_shl_wide",
            HirBinOp::Shr if wide => "faber_shr_wide",
            HirBinOp::Shl => "faber_shl",
            HirBinOp::Shr => "faber_shr",
            other => return Err(unsupported(binary_name(other))),
        };
        let (Some(a), Some(b)) = (l.constant, r.constant) else {
            self.support.need(Helper::Bitwise);
            let effect = l.effect || r.effect;
            let mut operands = [l, r];
            let prelude = self.sequence(&mut operands)?;
            let [l, r] = operands;
            let text = wrap(&prelude, &format!("{name}({}, {})", l.text, r.text));
            return Ok(Operand::new(text, Value::Exact).with_effect(effect));
        };
        match fold_bitwise(op, a, b) {
            Some(value) => self.int(value),
            None => Err(unsupported(
                "integer constant beyond the 64-bit representation cap",
            )),
        }
    }

    /// Arithmetic with at least one runtime operand. Division and remainder
    /// of an `int64_t` by a constant other than 0 and -1 stay in `int64_t`
    /// (C4); everything else is an exact helper call, carrying a 64-bit fast
    /// form when the operands are pure `int64_t` expressions.
    fn runtime_arithmetic(
        &mut self,
        op: HirBinOp,
        helper: Helper,
        name: &str,
        l: Operand,
        r: Operand,
        wide: bool,
    ) -> Result<Operand, CodegenError> {
        let effect = l.effect || r.effect;
        if wide {
            self.support.need(helper);
            let mut operands = [l, r];
            let prelude = self.sequence(&mut operands)?;
            let [l, r] = operands;
            let text = wrap(&prelude, &format!("{name}({}, {})", l.text, r.text));
            return Ok(Operand::new(text, Value::Exact).with_effect(effect));
        }
        let floor = match op {
            HirBinOp::Div => Some(Helper::FdivI64),
            HirBinOp::Mod => Some(Helper::FmodI64),
            _ => None,
        };
        let safe_divisor = r
            .constant
            .is_some_and(|c| c != 0 && c != -1 && i64::try_from(c).is_ok());
        if let Some(floor) = floor
            && safe_divisor
            && l.value == Value::Int
        {
            self.support.need(floor);
            let text = Narrow::join(floor, &Narrow::leaf(&l.text), &Narrow::leaf(&r.text)).text;
            let narrow = match (&l.narrow, &r.narrow) {
                (Some(ln), Some(rn)) => Some(Narrow::join(floor, ln, rn)),
                _ => None,
            };
            let mut operand = Operand::new(text, Value::Int).with_effect(effect);
            operand.narrow = narrow;
            return Ok(operand);
        }
        self.support.need(helper);
        let fast = match op {
            HirBinOp::Add => Some(Helper::AddO),
            HirBinOp::Sub => Some(Helper::SubO),
            HirBinOp::Mul => Some(Helper::MulO),
            _ if safe_divisor => floor,
            _ => None,
        };
        let narrow = match (fast, &l.narrow, &r.narrow) {
            (Some(fast), Some(ln), Some(rn)) if !effect => Some(Narrow::join(fast, ln, rn)),
            _ => None,
        };
        let mut operands = [l, r];
        let prelude = self.sequence(&mut operands)?;
        let [l, r] = operands;
        let text = wrap(&prelude, &format!("{name}({}, {})", l.text, r.text));
        let mut operand = Operand::new(text, Value::Exact).with_effect(effect);
        operand.narrow = narrow;
        Ok(operand)
    }

    /// Narrow an exact runtime value into an `int64_t` cell. With a fast form
    /// the value is computed in `int64_t` first, and only an overflow
    /// re-evaluates it exactly through `faber_store_i64`; the expression is
    /// pure, so evaluating it twice cannot show.
    pub(crate) fn store_exact(&mut self, value: &Operand, at: &str) -> String {
        self.support.need(Helper::StoreI64);
        let exact = format!("faber_store_i64({}, {at})", value.text);
        self.fast_or(value, exact)
    }

    /// An exact value that reached an `int` cell with no store node: narrow it
    /// to its own expression type, which typecheck proved is `int` (see
    /// `FIT_I64`). Other values pass through unchanged.
    pub(crate) fn fit(&mut self, value: Operand) -> Result<Operand, CodegenError> {
        if value.value != Value::Exact {
            return Ok(value);
        }
        if value.constant.is_some() {
            return Err(unsupported("integer constant beyond i64"));
        }
        self.support.need(Helper::FitI64);
        let exact = format!("faber_fit_i64({})", value.text);
        let text = self.fast_or(&value, exact);
        Ok(Operand::new(text, Value::Int).with_effect(value.effect))
    }

    /// `exact`, or the fast `int64_t` form of `value` with `exact` as the
    /// overflow fallback when `value` has one.
    fn fast_or(&mut self, value: &Operand, exact: String) -> String {
        let Some(narrow) = &value.narrow else {
            return exact;
        };
        for helper in &narrow.helpers {
            self.support.need(*helper);
        }
        format!(
            "(__extension__ ({{ int faber_o = 0; int64_t faber_r = {}; faber_o ? {exact} : faber_r; }}))",
            narrow.text
        )
    }

    /// The `copy` marker (R7): a deep duplicate. A list gets a fresh arena
    /// array (its elements are values, or immutable text); a genus is
    /// duplicated field by field through the `g_<Name>_copy` generated on
    /// request; a scalar, text or optional copies by assignment. The source
    /// is evaluated once, as the call's argument.
    pub(crate) fn copy_value(&mut self, value: Operand) -> Result<Operand, CodegenError> {
        let effect = value.effect;
        match value.value {
            Value::List(elem) => {
                if matches!(elem, Elem::Genus(_)) {
                    return Err(unsupported("copy of a list of genus"));
                }
                self.support.need(Helper::Arena);
                self.support
                    .need(Helper::List(elem, crate::helpers::ListOp::Of));
                let text = format!(
                    "faber_list_{}_copy(faber_a, {})",
                    self.elem_suffix(elem)?,
                    value.text
                );
                Ok(Operand::new(text, Value::List(elem)).with_effect(effect))
            }
            Value::Genus(def_id) => {
                self.support.need(Helper::Arena);
                let Some(genus) = self.genera.get(&def_id) else {
                    return Err(unsupported("copy of a genus outside the program"));
                };
                let c_name = genus.c_name.clone();
                self.copies.push(def_id);
                let text = format!("{c_name}_copy(faber_a, {})", value.text);
                Ok(Operand::new(text, Value::Genus(def_id)).with_effect(effect))
            }
            _ => Ok(value),
        }
    }

    /// Integer comparison by exact value: C's usual arithmetic conversions
    /// widen an `int64_t` operand to `faber_i128` when the other is exact.
    fn comparison(
        &mut self,
        op: HirBinOp,
        l: Operand,
        r: Operand,
    ) -> Result<Operand, CodegenError> {
        if let (Value::Genus(left), Value::Genus(right)) = (l.value, r.value)
            && left == right
            && matches!(
                op,
                HirBinOp::Eq | HirBinOp::StrictEq | HirBinOp::NotEq | HirBinOp::StrictNotEq
            )
        {
            return self.enum_comparison(op, left, l, r);
        }
        let text_pair = l.value == Value::Text && r.value == Value::Text;
        let truth_pair = l.value == Value::Truth && r.value == Value::Truth;
        if !text_pair && !truth_pair && (!l.is_integer() || !r.is_integer()) {
            return Err(unsupported("comparison of values of different classes"));
        }
        let glyph = match op {
            HirBinOp::Lt => "<",
            HirBinOp::Gt => ">",
            HirBinOp::LtEq => "<=",
            HirBinOp::GtEq => ">=",
            HirBinOp::Eq | HirBinOp::StrictEq | HirBinOp::ApproxEq => "==",
            HirBinOp::NotEq | HirBinOp::StrictNotEq | HirBinOp::ApproxNotEq => "!=",
            other => return Err(unsupported(binary_name(other))),
        };
        if truth_pair && !matches!(glyph, "==" | "!=") {
            return Err(unsupported("ordering of booleans"));
        }
        let effect = l.effect || r.effect;
        let mut operands = [l, r];
        let prelude = self.sequence(&mut operands)?;
        let [l, r] = operands;
        // Text orders by code point, which is the order of its UTF-8 bytes
        // read as unsigned: exactly what `strcmp` compares.
        let body = if text_pair {
            format!("(strcmp({}, {}) {glyph} 0)", l.text, r.text)
        } else if truth_pair
            || (l.constant.is_some() && r.constant.is_some())
            || !Self::negative_constant(&l, &r)
        {
            format!("({} {glyph} {})", l.text, r.text)
        } else {
            // A helper, not a bare operator: a sized or word read compared
            // with a negative constant can be provably constant (`u8 > -1`),
            // which `-Wtype-limits` rejects; the helper is the exact model's
            // too.
            let helper = match op {
                HirBinOp::Lt => "faber_int_lt",
                HirBinOp::Gt => "faber_int_gt",
                HirBinOp::LtEq => "faber_int_le",
                HirBinOp::GtEq => "faber_int_ge",
                HirBinOp::Eq | HirBinOp::StrictEq | HirBinOp::ApproxEq => "faber_int_eq",
                _ => "faber_int_ne",
            };
            self.support.need(Helper::IntCmp);
            format!("({helper}({}, {}))", l.text, r.text)
        };
        let text = wrap(&prelude, &body);
        Ok(Operand::new(text, Value::Truth).with_effect(effect))
    }

    /// Whether one operand is a negative constant and the other is not a
    /// constant: the comparison could be provably constant for an unsigned
    /// read and trip `-Wtype-limits`, so it takes the helper form.
    fn negative_constant(l: &Operand, r: &Operand) -> bool {
        match (l.constant, r.constant) {
            (Some(value), None) | (None, Some(value)) => value < 0,
            _ => false,
        }
    }

    /// `and` / `or` short-circuit, as C's `&&` / `||` do, and C sequences the
    /// left operand before the right, so no temporaries are needed.
    fn logical(op: HirBinOp, l: &Operand, r: &Operand) -> Result<Operand, CodegenError> {
        if l.value != Value::Truth || r.value != Value::Truth {
            return Err(unsupported("logical operator on a non-boolean"));
        }
        let glyph = if op == HirBinOp::And { "&&" } else { "||" };
        Ok(
            Operand::new(format!("({} {glyph} {})", l.text, r.text), Value::Truth)
                .with_effect(l.effect || r.effect),
        )
    }

    /// Typecheck's implicit store around a value: the one place an exact
    /// intermediate is narrowed to its cell.
    fn implicit_store(
        &mut self,
        source: &HirExpression,
        slot: radix_types::TypeId,
        site: &HirStoreSite,
    ) -> Result<Operand, CodegenError> {
        let class = classify(self.types, slot)?;
        let value = self.expr(source)?;
        self.store(value, class, site)
    }

    /// Narrow `value` into a cell of class `class`. An exact constant is
    /// checked now; an exact runtime value goes through `faber_store_i64`.
    pub(crate) fn store(
        &mut self,
        value: Operand,
        class: Ty,
        site: &HirStoreSite,
    ) -> Result<Operand, CodegenError> {
        match (class, value.value) {
            (Ty::Int, Value::Int)
            | (Ty::Text, Value::Text)
            | (Ty::Bool, Value::Truth)
            | (Ty::Float, Value::Float)
            | (Ty::Float32, Value::Float32) => Ok(value),
            // A store across the two float widths is the C cast: `f32`
            // narrows, `float` widens.
            (Ty::Float32, Value::Float) => Ok(Operand::new(
                format!("((float){})", value.text),
                Value::Float32,
            )
            .with_effect(value.effect)),
            (Ty::Float, Value::Float32) => Ok(Operand::new(
                format!("((double){})", value.text),
                Value::Float,
            )
            .with_effect(value.effect)),
            (Ty::List(slot), Value::List(elem)) if slot == elem => Ok(value),
            (Ty::Genus(slot), Value::Genus(genus)) if slot == genus => Ok(value),
            (Ty::Int, Value::Exact) => {
                let at = self.text_literal(&self.store_position(site))?;
                let text = self.store_exact(&value, &at);
                Ok(Operand::new(text, Value::Int).with_effect(value.effect))
            }
            (Ty::Sized(width), Value::Int | Value::Exact) => {
                let at = self.text_literal(&self.store_position(site))?;
                Ok(self.store_sized(&value, width, &at))
            }
            (Ty::Bits(width), Value::Int | Value::Exact) => Self::store_bits(&value, width),
            // A modular word reduces modulo 2^W at the store; a saturating
            // word clamps. Both are total (they never trap).
            (Ty::Modular(width), Value::Int | Value::Exact) => {
                Ok(self.store_word(&value, Ty::Modular(width), "wrapping", width))
            }
            (Ty::Saturating(width), Value::Int | Value::Exact) => {
                Ok(self.store_word(&value, Ty::Saturating(width), "saturating", width))
            }
            (Ty::Opt(slot), Value::Opt(inner)) if slot == inner => Ok(value),
            (Ty::Opt(inner), form) if form == Value::from(inner.ty()) => {
                Ok(self.some_value(&value, inner))
            }
            // An exact value stored into `int ∪ none` narrows to `int` first.
            (Ty::Opt(inner), Value::Exact) if inner == Inner::Int => {
                let narrowed = self.store(value, Ty::Int, site)?;
                Ok(self.some_value(&narrowed, inner))
            }
            // A decimal rounds half-even to scale 8 at the store and checks
            // the `i64` carrier. Nothing else in this subset stores one.
            (Ty::Decimal, Value::Decimal) => {
                let at = self.text_literal(&self.store_position(site))?;
                self.support.need(Helper::Decimal);
                let text = format!("faber_dec_of(faber_dec_store({}, {at}, \"\"))", value.text);
                let mut operand = Operand::new(text, Value::Decimal).with_effect(value.effect);
                operand.stored = Some(Ty::Decimal);
                Ok(operand)
            }
            _ => Err(unsupported("conversion between value classes")),
        }
    }

    /// Narrow an integer into a cell of width `width`: in range at emit time
    /// it needs no check; otherwise `faber_store_<w>` checks it and writes the
    /// runner's `<n> does not fit in `<w>` (<at>)`.
    pub(crate) fn store_sized(&mut self, value: &Operand, width: IntWidth, at: &str) -> Operand {
        let class = Ty::Sized(width);
        let (low, high) = width.range();
        let mut operand = if value.constant.is_some_and(|c| (low..=high).contains(&c)) {
            let mut operand = Self::cell_read(&value.text, class);
            // A constant keeps its own literal text; only the cell's value
            // form matters here.
            operand.text.clone_from(&value.text);
            operand.value = Value::from(class);
            operand.constant = value.constant;
            operand.narrow.clone_from(&value.narrow);
            operand
        } else {
            self.support.need(Helper::SizedStore);
            let call = format!("faber_store_{}({}, {at})", width.marker(), value.text);
            if width == IntWidth::U64 {
                Operand::new(format!("((faber_i128){call})"), Value::Exact)
            } else {
                Operand::new(format!("((int64_t){call})"), Value::Int)
            }
        };
        operand.effect = value.effect;
        operand.stored = Some(class);
        operand
    }

    /// Store an integer value in a raw-bits cell. C's unsigned conversion is
    /// modulo 2^N, but a folded out-of-range value is a rejected lowering, not
    /// a constant that this backend silently masks.
    fn store_bits(value: &Operand, width: BitsWidth) -> Result<Operand, CodegenError> {
        let class = Ty::Bits(width);
        let (low, high) = int_width_of_bits(width).range();
        if value
            .constant
            .is_some_and(|constant| !(low..=high).contains(&constant))
        {
            return Err(unsupported("bits value outside its width"));
        }
        let cell = int_width_of_bits(width).c_type();
        let mut operand = Operand::new(format!("(({cell})({}))", value.text), Value::from(class));
        operand.constant = value.constant;
        operand.effect = value.effect;
        operand.stored = Some(class);
        Ok(operand)
    }

    /// A store into a modular or saturating word: the helper reduces modulo
    /// 2^W or clamps, then the value widens to the class's read form.
    pub(crate) fn store_word(
        &mut self,
        value: &Operand,
        class: Ty,
        kind: &str,
        width: IntWidth,
    ) -> Operand {
        self.support.need_ints();
        let call = format!("faber_store_{kind}_{}({})", width.marker(), value.text);
        let mut operand = if width == IntWidth::U64 {
            Operand::new(format!("((faber_i128){call})"), Value::Exact)
        } else {
            Operand::new(format!("((int64_t){call})"), Value::Int)
        };
        operand.effect = value.effect;
        operand.stored = Some(class);
        operand
    }

    /// An integer that reached a cell of `class` with no store node: an `int`
    /// narrows by its expression type, a sized cell checks its range (typecheck
    /// proved it, so a miss is `numerus overflow`, never a truncation).
    pub(crate) fn settle(&mut self, value: Operand, class: Ty) -> Result<Operand, CodegenError> {
        match class {
            Ty::Int => self.fit(value),
            Ty::Sized(width) => {
                let integer = matches!(value.value, Value::Int | Value::Exact);
                let (low, high) = width.range();
                let proved = value.stored == Some(class)
                    || value.constant.is_some_and(|c| (low..=high).contains(&c));
                if !integer {
                    return Ok(value);
                }
                if proved {
                    // A `u64` cell reads as an exact value (`Value::Exact`); a
                    // proved in-range constant that never went through a store
                    // arrives as an `int64_t` value, so it is wrapped here.
                    if width == IntWidth::U64 && value.value == Value::Int {
                        let mut operand =
                            Operand::new(format!("((faber_i128){})", value.text), Value::Exact);
                        operand.constant = value.constant;
                        operand.effect = value.effect;
                        operand.stored = Some(class);
                        return Ok(operand);
                    }
                    return Ok(value);
                }
                self.support.need(Helper::SizedStore);
                let call = format!("faber_fit_{}({})", width.marker(), value.text);
                let mut operand = if width == IntWidth::U64 {
                    Operand::new(format!("((faber_i128){call})"), Value::Exact)
                } else {
                    Operand::new(format!("((int64_t){call})"), Value::Int)
                };
                operand.effect = value.effect;
                operand.stored = Some(class);
                Ok(operand)
            }
            Ty::Opt(inner) if value.value != Value::Opt(inner) => {
                let value = self.settle(value, inner.ty())?;
                if value.value == Value::from(inner.ty()) {
                    Ok(self.some_value(&value, inner))
                } else {
                    Ok(value)
                }
            }
            // A raw integer reaching a decimal cell (typecheck inserted no
            // store): wrap it at scale 0 and round it like the store would.
            Ty::Decimal if value.value != Value::Decimal => {
                self.support.need(Helper::Decimal);
                let pair = Operand::new(
                    format!("faber_dec_int((faber_i128){})", value.text),
                    Value::Decimal,
                )
                .with_effect(value.effect);
                let stored = format!("faber_dec_of(faber_dec_store({}, \"\", \"\"))", pair.text);
                let mut operand = Operand::new(stored, Value::Decimal).with_effect(value.effect);
                operand.stored = Some(Ty::Decimal);
                Ok(operand)
            }
            Ty::Bits(width) => {
                if !matches!(value.value, Value::Int | Value::Exact) || value.stored == Some(class)
                {
                    return Ok(value);
                }
                Self::store_bits(&value, width)
            }
            Ty::Modular(width) => {
                if value.stored == Some(class) || !matches!(value.value, Value::Int | Value::Exact)
                {
                    return Ok(value);
                }
                Ok(self.store_word(&value, class, "wrapping", width))
            }
            Ty::Saturating(width) => {
                if value.stored == Some(class) || !matches!(value.value, Value::Int | Value::Exact)
                {
                    return Ok(value);
                }
                Ok(self.store_word(&value, class, "saturating", width))
            }
            // A store across the two float widths is the C cast.
            Ty::Float32 => match value.value {
                Value::Float => Ok(Operand::new(
                    format!("((float){})", value.text),
                    Value::Float32,
                )
                .with_effect(value.effect)),
                _ => Ok(value),
            },
            Ty::Float => match value.value {
                Value::Float32 => Ok(Operand::new(
                    format!("((double){})", value.text),
                    Value::Float,
                )
                .with_effect(value.effect)),
                _ => Ok(value),
            },
            _ => Ok(value),
        }
    }

    /// How a trap names the store, worded as the runner and the Rust target
    /// (`exact_int.rs` `store_position`) word it.
    fn store_position(&self, site: &HirStoreSite) -> String {
        let slot = site.slot.map(|slot| self.names.resolve_symbol(slot));
        match (site.kind, slot) {
            (HirStoreKind::Declaration, Some(slot)) => format!("declaration of `{slot}`"),
            (HirStoreKind::Assignment, Some(slot)) => format!("assignment to `{slot}`"),
            (HirStoreKind::Argument, Some(slot)) => format!("argument `{slot}`"),
            (HirStoreKind::Field, Some(slot)) => format!("field `{slot}`"),
            (HirStoreKind::AwaitBinding, Some(slot)) => format!("await-binding `{slot}`"),
            (kind, _) => store_kind_phrase(kind).to_owned(),
        }
    }
}

/// `prelude` and `expr` as one GNU statement expression when the prelude
/// declared temporaries, else `expr`. `__extension__` keeps `-pedantic`
/// quiet; the outer parentheses keep a truth operand self-parenthesized.
pub(crate) fn wrap(prelude: &str, expr: &str) -> String {
    if prelude.is_empty() {
        expr.to_owned()
    } else {
        format!("(__extension__ ({{ {prelude}{expr}; }}))")
    }
}

fn store_kind_phrase(kind: HirStoreKind) -> &'static str {
    match kind {
        HirStoreKind::Declaration => "declaration",
        HirStoreKind::Assignment => "assignment",
        HirStoreKind::Argument => "argument",
        HirStoreKind::ParameterDefault => "parameter default",
        HirStoreKind::Return => "return",
        HirStoreKind::Yield => "yield",
        HirStoreKind::Throw => "throw",
        HirStoreKind::Field => "field",
        HirStoreKind::Element => "collection element",
        HirStoreKind::TuplePosition => "tuple position",
        HirStoreKind::ConversionDefault => "conversion default",
        HirStoreKind::AwaitBinding => "await-binding",
    }
}

/// Floor division; `None` for division by zero.
fn floor_div(a: i128, b: i128) -> Option<i128> {
    let quotient = a.checked_div(b)?;
    if a % b != 0 && ((a < 0) != (b < 0)) {
        Some(quotient - 1)
    } else {
        Some(quotient)
    }
}

/// Floor remainder; `None` for division by zero.
fn floor_mod(a: i128, b: i128) -> Option<i128> {
    let quotient = floor_div(a, b)?;
    Some(a - quotient * b)
}

/// The emit-time fold of a bitwise or shift operator, under the runner's exact
/// rules (`faber::exact`): a result outside the cap is `None`, as is a
/// negative count or a left shift past `2^64`. The frontend rejects the
/// constant forms of those first, so this is the backstop for an unfolded pair.
fn fold_bitwise(op: HirBinOp, a: i128, b: i128) -> Option<i128> {
    let value = match op {
        HirBinOp::BitAnd => a & b,
        HirBinOp::BitOr => a | b,
        HirBinOp::BitXor => a ^ b,
        HirBinOp::Shl => {
            if b < 0 {
                return None;
            }
            // The runtime returns zero before checking the upper shift bound.
            if a == 0 {
                return Some(0);
            }
            if b > 64 {
                return None;
            }
            a.checked_mul(1_i128 << b)?
        }
        HirBinOp::Shr => {
            if b < 0 {
                return None;
            }
            if b >= 127 {
                if a < 0 { -1 } else { 0 }
            } else {
                a >> b
            }
        }
        _ => return None,
    };
    (CAP_MIN..=CAP_MAX).contains(&value).then_some(value)
}

impl Emitter<'_> {
    /// A text literal as a value: interned into a static that carries the
    /// text header (`FABER_TEXT_LITERAL`), so the operand is a `.s`
    /// reference whose counts are already in place and `length` never walks.
    /// One static per distinct content: a repeated literal reuses the index.
    pub(crate) fn text_literal(&mut self, decoded: &str) -> Result<String, CodegenError> {
        let source = crate::expr::c_string(decoded)?;
        if let Some(&index) = self.text_literal_index.get(&source) {
            return Ok(format!("faber_L{index}.s"));
        }
        // Reinterpret the count bits, preserving the original signed-cast semantics.
        let bytes = (decoded.len() as u64).cast_signed();
        let cps = (decoded.chars().count() as u64).cast_signed();
        let index = self.text_literals.len();
        self.text_literals.push((source.clone(), bytes, cps));
        self.text_literal_index.insert(source, index);
        Ok(format!("faber_L{index}.s"))
    }
}

/// A C string literal holding the UTF-8 bytes of `text`.
///
/// Printable ASCII is verbatim and the common escapes go by name. `?` is
/// escaped so no `??x` trigraph forms under `-std=c11`. Every other byte is a
/// three-digit octal escape: a `\x` escape would swallow a following hex
/// digit (`"\xe9" "abc"` is not `"\xe9abc"`), an octal escape stops at three.
/// A NUL is rejected because the string is NUL-terminated and printed with
/// `%s`, which would stop there.
pub(crate) fn c_string(text: &str) -> Result<String, CodegenError> {
    let mut out = String::from("\"");
    for byte in text.bytes() {
        match byte {
            0 => return Err(unsupported("text containing a NUL character")),
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            b'?' => out.push_str("\\?"),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            b' '..=b'~' => out.push(char::from(byte)),
            other => {
                write!(out, "\\{other:03o}").ok();
            }
        }
    }
    out.push('"');
    Ok(out)
}

fn binary_name(op: HirBinOp) -> &'static str {
    match op {
        HirBinOp::Add => "addition",
        HirBinOp::Sub => "subtraction",
        HirBinOp::Mul => "multiplication",
        HirBinOp::Div => "division",
        HirBinOp::Mod => "remainder",
        HirBinOp::TrueDiv => "true division",
        HirBinOp::Dot
        | HirBinOp::Cross
        | HirBinOp::TensorProduct
        | HirBinOp::Hadamard
        | HirBinOp::HadamardDiv => "tensor product operator",
        HirBinOp::Eq | HirBinOp::NotEq | HirBinOp::StrictEq | HirBinOp::StrictNotEq => "equality",
        HirBinOp::ApproxEq | HirBinOp::ApproxNotEq | HirBinOp::FuzzyEq | HirBinOp::FuzzyNotEq => {
            "approximate equality"
        }
        HirBinOp::Lt | HirBinOp::Gt | HirBinOp::LtEq | HirBinOp::GtEq => "comparison",
        HirBinOp::And | HirBinOp::Or => "logical operator",
        HirBinOp::Coalesce => "null coalescing",
        HirBinOp::BitAnd | HirBinOp::BitOr | HirBinOp::BitXor => "bitwise operator",
        HirBinOp::Shl | HirBinOp::Shr => "shift operator",
        HirBinOp::Is | HirBinOp::IsNot => "type test",
        HirBinOp::InRange | HirBinOp::NotInRange | HirBinOp::Between | HirBinOp::NotBetween => {
            "range membership"
        }
    }
}

/// The name a rejection gives an expression kind the subset does not admit.
pub(crate) fn construct_name(kind: &HirExpressionKind) -> &'static str {
    match kind {
        HirExpressionKind::Path(_) => "path",
        HirExpressionKind::Literal(_) => "literal",
        HirExpressionKind::Vacua => "empty collection (vacua)",
        HirExpressionKind::Binary(..) => "binary expression",
        HirExpressionKind::Unary(..) => "unary expression",
        HirExpressionKind::Call(..) => "call",
        HirExpressionKind::Gradient { .. } => "gradient selection",
        HirExpressionKind::MethodCall(..) => "method call",
        HirExpressionKind::Field(..) => "field access",
        HirExpressionKind::Index(..) => "index access",
        HirExpressionKind::OptionalChain(..) => "optional chaining",
        HirExpressionKind::NonNull(..) => "non-null assertion",
        HirExpressionKind::Block(_) => "block expression",
        HirExpressionKind::Si { .. } => "conditional",
        HirExpressionKind::Discerne { .. } => "discerne or switch",
        HirExpressionKind::Loop(_) => "loop",
        HirExpressionKind::Dum(..) => "while loop",
        HirExpressionKind::Itera { .. } => "iteration loop",
        HirExpressionKind::IteraLockstep { .. } => "lockstep iteration loop",
        HirExpressionKind::IteraRange { .. } => "range-product iteration loop",
        HirExpressionKind::Reduction { .. } => "reduction",
        HirExpressionKind::Scan { .. } => "scan",
        HirExpressionKind::Sort { .. } => "sort expression",
        HirExpressionKind::Intervallum { .. } => "range value",
        HirExpressionKind::Assign(..) => "assignment",
        HirExpressionKind::ConversioAssign { .. } => "conversion assignment",
        HirExpressionKind::Array(_) => "list literal",
        HirExpressionKind::Struct(..) => "genus literal",
        HirExpressionKind::Tuple(..) => "tuple",
        HirExpressionKind::Scribe(..) => "print as a value",
        HirExpressionKind::Scriptum(..) => "formatted text",
        HirExpressionKind::ReadLine(_) => "input read",
        HirExpressionKind::Praefixum(_) => "compile-time evaluation",
        HirExpressionKind::Insere(_) => "file embed",
        HirExpressionKind::Adfirma(..) => "assertion",
        HirExpressionKind::Panic(_) => "panic",
        HirExpressionKind::Throw(_) => "error channel throw",
        HirExpressionKind::Handled { .. } => "error channel handler",
        HirExpressionKind::Trap(_) => "error channel capture",
        HirExpressionKind::CallDefault { .. } => "error channel call default",
        HirExpressionKind::Clausura(..) => "closure",
        HirExpressionKind::Cede(_)
        | HirExpressionKind::Reddet(_)
        | HirExpressionKind::Tacebit(_) => "await",
        HirExpressionKind::Yield(_) => "yield",
        HirExpressionKind::Verte { .. } => "type ascription",
        HirExpressionKind::Conversio { .. } => "value conversion",
        HirExpressionKind::Widen { .. } => "integer widening",
        HirExpressionKind::Ad { .. } => "stream route",
        HirExpressionKind::Ref(..) | HirExpressionKind::Deref(_) => "reference",
        HirExpressionKind::TypeCheck { .. } => "type test",
        HirExpressionKind::CopyInto { .. } => "copy-into statement",
        HirExpressionKind::SinkInto { .. } => "sink-into statement",
        HirExpressionKind::Format { .. } => "format operator",
        HirExpressionKind::Marked { .. } => "value-side copy or own marker",
        HirExpressionKind::Error => "error placeholder",
    }
}

#[cfg(test)]
#[path = "expr_test.rs"]
mod tests;
