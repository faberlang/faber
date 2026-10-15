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
use crate::ty::{Elem, Inner, IntWidth, Ty, classify};
use radix_codegen_shared::CodegenError;
use radix_hir::{
    DefId, HirBinOp, HirConversioTarget, HirExpression, HirExpressionKind, HirLiteral,
    HirStoreKind, HirStoreSite, HirUnOp,
};
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
    /// A pointer to an arena-allocated genus.
    Genus(DefId),
    /// A binary64 value (`double`).
    Float,
    /// An optional: a pointer that may be `NULL`, or a presence-flag struct.
    Opt(Inner),
}

impl From<Ty> for Value {
    fn from(ty: Ty) -> Self {
        match ty {
            Ty::Int => Value::Int,
            // `u64` does not fit an `int64_t`, so it reads as an exact value.
            Ty::Sized(IntWidth::U64) => Value::Exact,
            Ty::Sized(_) => Value::Int,
            Ty::Text => Value::Text,
            Ty::Bool => Value::Truth,
            Ty::List(elem) => Value::List(elem),
            Ty::Genus(def_id) => Value::Genus(def_id),
            Ty::Float => Value::Float,
            Ty::Opt(inner) => Value::Opt(inner),
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

    fn is_integer(&self) -> bool {
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
            HirExpressionKind::Path(def_id) => {
                let operand = self.path(*def_id)?;
                // Typecheck narrows the type of a read the guard proved
                // present; the cell is still the optional.
                if let (Value::Opt(inner), Some(ty)) = (operand.value, expr.ty)
                    && classify(self.types, ty).is_ok_and(|class| class == inner.ty())
                {
                    return Ok(Self::narrow_read(&operand, inner));
                }
                Ok(operand)
            }
            HirExpressionKind::OptionalChain(subject, kind) => {
                self.optional_member(expr, subject, kind)
            }
            HirExpressionKind::NonNull(subject, kind) => {
                let unwrapped = self.unwrap(subject)?;
                match kind {
                    radix_hir::HirNonNullKind::Value => Ok(unwrapped),
                    radix_hir::HirNonNullKind::Member(name) => self.field_from(&unwrapped, *name),
                    _ => Err(unsupported("non-null assertion followed by an index")),
                }
            }
            HirExpressionKind::Literal(literal) => {
                // An integer literal typed as a float is that float (`1` in a
                // `float` slot is `1.0`); typecheck admits no other crossing.
                let as_float = expr.ty.is_some_and(|ty| {
                    classify(self.types, ty).is_ok_and(|class| class == Ty::Float)
                });
                match literal {
                    HirLiteral::Nil => self.none_value(expr),
                    HirLiteral::Int(value) if as_float =>
                    {
                        #[expect(clippy::cast_precision_loss)]
                        Ok(self.float_literal(*value as f64))
                    }
                    _ => self.literal(literal),
                }
            }
            HirExpressionKind::Binary(op, left, right) => self.binary(*op, left, right),
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
            } => self.conversion(source, *target, params, recovery.as_deref()),
            HirExpressionKind::Conversio {
                source,
                target: HirConversioTarget::Type(_),
                recovery,
                route: Some(radix_hir::HirConversioRoute::Ordo { row, members }),
                implicit: None,
                ..
            } => self.ordo_conversion(source, *row, members, recovery.as_deref()),
            HirExpressionKind::Scriptum(template, args) => self.template(*template, args),
            HirExpressionKind::Si {
                cond,
                then_block,
                then_catch: None,
                else_block: Some(else_block),
            } => self.conditional(expr, cond, then_block, else_block),
            HirExpressionKind::Vacua => self.empty_list(expr),
            HirExpressionKind::Array(elements) => self.list_literal(expr, elements),
            HirExpressionKind::Index(base, index) => self.list_read(base, index),
            HirExpressionKind::MethodCall(receiver, method, type_args, args, arm) => {
                if !type_args.is_empty() || arm.is_some() {
                    return Err(unsupported("generic or propagating method call"));
                }
                let class = receiver.ty.map(|ty| classify(self.types, ty)).transpose()?;
                if let Some(Ty::Genus(_)) = class {
                    let receiver = self.expr(receiver)?;
                    return self.genus_method(receiver, *method, args);
                }
                if let (Some(Ty::Float), Some(receiver_ty)) = (class, receiver.ty) {
                    let receiver = self.expr(receiver)?;
                    return self.float_method(&receiver, receiver_ty, *method, args);
                }
                if let (Some(Ty::Text), Some(receiver_ty)) = (class, receiver.ty) {
                    let receiver = self.expr(receiver)?;
                    return self.text_method(receiver, receiver_ty, *method, args);
                }
                self.list_method(receiver, *method, args)
            }
            HirExpressionKind::Sort {
                direction,
                source_mode: radix_hir::HirSortSource::Values,
                source,
                binding: None,
                body: None,
                ..
            } => self.sort_values(*direction, source),
            HirExpressionKind::Struct(def_id, fields) => {
                let given = fields
                    .iter()
                    .map(|(symbol, value)| (*symbol, value))
                    .collect::<Vec<_>>();
                if self.variant(*def_id).is_some() {
                    return self.construct_variant(*def_id, &given);
                }
                self.construct(*def_id, &given)
            }
            HirExpressionKind::Verte {
                target,
                entries: Some(entries),
                ..
            } => match classify(self.types, *target)? {
                Ty::Genus(def_id) => self.construct_entries(def_id, entries),
                _ => Err(unsupported("object literal that is not a genus")),
            },
            // `value ∷ T`: the value, checked against the type it is given.
            HirExpressionKind::Verte {
                source,
                target,
                entries: None,
            } => {
                let class = classify(self.types, *target)?;
                if Self::is_none_literal(source) {
                    return self.none_of(class);
                }
                // `[1, 2, 3] ∷ list<int>`: the literal's own type is bounded
                // by its length; the ascription makes it growable.
                let literal = matches!(
                    source.kind,
                    HirExpressionKind::Array(_) | HirExpressionKind::Vacua
                );
                let widen =
                    literal && matches!(class, Ty::List(_)) && !self.is_bounded_type(*target);
                let before = self.unbounded_literals;
                self.unbounded_literals = before || widen;
                let value = self.expr(source);
                self.unbounded_literals = before;
                self.settle(value?, class)
            }
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
            other => Err(unsupported(construct_name(other))),
        }
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
        if self.variant(*def_id).is_some() {
            return self.construct_variant_call(*def_id, args);
        }
        self.invoke(*def_id, None, args)
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
            return Err(unsupported("call to a function outside the program"));
        };
        let (name, params, ret) = (function.name.clone(), function.params.clone(), function.ret);
        // A call from a function to itself is no use: a function only it
        // calls is as unused as one nothing calls.
        if self.current != Some(def_id) {
            self.called.insert(def_id);
        }
        if receiver.is_some() != function.receiver.is_some() {
            return Err(unsupported("method called without a receiver"));
        }
        if args.len() != params.len() {
            return Err(unsupported("call with omitted or extra arguments"));
        }
        let mut operands = Vec::with_capacity(args.len() + 1);
        operands.extend(receiver);
        for (arg, param) in args.iter().zip(params) {
            if arg.name.is_some() || arg.spread {
                return Err(unsupported("named or spread argument"));
            }
            let list_view = matches!(
                arg.marker,
                Some(radix_hir::HirArgMarker::Ref | radix_hir::HirArgMarker::Mut)
            ) && matches!(param, Ty::List(_) | Ty::Genus(_));
            if arg.marker.is_some() && !list_view {
                return Err(unsupported(
                    "access marker on an argument that is not a list or genus",
                ));
            }
            let mut operand = self.expr(&arg.expr)?;
            // An argument carries no store node in HIR, but the runner checks
            // it as a store and names it `argument`; the C target does the same.
            if let Ty::Sized(width) = param {
                if matches!(operand.value, Value::Int | Value::Exact)
                    && operand.stored != Some(param)
                {
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
            operands.push(operand);
        }
        let prelude = self.sequence(&mut operands);
        let mut slots = Vec::with_capacity(operands.len() + 1);
        if self.arena {
            slots.push("faber_a");
        }
        slots.extend(operands.iter().map(|operand| operand.text.as_str()));
        let list = slots.join(", ");
        let value = ret.map_or(Value::Void, Value::from);
        let mut call = format!("{name}({list})");
        // A sized result is a C `intN_t`; it widens to the value form the
        // arithmetic and `print` take.
        match ret {
            Some(Ty::Sized(IntWidth::U64)) => call = format!("((faber_i128){call})"),
            Some(Ty::Sized(_)) => call = format!("((int64_t){call})"),
            _ => {}
        }
        let text = wrap(&prelude, &call);
        // A boolean result carries its own parentheses like every truth operand.
        let text = if value == Value::Truth && prelude.is_empty() {
            format!("({text})")
        } else {
            text
        };
        Ok(Operand::new(text, value).with_effect(true))
    }

    /// C leaves the order of function arguments, and of the two operands of a
    /// helper call, unspecified; Faber evaluates left to right. When two or more
    /// operands share one evaluation and any of them calls a function, each
    /// non-constant operand is bound to a temporary in source order, and the
    /// returned prelude declares them. A call can print, and a sibling can trap,
    /// so the order shows in stdout. Without a call no operand has an effect
    /// that order could reveal: traps all end the program with status 1.
    pub(crate) fn sequence(&mut self, operands: &mut [Operand]) -> String {
        if operands.len() < 2 || !operands.iter().any(|operand| operand.effect) {
            return String::new();
        }
        let mut prelude = String::new();
        for operand in operands.iter_mut() {
            if operand.constant.is_none() {
                prelude.push_str(&self.bind(operand));
            }
        }
        prelude
    }

    /// Bind `operand` to a fresh temporary: returns its declaration and
    /// rewrites the operand to read the temporary.
    pub(crate) fn bind(&mut self, operand: &mut Operand) -> String {
        let c_type = match operand.value {
            Value::Int => "int64_t ".to_owned(),
            Value::Exact => "faber_i128 ".to_owned(),
            Value::Text => "const char *".to_owned(),
            Value::Truth => {
                self.support.need_bool();
                "bool ".to_owned()
            }
            Value::List(Elem::Int) => "faber_list_i64 *".to_owned(),
            Value::List(Elem::Text) => "faber_list_text *".to_owned(),
            Value::List(Elem::Bool) => "faber_list_bool *".to_owned(),
            Value::List(Elem::Float) => "faber_list_f64 *".to_owned(),
            Value::Float => "double ".to_owned(),
            Value::Opt(inner) => match self.c_type(Ty::Opt(inner)) {
                Ok(c_type) if c_type.ends_with('*') => c_type,
                Ok(c_type) => format!("{c_type} "),
                Err(_) => return String::new(),
            },
            Value::Genus(def_id) => match self.c_type(Ty::Genus(def_id)) {
                Ok(c_type) => c_type,
                Err(_) => return String::new(),
            },
            Value::Void => return String::new(),
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
        declaration
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

    fn path(&self, def_id: radix_hir::DefId) -> Result<Operand, CodegenError> {
        match self.locals.get(&def_id) {
            Some(local) => {
                let mut operand = Self::cell_read(&local.name, local.ty);
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
            other => Operand::new(cell.to_owned(), other.into()),
        }
    }

    fn literal(&mut self, literal: &HirLiteral) -> Result<Operand, CodegenError> {
        match literal {
            HirLiteral::Int(value) => self.int(i128::from(*value)),
            HirLiteral::String(symbol) => {
                let raw = self.names.resolve_symbol(*symbol);
                let decoded = radix_codegen_shared::unescape_faber_text(raw);
                Ok(Operand::new(c_string(&decoded)?, Value::Text))
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
            HirLiteral::Ascii(_) => Err(unsupported("ascii text literal")),
            HirLiteral::Octeti(_) => Err(unsupported("byte-buffer literal")),
            HirLiteral::JsonValor(_) => Err(unsupported("json literal")),
            HirLiteral::Regex(..) => Err(unsupported("regex literal")),
            HirLiteral::Nil => Err(unsupported("null literal")),
        }
    }

    fn unary(&mut self, op: HirUnOp, operand: &HirExpression) -> Result<Operand, CodegenError> {
        let inner = self.expr(operand)?;
        match op {
            HirUnOp::Neg if inner.value == Value::Float => Ok(Self::float_negate(&inner)),
            HirUnOp::Neg if !inner.is_integer() => Err(unsupported("negation of a non-integer")),
            HirUnOp::Neg => {
                if let Some(value) = inner.constant {
                    return self.int(-value);
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
            HirUnOp::BitNot => Err(unsupported("bitwise not")),
        }
    }

    fn binary(
        &mut self,
        op: HirBinOp,
        left: &HirExpression,
        right: &HirExpression,
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
        let l = self.expr(left)?;
        let r = self.expr(right)?;
        if l.value == Value::Float || r.value == Value::Float || op == HirBinOp::TrueDiv {
            return self.float_binary(op, l, r);
        }
        match op {
            HirBinOp::Add if l.value == Value::Text && r.value == Value::Text => {
                Ok(self.concat(l, r))
            }
            HirBinOp::Add | HirBinOp::Sub | HirBinOp::Mul | HirBinOp::Div | HirBinOp::Mod => {
                self.arithmetic(op, l, r)
            }
            HirBinOp::Lt
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

    fn arithmetic(
        &mut self,
        op: HirBinOp,
        l: Operand,
        r: Operand,
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
        let (Some(a), Some(b)) = (l.constant, r.constant) else {
            return Ok(self.runtime_arithmetic(op, helper, name, l, r));
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
    ) -> Operand {
        let effect = l.effect || r.effect;
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
            return operand;
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
        let prelude = self.sequence(&mut operands);
        let [l, r] = operands;
        let text = wrap(&prelude, &format!("{name}({}, {})", l.text, r.text));
        let mut operand = Operand::new(text, Value::Exact).with_effect(effect);
        operand.narrow = narrow;
        operand
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
            HirBinOp::Eq | HirBinOp::StrictEq => "==",
            HirBinOp::NotEq | HirBinOp::StrictNotEq => "!=",
            other => return Err(unsupported(binary_name(other))),
        };
        if truth_pair && !matches!(glyph, "==" | "!=") {
            return Err(unsupported("ordering of booleans"));
        }
        let effect = l.effect || r.effect;
        let mut operands = [l, r];
        let prelude = self.sequence(&mut operands);
        let [l, r] = operands;
        // Text orders by code point, which is the order of its UTF-8 bytes
        // read as unsigned: exactly what `strcmp` compares.
        let body = if text_pair {
            format!("(strcmp({}, {}) {glyph} 0)", l.text, r.text)
        } else {
            format!("({} {glyph} {})", l.text, r.text)
        };
        let text = wrap(&prelude, &body);
        Ok(Operand::new(text, Value::Truth).with_effect(effect))
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
            | (Ty::Float, Value::Float) => Ok(value),
            (Ty::List(slot), Value::List(elem)) if slot == elem => Ok(value),
            (Ty::Genus(slot), Value::Genus(genus)) if slot == genus => Ok(value),
            (Ty::Int, Value::Exact) if value.constant.is_some() => {
                Err(unsupported("integer constant beyond i64"))
            }
            (Ty::Int, Value::Exact) => {
                let at = c_string(&self.store_position(site))?;
                let text = self.store_exact(&value, &at);
                Ok(Operand::new(text, Value::Int).with_effect(value.effect))
            }
            (Ty::Sized(width), Value::Int | Value::Exact) => {
                let at = c_string(&self.store_position(site))?;
                Ok(self.store_sized(&value, width, &at))
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
            operand.narrow = value.narrow.clone();
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
                if !integer || proved {
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
        HirExpressionKind::Sort { .. } => "sort expression",
        HirExpressionKind::FirstMatch { .. } => "first-match selection",
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
        HirExpressionKind::Format { .. } => "format operator",
        HirExpressionKind::Marked { .. } => "value-side copy or own marker",
        HirExpressionKind::Error => "error placeholder",
    }
}
