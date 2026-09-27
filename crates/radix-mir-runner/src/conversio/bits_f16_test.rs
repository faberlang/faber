//! Runner pins for the u16 ↔ fractus<f16> Bits carriers (bits-f16 U3):
//! bit-identity is the invariant, including the NaN payload through the
//! width-agnostic `Value::Float` carrier.

use super::*;
use radix_lexer::{Interner, Span};
use radix_mir::{
    MirBlockId, MirFunction, MirFunctionId, MirProgram, MirType, MirValidationContext,
};
use radix_types::{NumericWidth, Primitive, TypeId, TypeTable};

fn eval_convert(value: Value, target: NumericWidth) -> Result<Value, RunnerError> {
    eval_convert_with(value, target, true)
}

fn eval_widen(value: Value) -> Result<Value, RunnerError> {
    eval_convert_with(value, NumericWidth::F32, false)
}

fn eval_convert_with(
    value: Value,
    target: NumericWidth,
    bits_hint: bool,
) -> Result<Value, RunnerError> {
    let mut type_table = TypeTable::new();
    let _target_ty = type_table.sized_numeric(
        if matches!(target, NumericWidth::U16) {
            Primitive::Numerus
        } else {
            Primitive::Fractus
        },
        target,
    );
    let mut interner = Interner::new();
    let bits_sym = interner.intern("Bits");
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![],
        locals: vec![],
        temps: vec![],
        blocks: vec![],
        return_ty: MirType::semantic(TypeId(0)),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    });
    let mut host = crate::host::BufferHost::default();
    let mut runner = crate::Runner::new(&program, &context, &type_table, &interner, &mut host);
    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));
    if matches!(target, NumericWidth::U16) {
        // Source type: these carriers carry no decimal source width, so the
        // type id is the untyped one the conversion path only reads for
        // decimal widening.
        runner.convert_to_numerus_from(value, TypeId(0), &[bits_sym], target, None, Span::default())
    } else if bits_hint {
        runner.convert_to_fractus(value, &[bits_sym], target, None, Span::default())
    } else {
        runner.convert_to_fractus(value, &[], target, None, Span::default())
    }
}

fn u16_to_f16(bits: u16) -> Value {
    eval_convert(Value::UInt(u64::from(bits)), NumericWidth::F16).expect("u16 -> fractus<f16,Bits>")
}

fn f16_to_u16(value: Value) -> u16 {
    let result = eval_convert(value, NumericWidth::U16).expect("fractus<f16> -> numerus<u16,Bits>");
    let Value::UInt(bits) = result else {
        panic!("expected u16 bits carrier, got {result:?}");
    };
    u16::try_from(bits).expect("u16 bits fit")
}

#[test]
fn f16_bits_round_trip_returns_original_bits() {
    for original in [0x3C00, 0x7E00, 0x7C00, 0xFC00, 0x0001] {
        let carrier = u16_to_f16(original);
        let back = f16_to_u16(carrier.clone());
        assert_eq!(back, original, "round trip must return original bits");
    }
}

#[test]
fn f16_nan_payload_survives_width_agnostic_float_carrier() {
    // The intermediate carrier is the width-agnostic `Value::Float` (f64);
    // the 0x7E00 NaN payload must still encode back to 0x7E00 bits.
    let carrier = u16_to_f16(0x7E00);
    let Value::Float(v) = &carrier else {
        panic!("expected Value::Float carrier, got {carrier:?}");
    };
    assert!(v.is_nan(), "carrier must be NaN for the 0x7E00 payload");
    assert_eq!(f16_to_u16(carrier), 0x7E00, "NaN payload bit-identity");
}

#[test]
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn f16_bits_folded_chain_equals_direct_exact_widen() {
    // Folded: u16 -> fractus<f16,Bits> -> fractus<f32>; the f16 value widened
    // exactly to f32 (and f64 in between) must equal the direct exact widen.
    for bits in [0x3C00, 0x7C00, 0xFC00, 0x0001] {
        let folded = u16_to_f16(bits);
        // The fold step `fractus<f16> -> fractus<f32>` is a plain widen — no
        // Bits hint on the second hop.
        let Value::Float(v) =
            eval_widen(folded).expect("fractus<f16> -> fractus<f32> identity fold")
        else {
            panic!("expected float fold result");
        };
        let direct = f64::from(crate::half_to_f32(bits));
        assert_eq!(
            (v as f32).to_bits(),
            (direct as f32).to_bits(),
            "folded chain must equal direct exact widen for {bits:#06x}"
        );
    }
}
