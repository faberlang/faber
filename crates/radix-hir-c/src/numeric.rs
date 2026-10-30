//! Integer-only methods (goal unit C9): `absolutum`, `signum`, `minimus`,
//! `maximus` and `potentia` on a `numerus` (and its sized spellings), the
//! twins of the float methods in `float.rs`.
//!
//! The receiver is an exact value: `abs` of `i64::MIN` is `2^63`, still in the
//! representation cap, and `potentia` is repeated squaring that traps
//! `numerus overflow` like the runner's `faber::exact::pow`. Nothing here
//! folds a constant pair; the frontend does that first, and the runtime call
//! is exact either way.

use crate::Emitter;
use crate::error::unsupported;
use crate::expr::{Operand, Value, wrap};
use crate::helpers::Helper;
use radix_codegen_shared::CodegenError;
use radix_hir::HirCallArg;
use radix_intrinsics::IntrinsicMirKind;
use radix_lexer::Symbol;
use radix_types::{MirCollectionOp, TypeId};

impl Emitter<'_> {
    /// A method call on a `numerus` receiver, resolved through the receiver
    /// intrinsic catalog. Anything else on an integer is rejected by name.
    pub(crate) fn int_method(
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
        if !receiver.is_integer() {
            return Err(unsupported("integer method on a non-integer"));
        }
        if args
            .iter()
            .any(|arg| arg.name.is_some() || arg.spread || arg.marker.is_some())
        {
            return Err(unsupported("named, spread, or marked argument"));
        }
        let effect = receiver.effect;
        match entry.mir_kind {
            IntrinsicMirKind::Collection(MirCollectionOp::Absolute) => {
                self.support.need(Helper::IntMethod);
                Ok(Self::exact_call("faber_abs", &receiver, effect))
            }
            IntrinsicMirKind::Collection(MirCollectionOp::Signum) => {
                self.support.need(Helper::IntMethod);
                Ok(Self::exact_call("faber_sign", &receiver, effect))
            }
            IntrinsicMirKind::Collection(MirCollectionOp::Minimum | MirCollectionOp::Maximum) => {
                let minimum =
                    entry.mir_kind == IntrinsicMirKind::Collection(MirCollectionOp::Minimum);
                self.int_binary_method(receiver, effect, minimum, args)
            }
            _ if name == "potentia" => self.int_power(receiver, effect, args),
            _ => Err(unsupported("integer method outside the subset")),
        }
    }

    /// A one-argument exact helper call whose argument is the receiver.
    fn exact_call(helper: &str, receiver: &Operand, effect: bool) -> Operand {
        Operand::new(format!("{helper}({})", receiver.text), Value::Exact).with_effect(effect)
    }

    /// `x.minimus(y)` / `x.maximus(y)`: two exact operands, sequenced left to
    /// right (`min`/`max` are pure, but an argument may call).
    fn int_binary_method(
        &mut self,
        receiver: Operand,
        effect: bool,
        minimum: bool,
        args: &[HirCallArg],
    ) -> Result<Operand, CodegenError> {
        let [arg] = args else {
            return Err(unsupported("integer method without one argument"));
        };
        let argument = self.expr(&arg.expr)?;
        if !argument.is_integer() {
            return Err(unsupported(
                "integer method argument that is not an integer",
            ));
        }
        self.support.need(Helper::IntMethod);
        let helper = if minimum {
            "faber_min_i128"
        } else {
            "faber_max_i128"
        };
        let mut operands = [receiver, argument];
        let prelude = self.sequence(&mut operands)?;
        let [left, right] = operands;
        let effect = effect || right.effect;
        let call = format!("{helper}({}, {})", left.text, right.text);
        Ok(Operand::new(wrap(&prelude, &call), Value::Exact).with_effect(effect))
    }

    /// `x.potentia(n)`: repeated squaring; a negative exponent aborts, an
    /// overflow traps `numerus overflow`, both as the runner's `pow` does.
    fn int_power(
        &mut self,
        receiver: Operand,
        effect: bool,
        args: &[HirCallArg],
    ) -> Result<Operand, CodegenError> {
        let [arg] = args else {
            return Err(unsupported("integer potentia without one argument"));
        };
        let exponent = self.expr(&arg.expr)?;
        if !exponent.is_integer() {
            return Err(unsupported(
                "integer potentia exponent that is not an integer",
            ));
        }
        self.support.need(Helper::IntMethod);
        let mut operands = [receiver, exponent];
        let prelude = self.sequence(&mut operands)?;
        let [base, exponent] = operands;
        let effect = effect || exponent.effect;
        let call = format!("faber_pow({}, {})", base.text, exponent.text);
        Ok(Operand::new(wrap(&prelude, &call), Value::Exact).with_effect(effect))
    }
}
