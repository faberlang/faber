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
use radix_types::{MirCollectionOp, MirMathFunction, TypeId};

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

/// A binary32 value as a C float literal. Rust's `{:?}` on an `f32` writes the
/// shortest round-trip digits for the binary32 width (`0.1`, `1e20`, `1.0`),
/// every one a valid C `float` literal with the `f` suffix.
pub(crate) fn c_float(value: f32) -> String {
    if value.is_nan() {
        "__builtin_nanf(\"\")".to_owned()
    } else if value.is_infinite() {
        let inf = "__builtin_inff()";
        if value < 0.0 {
            format!("(-{inf})")
        } else {
            inf.to_owned()
        }
    } else {
        format!("{value:?}f")
    }
}

impl Emitter<'_> {
    pub(crate) fn float_literal(&mut self, value: f64) -> Operand {
        self.support.need(Helper::Float);
        Operand::new(c_double(value), Value::Float)
    }

    pub(crate) fn float_literal32(&mut self, value: f32) -> Operand {
        self.support.need(Helper::Float);
        Operand::new(c_float(value), Value::Float32)
    }

    /// `-x` on a float.
    pub(crate) fn float_negate(inner: &Operand) -> Operand {
        Operand::new(format!("(-{})", inner.text), inner.value).with_effect(inner.effect)
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
        let is_float = |value: Value| matches!(value, Value::Float | Value::Float32);
        let both_float = is_float(l.value) && is_float(r.value);
        // A binary32 operand makes the result binary32; typecheck inserts a
        // conversion across widths, so both operands agree here.
        let f32_result = l.value == Value::Float32 || r.value == Value::Float32;
        let float_value = if f32_result {
            Value::Float32
        } else {
            Value::Float
        };
        // `≅` / `≇` across families compares true values, never rounding the
        // integer to binary64 first.
        if matches!(op, HirBinOp::ApproxEq | HirBinOp::ApproxNotEq) && (integer(&l) || integer(&r))
        {
            self.support.need(Helper::Approx);
            let (exact, float) = if integer(&l) { (l, r) } else { (r, l) };
            let mut operands = [exact, float];
            let prelude = self.sequence(&mut operands)?;
            let [exact, float] = operands;
            let call = format!("faber_int_eq_float({}, {})", exact.text, float.text);
            let text = if op == HirBinOp::ApproxNotEq {
                format!("(!{call})")
            } else {
                call
            };
            return Ok(Operand::new(wrap(&prelude, &text), Value::Truth).with_effect(effect));
        }
        let (call, value): (fn(&str, &str) -> String, Value) = match op {
            // `÷` on integers: both become binary64, and a zero divisor traps.
            HirBinOp::TrueDiv if integer(&l) && integer(&r) => {
                (|a, b| format!("faber_true_div({a}, {b})"), Value::Float)
            }
            HirBinOp::TrueDiv | HirBinOp::Div if both_float => {
                (|a, b| format!("({a} / {b})"), float_value)
            }
            HirBinOp::Add if both_float => (|a, b| format!("({a} + {b})"), float_value),
            HirBinOp::Sub if both_float => (|a, b| format!("({a} - {b})"), float_value),
            HirBinOp::Mul if both_float => (|a, b| format!("({a} * {b})"), float_value),
            HirBinOp::Mod if both_float => {
                (|a, b| format!("faber_fmod_f64({a}, {b})"), float_value)
            }
            HirBinOp::Lt if both_float => (|a, b| format!("({a} < {b})"), Value::Truth),
            HirBinOp::Gt if both_float => (|a, b| format!("({a} > {b})"), Value::Truth),
            HirBinOp::LtEq if both_float => (|a, b| format!("({a} <= {b})"), Value::Truth),
            HirBinOp::GtEq if both_float => (|a, b| format!("({a} >= {b})"), Value::Truth),
            HirBinOp::Eq | HirBinOp::StrictEq | HirBinOp::ApproxEq if both_float => {
                (|a, b| format!("({a} == {b})"), Value::Truth)
            }
            HirBinOp::NotEq | HirBinOp::StrictNotEq | HirBinOp::ApproxNotEq if both_float => {
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
        let prelude = self.sequence(&mut operands)?;
        let [l, r] = operands;
        let text = call(&l.text, &r.text);
        // The binary32 floored `%` is the one helper whose name carries the
        // width; every other float helper promotes the operand exactly.
        let text = if f32_result {
            text.replace("faber_fmod_f64", "faber_fmod_f32")
        } else {
            text
        };
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
        receiver: Operand,
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
        let is32 = receiver.value == Value::Float32;
        let suffix = if is32 { "f32" } else { "f64" };
        let float_value = if is32 { Value::Float32 } else { Value::Float };
        if name == "potentia" {
            return self.float_power(receiver, suffix, float_value, args);
        }
        let helper = match entry.mir_kind {
            IntrinsicMirKind::Collection(MirCollectionOp::Floor) => "faber_floor_i64",
            IntrinsicMirKind::Collection(MirCollectionOp::Ceiling) => "faber_ceil_i64",
            IntrinsicMirKind::Collection(MirCollectionOp::Round) => "faber_nearest_i64",
            IntrinsicMirKind::Collection(MirCollectionOp::Truncate) => "faber_trunc_i64",
            IntrinsicMirKind::Collection(MirCollectionOp::ScalarMath(function)) => {
                return Ok(self.float_math(function, &receiver, is32, float_value));
            }
            IntrinsicMirKind::Collection(MirCollectionOp::Approximate) => {
                return self.float_approx(receiver, args);
            }
            _ => {
                return self.float_value_method(
                    entry.mir_kind,
                    receiver,
                    suffix,
                    float_value,
                    args,
                );
            }
        };
        self.support.need(Helper::Float);
        self.support.need_ints();
        let effect = receiver.effect;
        Ok(Operand::new(format!("{helper}({})", receiver.text), Value::Int).with_effect(effect))
    }

    /// `x.sqrt()`, `x.exp()`, `x.ln()`, `x.log10()`, `x.sin()`, `x.cos()`,
    /// `x.tan()` (and `x.abs()` if the catalog routes it here): the libm
    /// function at the declared width, which the runner's `f32`/`f64` method
    /// calls on this host.
    fn float_math(
        &mut self,
        function: MirMathFunction,
        receiver: &Operand,
        is32: bool,
        float_value: Value,
    ) -> Operand {
        self.support.need(Helper::Float);
        let base = match function {
            MirMathFunction::Abs => "fabs",
            MirMathFunction::Sqrt => "sqrt",
            MirMathFunction::Cbrt => "cbrt",
            MirMathFunction::Exp => "exp",
            MirMathFunction::Ln => "log",
            MirMathFunction::Log10 => "log10",
            MirMathFunction::Sin => "sin",
            MirMathFunction::Cos => "cos",
            MirMathFunction::Tan => "tan",
        };
        let call = if is32 {
            format!("{base}f({})", receiver.text)
        } else {
            format!("{base}({})", receiver.text)
        };
        Operand::new(call, float_value).with_effect(receiver.effect)
    }

    /// `x.approx(y, tolerance)`: `|x − y| <= tolerance`, every operand
    /// promoted to binary64, the runner's `approximate_fractus` three-arg
    /// form.
    fn float_approx(
        &mut self,
        receiver: Operand,
        args: &[HirCallArg],
    ) -> Result<Operand, CodegenError> {
        let [other, tolerance] = args else {
            return Err(unsupported("approximate comparison arity"));
        };
        let other = self.expr(&other.expr)?;
        let tolerance = self.expr(&tolerance.expr)?;
        let numeric = |operand: &Operand| {
            matches!(
                operand.value,
                Value::Int | Value::Exact | Value::Float | Value::Float32
            )
        };
        if !numeric(&other) || !numeric(&tolerance) {
            return Err(unsupported("approximate comparison of a non-numeric value"));
        }
        self.support.need(Helper::Float);
        let effect = receiver.effect || other.effect || tolerance.effect;
        let mut operands = [receiver, other, tolerance];
        let prelude = self.sequence(&mut operands)?;
        let [receiver, other, tolerance] = operands;
        let call = format!(
            "(fabs((double){} - (double){}) <= (double){})",
            receiver.text, other.text, tolerance.text
        );
        Ok(Operand::new(wrap(&prelude, &call), Value::Truth).with_effect(effect))
    }

    /// `x.potentia(n)`: IEEE 754 `pow`, no domain guard (`f64::powf`).
    fn float_power(
        &mut self,
        receiver: Operand,
        suffix: &str,
        float_value: Value,
        args: &[HirCallArg],
    ) -> Result<Operand, CodegenError> {
        let [arg] = args else {
            return Err(unsupported("float potentia without one argument"));
        };
        let exponent = self.expr(&arg.expr)?;
        if !matches!(exponent.value, Value::Float | Value::Float32) {
            return Err(unsupported("float potentia exponent that is not a float"));
        }
        self.support.need(Helper::Float);
        let effect = receiver.effect || exponent.effect;
        let mut operands = [receiver, exponent];
        let prelude = self.sequence(&mut operands)?;
        let [base, exponent] = operands;
        let call = format!("faber_pow_{suffix}({}, {})", base.text, exponent.text);
        Ok(Operand::new(wrap(&prelude, &call), float_value).with_effect(effect))
    }

    /// A float method that yields a float: `absolutum`, `signum`, and the
    /// two-operand `minimus` / `maximus` (IEEE, NaN propagates).
    fn float_value_method(
        &mut self,
        kind: IntrinsicMirKind,
        receiver: Operand,
        suffix: &str,
        float_value: Value,
        args: &[HirCallArg],
    ) -> Result<Operand, CodegenError> {
        self.support.need(Helper::Float);
        let effect = receiver.effect;
        let text = match kind {
            IntrinsicMirKind::Collection(MirCollectionOp::Absolute) => {
                format!("faber_abs_{suffix}({})", receiver.text)
            }
            IntrinsicMirKind::Collection(MirCollectionOp::Signum) => {
                format!("faber_signum_{suffix}({})", receiver.text)
            }
            IntrinsicMirKind::Collection(MirCollectionOp::Minimum | MirCollectionOp::Maximum) => {
                let Some(arg) = args.first() else {
                    return Err(unsupported("float method outside the subset"));
                };
                let helper =
                    if matches!(kind, IntrinsicMirKind::Collection(MirCollectionOp::Minimum)) {
                        "faber_minimum"
                    } else {
                        "faber_maximum"
                    };
                let mut operands = [receiver, self.expr(&arg.expr)?];
                if !matches!(operands[1].value, Value::Float | Value::Float32) {
                    return Err(unsupported("float method argument that is not a float"));
                }
                let prelude = self.sequence(&mut operands)?;
                let [left, right] = operands;
                let effect = effect || right.effect;
                let call = format!("{helper}_{suffix}({}, {})", left.text, right.text);
                return Ok(Operand::new(wrap(&prelude, &call), float_value).with_effect(effect));
            }
            _ => return Err(unsupported("float method outside the subset")),
        };
        Ok(Operand::new(text, float_value).with_effect(effect))
    }
}
