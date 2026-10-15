//! Expression emission for the scalar-and-print subset.
//!
//! Integer arithmetic is not emitted as C arithmetic: signed overflow is
//! undefined behavior in C, and Faber arithmetic is exact until the store. The
//! only arithmetic this spike admits is constant folding in exact 128-bit
//! integers with the runner's 64-bit representation cap checked after every
//! operation; the folded value is emitted as a literal. Anything over a
//! variable is rejected until the exact model lands.

use crate::Emitter;
use crate::error::unsupported;
use crate::ty::{Ty, classify};
use radix_codegen_shared::CodegenError;
use radix_hir::{
    HirBinOp, HirConversioTarget, HirExpression, HirExpressionKind, HirLiteral, HirUnOp,
};
use std::fmt::Write as _;

/// The runner's representation cap (`exact_int.rs`, ruling 17a): every integer
/// operand and intermediate lies in `[-2^63, 2^64 - 1]`.
const CAP_MIN: i128 = -(1_i128 << 63);
const CAP_MAX: i128 = (1_i128 << 64) - 1;

/// One emitted expression: C text, its class, and its value when it is a
/// folded integer constant.
pub(crate) struct Operand {
    pub(crate) text: String,
    pub(crate) ty: Ty,
    pub(crate) constant: Option<i128>,
}

impl Operand {
    fn text(text: String) -> Self {
        Self {
            text,
            ty: Ty::Text,
            constant: None,
        }
    }

    /// A folded integer. Printing or storing needs it inside `int64_t`; a
    /// value between 2^63 and the cap is exact for the runner but has no
    /// `int64_t` form.
    fn int(value: i128) -> Result<Self, CodegenError> {
        if !(CAP_MIN..=CAP_MAX).contains(&value) {
            return Err(unsupported(
                "integer constant beyond the 64-bit representation cap",
            ));
        }
        let Ok(narrow) = i64::try_from(value) else {
            return Err(unsupported("integer constant beyond i64"));
        };
        Ok(Self {
            text: c_int64(narrow),
            ty: Ty::Int,
            constant: Some(value),
        })
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
                implicit: Some(_),
                route: None,
                recovery: None,
                ..
            } => self.implicit_store(source, *slot),
            other => Err(unsupported(construct_name(other))),
        }
    }

    fn path(&self, def_id: radix_hir::DefId) -> Result<Operand, CodegenError> {
        match self.locals.get(&def_id) {
            Some(local) => Ok(Operand {
                text: local.name.clone(),
                ty: local.ty,
                constant: local.constant,
            }),
            None => Err(unsupported("reference to a name that is not a local")),
        }
    }

    fn literal(&self, literal: &HirLiteral) -> Result<Operand, CodegenError> {
        match literal {
            HirLiteral::Int(value) => Operand::int(i128::from(*value)),
            HirLiteral::String(symbol) => {
                let raw = self.names.resolve_symbol(*symbol);
                let decoded = radix_codegen_shared::unescape_faber_text(raw);
                Ok(Operand::text(c_string(&decoded)?))
            }
            HirLiteral::BigInt(_) => Err(unsupported("integer literal beyond u64")),
            HirLiteral::Float(_) | HirLiteral::FloatText { .. } => {
                Err(unsupported("float literal"))
            }
            HirLiteral::Bool(_) => Err(unsupported("boolean literal")),
            HirLiteral::Ascii(_) => Err(unsupported("ascii text literal")),
            HirLiteral::Octeti(_) => Err(unsupported("byte-buffer literal")),
            HirLiteral::JsonValor(_) => Err(unsupported("json literal")),
            HirLiteral::Regex(..) => Err(unsupported("regex literal")),
            HirLiteral::Nil => Err(unsupported("null literal")),
        }
    }

    fn unary(&mut self, op: HirUnOp, operand: &HirExpression) -> Result<Operand, CodegenError> {
        let inner = self.expr(operand)?;
        match (op, inner.constant) {
            (HirUnOp::Neg, Some(value)) => Operand::int(-value),
            (HirUnOp::Neg, None) => {
                Err(unsupported("negation of a variable (exact model pending)"))
            }
            (HirUnOp::Not, _) => Err(unsupported("logical not")),
            (HirUnOp::BitNot, _) => Err(unsupported("bitwise not")),
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
        let fold = match op {
            HirBinOp::Add => i128::checked_add,
            HirBinOp::Sub => i128::checked_sub,
            HirBinOp::Mul => i128::checked_mul,
            HirBinOp::Div => floor_div,
            HirBinOp::Mod => floor_mod,
            other => return Err(unsupported(binary_name(other))),
        };
        let (Some(a), Some(b)) = (l.constant, r.constant) else {
            return Err(unsupported(
                "arithmetic on a variable or text (exact model pending)",
            ));
        };
        if b == 0 {
            match op {
                HirBinOp::Div => return Err(unsupported("integer division by zero")),
                HirBinOp::Mod => return Err(unsupported("integer remainder by zero")),
                _ => {}
            }
        }
        match fold(a, b) {
            Some(value) => Operand::int(value),
            None => Err(unsupported(
                "integer constant beyond the 64-bit representation cap",
            )),
        }
    }

    /// Typecheck's implicit store check around a value. It cannot fail when the
    /// slot is the value's own class, which is the only slot this subset has.
    fn implicit_store(
        &mut self,
        source: &HirExpression,
        slot: radix_types::TypeId,
    ) -> Result<Operand, CodegenError> {
        let class = classify(self.types, slot)?;
        let value = self.expr(source)?;
        if value.ty != class {
            return Err(unsupported("conversion between value classes"));
        }
        Ok(value)
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
