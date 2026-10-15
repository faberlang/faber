//! Floats (goal unit C7): `float` is IEEE binary64, C `double`.
//!
//! Arithmetic is plain C arithmetic: IEEE gives the runner's results bit for
//! bit once fused multiply-add contraction is off (the runtime header's
//! pragma, and `-std=c11` for gcc). What C would get wrong is lowered through
//! the runtime: the floored `%`, `÷` on integers, exact comparisons of an
//! integer with a float, rounding to an integer (saturating, as the runner
//! does), and the runner's float text. Integers never convert to floats
//! implicitly: typecheck demands a conversion (`integer_to_float_requires_conversio`),
//! so only an integer literal typed as a float becomes a float here.

use crate::Emitter;
use crate::error::unsupported;
use crate::expr::{Operand, Value, wrap};
use crate::helpers::Helper;
use radix_codegen_shared::CodegenError;
use radix_hir::{HirBinOp, HirCallArg};
use radix_intrinsics::IntrinsicMirKind;
use radix_lexer::Symbol;
use radix_types::{MirCollectionOp, TypeId};

/// A binary64 value as a C literal. Rust's `{:?}` writes the shortest
/// round-trip digits (`0.1`, `1e300`, `100.0`), every one a valid C double
/// literal that the compiler reads back to the same value.
pub(crate) fn c_double(value: f64) -> String {
    if value.is_nan() {
        "__builtin_nan(\"\")".to_owned()
    } else if value.is_infinite() {
        let inf = "__builtin_inf()";
        if value < 0.0 {
            format!("(-{inf})")
        } else {
            inf.to_owned()
        }
    } else {
        format!("{value:?}")
    }
}

impl Emitter<'_> {
    pub(crate) fn float_literal(&mut self, value: f64) -> Operand {
        self.support.need(Helper::Float);
        Operand::new(c_double(value), Value::Float)
    }

    /// `-x` on a float.
    pub(crate) fn float_negate(inner: &Operand) -> Operand {
        Operand::new(format!("(-{})", inner.text), Value::Float).with_effect(inner.effect)
    }

    /// A binary operator with a float operand, or `÷`.
    pub(crate) fn float_binary(
        &mut self,
        op: HirBinOp,
        l: Operand,
        r: Operand,
    ) -> Result<Operand, CodegenError> {
        self.support.need(Helper::Float);
        let integer = |operand: &Operand| matches!(operand.value, Value::Int | Value::Exact);
        let effect = l.effect || r.effect;
        let both_float = l.value == Value::Float && r.value == Value::Float;
        let (call, value): (fn(&str, &str) -> String, Value) = match op {
            // `÷` on integers: both become binary64, and a zero divisor traps.
            HirBinOp::TrueDiv if integer(&l) && integer(&r) => {
                (|a, b| format!("faber_true_div({a}, {b})"), Value::Float)
            }
            HirBinOp::TrueDiv | HirBinOp::Div if both_float => {
                (|a, b| format!("({a} / {b})"), Value::Float)
            }
            HirBinOp::Add if both_float => (|a, b| format!("({a} + {b})"), Value::Float),
            HirBinOp::Sub if both_float => (|a, b| format!("({a} - {b})"), Value::Float),
            HirBinOp::Mul if both_float => (|a, b| format!("({a} * {b})"), Value::Float),
            HirBinOp::Mod if both_float => {
                (|a, b| format!("faber_fmod_f64({a}, {b})"), Value::Float)
            }
            HirBinOp::Lt if both_float => (|a, b| format!("({a} < {b})"), Value::Truth),
            HirBinOp::Gt if both_float => (|a, b| format!("({a} > {b})"), Value::Truth),
            HirBinOp::LtEq if both_float => (|a, b| format!("({a} <= {b})"), Value::Truth),
            HirBinOp::GtEq if both_float => (|a, b| format!("({a} >= {b})"), Value::Truth),
            HirBinOp::Eq | HirBinOp::StrictEq if both_float => {
                (|a, b| format!("({a} == {b})"), Value::Truth)
            }
            HirBinOp::NotEq | HirBinOp::StrictNotEq if both_float => {
                (|a, b| format!("({a} != {b})"), Value::Truth)
            }
            // An integer against a float compares true values; C would round
            // the integer to binary64 first.
            HirBinOp::Lt if integer(&l) => {
                (|a, b| format!("faber_int_lt_float({a}, {b})"), Value::Truth)
            }
            HirBinOp::Gt if integer(&l) => {
                (|a, b| format!("faber_int_gt_float({a}, {b})"), Value::Truth)
            }
            HirBinOp::LtEq if integer(&l) => {
                (|a, b| format!("faber_int_le_float({a}, {b})"), Value::Truth)
            }
            HirBinOp::GtEq if integer(&l) => {
                (|a, b| format!("faber_int_ge_float({a}, {b})"), Value::Truth)
            }
            HirBinOp::Lt if integer(&r) => {
                (|a, b| format!("faber_int_gt_float({b}, {a})"), Value::Truth)
            }
            HirBinOp::Gt if integer(&r) => {
                (|a, b| format!("faber_int_lt_float({b}, {a})"), Value::Truth)
            }
            HirBinOp::LtEq if integer(&r) => {
                (|a, b| format!("faber_int_ge_float({b}, {a})"), Value::Truth)
            }
            HirBinOp::GtEq if integer(&r) => {
                (|a, b| format!("faber_int_le_float({b}, {a})"), Value::Truth)
            }
            _ => return Err(unsupported("float operation outside the subset")),
        };
        let mut operands = [l, r];
        let prelude = self.sequence(&mut operands);
        let [l, r] = operands;
        let text = call(&l.text, &r.text);
        // A truth operand carries its own parentheses; the helper calls are
        // wrapped here, the C comparisons already are.
        let text = if value == Value::Truth && text.starts_with("faber_") {
            format!("({text})")
        } else {
            text
        };
        Ok(Operand::new(wrap(&prelude, &text), value).with_effect(effect))
    }

    /// `⌊x⌋`, `⌈x⌉` and `⌊x⌉`: method calls on a float the catalog resolves
    /// by operation. The result is an `int`, saturating as the runner does.
    pub(crate) fn float_method(
        &mut self,
        receiver: &Operand,
        receiver_ty: TypeId,
        method: Symbol,
        args: &[HirCallArg],
    ) -> Result<Operand, CodegenError> {
        let name = self.names.resolve_symbol(method).to_owned();
        let Some(entry) =
            radix_intrinsics::lookup_method_for_type_id(receiver_ty, self.types, &name, args.len())
        else {
            return Err(unsupported("method call"));
        };
        let helper = match entry.mir_kind {
            IntrinsicMirKind::Collection(MirCollectionOp::Floor) => "faber_floor_i64",
            IntrinsicMirKind::Collection(MirCollectionOp::Ceiling) => "faber_ceil_i64",
            IntrinsicMirKind::Collection(MirCollectionOp::Round) => "faber_nearest_i64",
            _ => return Err(unsupported("float method outside the subset")),
        };
        self.support.need(Helper::Float);
        self.support.need_ints();
        let effect = receiver.effect;
        Ok(Operand::new(format!("{helper}({})", receiver.text), Value::Int).with_effect(effect))
    }
}
