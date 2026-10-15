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
use crate::ty::{Ty, classify};
use radix_codegen_shared::CodegenError;
use radix_hir::{
    HirBinOp, HirConversioTarget, HirExpression, HirExpressionKind, HirLiteral, HirStoreKind,
    HirStoreSite, HirUnOp,
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
}

impl From<Ty> for Value {
    fn from(ty: Ty) -> Self {
        match ty {
            Ty::Int => Value::Int,
            Ty::Text => Value::Text,
            Ty::Bool => Value::Truth,
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
}

impl Operand {
    fn new(text: String, value: Value) -> Self {
        Self {
            text,
            value,
            constant: None,
            effect: false,
        }
    }

    fn with_effect(mut self, effect: bool) -> Self {
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
fn c_int64(value: i64) -> String {
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
            HirExpressionKind::Path(def_id) => self.path(*def_id),
            HirExpressionKind::Literal(literal) => self.literal(literal),
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
            HirExpressionKind::Call(callee, type_args, args, arm) => {
                if !type_args.is_empty() || arm.is_some() {
                    return Err(unsupported("generic or propagating call"));
                }
                self.call(callee, args)
            }
            other => Err(unsupported(construct_name(other))),
        }
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
        let Some(function) = self.functions.get(def_id) else {
            return Err(unsupported("call to a function outside the program"));
        };
        let (name, params, ret) = (function.name.clone(), function.params.clone(), function.ret);
        if args.len() != params.len() {
            return Err(unsupported("call with omitted or extra arguments"));
        }
        let mut operands = Vec::with_capacity(args.len());
        for (arg, param) in args.iter().zip(params) {
            if arg.name.is_some() || arg.spread || arg.marker.is_some() {
                return Err(unsupported("named, spread, or marked argument"));
            }
            let mut operand = self.expr(&arg.expr)?;
            // An argument carries no store node in HIR, but the runner checks
            // it as a store and names it `argument`; the C target does the same.
            if operand.value == Value::Exact && param == Ty::Int {
                if operand.constant.is_some() {
                    return Err(unsupported("integer constant beyond i64"));
                }
                self.support.need(Helper::StoreI64);
                operand.text = format!("faber_store_i64({}, \"argument\")", operand.text);
                operand.value = Value::Int;
            }
            if operand.value != Value::from(param) {
                return Err(unsupported("value stored without a store check"));
            }
            operands.push(operand);
        }
        let prelude = self.sequence(&mut operands);
        let list = operands
            .iter()
            .map(|operand| operand.text.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let value = ret.map_or(Value::Void, Value::from);
        let text = wrap(&prelude, &format!("{name}({list})"));
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
            if operand.constant.is_some() {
                continue;
            }
            let c_type = match operand.value {
                Value::Int => "int64_t ",
                Value::Exact => "faber_i128 ",
                Value::Text => "const char *",
                Value::Truth => {
                    self.support.need_bool();
                    "bool "
                }
                Value::Void => continue,
            };
            let name = format!("faber_t{}", self.temps);
            self.temps += 1;
            write!(prelude, "{c_type}{name} = {}; ", operand.text).ok();
            operand.text = if operand.value == Value::Truth {
                format!("({name})")
            } else {
                name
            };
        }
        prelude
    }

    /// A folded integer constant. Inside `i64` it is an `int64_t` literal;
    /// between `2^63` and the cap it is an exact intermediate that only a
    /// print or a wider store can take.
    fn int(&mut self, value: i128) -> Result<Operand, CodegenError> {
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
        Ok(Operand {
            text,
            value: class,
            constant: Some(value),
            effect: false,
        })
    }

    fn path(&self, def_id: radix_hir::DefId) -> Result<Operand, CodegenError> {
        match self.locals.get(&def_id) {
            // A truth operand carries its own parentheses.
            Some(local) => Ok(Operand {
                text: if local.ty == Ty::Bool {
                    format!("({})", local.name)
                } else {
                    local.name.clone()
                },
                value: local.ty.into(),
                constant: local.constant,
                effect: false,
            }),
            None => Err(unsupported("reference to a name that is not a local")),
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
            HirLiteral::Float(_) | HirLiteral::FloatText { .. } => {
                Err(unsupported("float literal"))
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
            HirUnOp::Neg if !inner.is_integer() => Err(unsupported("negation of a non-integer")),
            HirUnOp::Neg => {
                if let Some(value) = inner.constant {
                    return self.int(-value);
                }
                self.support.need(Helper::Sub);
                Ok(
                    Operand::new(format!("faber_sub(0, {})", inner.text), Value::Exact)
                        .with_effect(inner.effect),
                )
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
        let l = self.expr(left)?;
        let r = self.expr(right)?;
        match op {
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
            self.support.need(helper);
            let effect = l.effect || r.effect;
            let mut operands = [l, r];
            let prelude = self.sequence(&mut operands);
            let [l, r] = operands;
            let text = wrap(&prelude, &format!("{name}({}, {})", l.text, r.text));
            return Ok(Operand::new(text, Value::Exact).with_effect(effect));
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

    /// Integer comparison by exact value: C's usual arithmetic conversions
    /// widen an `int64_t` operand to `faber_i128` when the other is exact.
    fn comparison(
        &mut self,
        op: HirBinOp,
        l: Operand,
        r: Operand,
    ) -> Result<Operand, CodegenError> {
        if !l.is_integer() || !r.is_integer() {
            return Err(unsupported("comparison of text or booleans"));
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
        let effect = l.effect || r.effect;
        let mut operands = [l, r];
        let prelude = self.sequence(&mut operands);
        let [l, r] = operands;
        let text = wrap(&prelude, &format!("({} {glyph} {})", l.text, r.text));
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
            (Ty::Int, Value::Int) | (Ty::Text, Value::Text) | (Ty::Bool, Value::Truth) => Ok(value),
            (Ty::Int, Value::Exact) if value.constant.is_some() => {
                Err(unsupported("integer constant beyond i64"))
            }
            (Ty::Int, Value::Exact) => {
                self.support.need(Helper::StoreI64);
                let at = c_string(&self.store_position(site))?;
                Ok(
                    Operand::new(format!("faber_store_i64({}, {at})", value.text), Value::Int)
                        .with_effect(value.effect),
                )
            }
            _ => Err(unsupported("conversion between value classes")),
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
fn wrap(prelude: &str, expr: &str) -> String {
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
