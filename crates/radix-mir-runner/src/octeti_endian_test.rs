//! Runner pins for octeti window ↦ sized int via Be/Le.

use super::*;
use radix_lexer::{Interner, Span};
use radix_mir::{
    MirBlockId, MirFunction, MirFunctionId, MirProgram, MirType, MirValidationContext,
};
use radix_types::{IndexExpr, NumericWidth, Primitive, TypeId, TypeTable};

fn eval_bytes(bytes: Vec<u8>, hint: &str, width: NumericWidth) -> Result<Value, RunnerError> {
    let mut type_table = TypeTable::new();
    let _target_ty = type_table.sized_numeric(Primitive::Numerus, width);
    let mut interner = Interner::new();
    let hint_sym = interner.intern(hint);
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
    // Source type: the octeti window conversion reads its endian hint from
    // `params`, not from the type id.
    runner.convert_to_numerus_from(
        Value::Octeti(bytes),
        TypeId(0),
        &[hint_sym],
        width,
        None,
        Span::default(),
    )
}

fn eval_float_bytes(bytes: Vec<u8>, hint: &str, width: NumericWidth) -> Result<Value, RunnerError> {
    let mut type_table = TypeTable::new();
    let _target_ty = type_table.sized_numeric(Primitive::Fractus, width);
    let mut interner = Interner::new();
    let hint_sym = interner.intern(hint);
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
    runner.convert_to_fractus(
        Value::Octeti(bytes),
        &[hint_sym],
        width,
        None,
        Span::default(),
    )
}

#[test]
fn octeti_endian_be_u32_is_deadbeef() {
    let result = eval_bytes(vec![0xde, 0xad, 0xbe, 0xef], "Be", NumericWidth::U32).expect("be u32");
    assert_eq!(result, Value::UInt(0xDEAD_BEEF));
}

#[test]
fn octeti_endian_le_u32_is_efbeadde() {
    let result = eval_bytes(vec![0xde, 0xad, 0xbe, 0xef], "Le", NumericWidth::U32).expect("le u32");
    assert_eq!(result, Value::UInt(0xEFBE_ADDE));
}

#[test]
fn octeti_endian_short_window_fails() {
    let err = eval_bytes(vec![0xde, 0xad, 0xbe], "Be", NumericWidth::U32)
        .expect_err("3-byte source must fail");
    assert_eq!(
        err.issue, "runner_octeti_endian_width_mismatch",
        "expected width mismatch, got: {err:?}"
    );
}

#[test]
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn octeti_endian_le_f32_preserves_bits() {
    let result =
        eval_float_bytes(vec![0x00, 0x00, 0x80, 0x3f], "Le", NumericWidth::F32).expect("le f32");
    let Value::Float(value) = result else {
        panic!("expected float result");
    };
    assert_eq!((value as f32).to_bits(), 0x3f80_0000);
}

#[test]
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn octeti_endian_le_f32_preserves_noncanonical_bits() {
    let result =
        eval_float_bytes(vec![0xde, 0xad, 0xbe, 0xef], "Le", NumericWidth::F32).expect("le f32");
    let Value::Float(value) = result else {
        panic!("expected float result");
    };
    assert_eq!((value as f32).to_bits(), 0xefbe_adde);
}

#[test]
fn octeti_endian_be_f64_decodes() {
    let result = eval_float_bytes(
        vec![0x3f, 0xf0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        "Be",
        NumericWidth::F64,
    )
    .expect("be f64");
    let Value::Float(value) = result else {
        panic!("expected float result");
    };
    assert_eq!(value.to_bits(), 1.0f64.to_bits());
}

/// A throwaway function frame for the value-level round-trip helpers.
fn throwaway_program() -> MirProgram {
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
    program
}

/// Byte width of one endian scalar (`W ↦ octeti<N>` capacity).
fn endian_scalar_bytes(width: NumericWidth) -> u64 {
    match width {
        NumericWidth::I16 | NumericWidth::U16 => 2,
        NumericWidth::I32 | NumericWidth::U32 | NumericWidth::F32 => 4,
        NumericWidth::I64 | NumericWidth::U64 | NumericWidth::F64 => 8,
        _ => panic!("not an endian scalar width: {width:?}"),
    }
}

/// `W ↦ octeti<N> via hint`: encode one scalar as its own bytes.
fn encode_scalar(value: Value, width: NumericWidth, hint: &str) -> Vec<u8> {
    let mut type_table = TypeTable::new();
    let source_ty = match width {
        NumericWidth::F32 | NumericWidth::F64 => {
            type_table.sized_numeric(Primitive::Fractus, width)
        }
        _ => type_table.sized_numeric(Primitive::Numerus, width),
    };
    let capacity = type_table.intern_index(IndexExpr::Literal(endian_scalar_bytes(width)));
    let mut interner = Interner::new();
    let hint_sym = interner.intern(hint);
    let context = MirValidationContext::new(&type_table);
    let program = throwaway_program();
    let mut host = crate::host::BufferHost::default();
    let mut runner = crate::Runner::new(&program, &context, &type_table, &interner, &mut host);
    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));
    let encoded = runner
        .convert_scalar_to_bounded_octeti(value, source_ty, capacity, &[hint_sym], Span::default())
        .expect("scalar write to bounded octeti");
    let Value::Octeti(bytes) = encoded else {
        panic!("expected octeti encode, got {encoded:?}");
    };
    bytes
}

/// `octeti<N> via hint ↦ W`: decode bytes with the octeti scalar reader.
fn decode_scalar(bytes: Vec<u8>, width: NumericWidth, hint: &str) -> Value {
    let type_table = TypeTable::new();
    let mut interner = Interner::new();
    let hint_sym = interner.intern(hint);
    let context = MirValidationContext::new(&type_table);
    let program = throwaway_program();
    let mut host = crate::host::BufferHost::default();
    let mut runner = crate::Runner::new(&program, &context, &type_table, &interner, &mut host);
    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));
    let decoded = match width {
        NumericWidth::F32 | NumericWidth::F64 => runner.convert_to_fractus(
            Value::Octeti(bytes),
            &[hint_sym],
            width,
            None,
            Span::default(),
        ),
        _ => runner.convert_to_numerus_from(
            Value::Octeti(bytes),
            TypeId(0),
            &[hint_sym],
            width,
            None,
            Span::default(),
        ),
    };
    decoded.expect("octeti read to scalar")
}

/// Float bit pattern → carrier → bytes → carrier → float bit pattern.
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn round_trip_float(bits_in: u64, width: NumericWidth, hint: &str) -> u64 {
    let value = match width {
        NumericWidth::F32 => f64::from(f32::from_bits(
            u32::try_from(bits_in).expect("f32 pattern fits 32 bits"),
        )),
        NumericWidth::F64 => f64::from_bits(bits_in),
        _ => panic!("not a float width: {width:?}"),
    };
    let encoded = encode_scalar(Value::Float(value), width, hint);
    let Value::Float(decoded) = decode_scalar(encoded, width, hint) else {
        panic!("expected float decode");
    };
    match width {
        NumericWidth::F32 => u64::from((decoded as f32).to_bits()),
        NumericWidth::F64 => decoded.to_bits(),
        _ => panic!("not a float width: {width:?}"),
    }
}

/// Integer → bytes → integer, with the decoded value widened to `i128`.
fn round_trip_int(value: i128, width: NumericWidth, hint: &str) -> i128 {
    let carrier = match width {
        NumericWidth::I16 | NumericWidth::I32 | NumericWidth::I64 => {
            Value::Int(i64::try_from(value).expect("signed pattern fits i64"))
        }
        NumericWidth::U16 | NumericWidth::U32 | NumericWidth::U64 => {
            Value::UInt(u64::try_from(value).expect("unsigned pattern fits u64"))
        }
        _ => panic!("not an integer width: {width:?}"),
    };
    match decode_scalar(encode_scalar(carrier, width, hint), width, hint) {
        Value::Int(decoded) => i128::from(decoded),
        Value::UInt(decoded) => i128::from(decoded),
        other => panic!("expected integer decode, got {other:?}"),
    }
}

#[test]
fn scalar_octeti_roundtrip_f32_bit_patterns() {
    let cases = [
        (0x0000_0000_u64, "+0.0"),
        (0x8000_0000, "-0.0"),
        (0x7f80_0000, "+inf"),
        (0xff80_0000, "-inf"),
        (0x0000_0001, "smallest subnormal"),
        (0x007f_ffff, "largest subnormal"),
        (0x7f7f_ffff, "f32::MAX"),
        (0x7fc0_0001, "quiet NaN payload"),
        (0xffc0_1234, "negative quiet NaN"),
    ];
    for hint in ["Le", "Be"] {
        for (bits, name) in cases {
            assert_eq!(
                round_trip_float(bits, NumericWidth::F32, hint),
                bits,
                "f32 {name} via {hint}"
            );
        }
    }
}

#[test]
fn scalar_octeti_roundtrip_f64_bit_patterns() {
    let cases = [
        (0x0000_0000_0000_0000_u64, "+0.0"),
        (0x8000_0000_0000_0000, "-0.0"),
        (0x7ff0_0000_0000_0000, "+inf"),
        (0xfff0_0000_0000_0000, "-inf"),
        (0x0000_0000_0000_0001, "smallest subnormal"),
        (0x7fef_ffff_ffff_ffff, "f64::MAX"),
        (0x7ff8_0000_0000_0001, "quiet NaN payload"),
        (0xfff8_0000_dead_beef, "negative quiet NaN"),
    ];
    for hint in ["Le", "Be"] {
        for (bits, name) in cases {
            assert_eq!(
                round_trip_float(bits, NumericWidth::F64, hint),
                bits,
                "f64 {name} via {hint}"
            );
        }
    }
}

#[test]
fn scalar_octeti_roundtrip_integer_extremes() {
    let cases = [
        (i128::from(i16::MIN), NumericWidth::I16, "i16::MIN"),
        (i128::from(i16::MAX), NumericWidth::I16, "i16::MAX"),
        (i128::from(u16::MAX), NumericWidth::U16, "u16::MAX"),
        (i128::from(i32::MIN), NumericWidth::I32, "i32::MIN"),
        (i128::from(i32::MAX), NumericWidth::I32, "i32::MAX"),
        (i128::from(u32::MAX), NumericWidth::U32, "u32::MAX"),
        (i128::from(i64::MIN), NumericWidth::I64, "i64::MIN"),
        (i128::from(i64::MAX), NumericWidth::I64, "i64::MAX"),
        (i128::from(u64::MAX), NumericWidth::U64, "u64::MAX"),
    ];
    for hint in ["Le", "Be"] {
        for (value, width, name) in cases {
            assert_eq!(
                round_trip_int(value, width, hint),
                value,
                "{name} via {hint}"
            );
        }
    }
}

#[test]
fn scalar_octeti_roundtrip_byte_images() {
    assert_eq!(
        encode_scalar(Value::UInt(0x0102), NumericWidth::U16, "Le"),
        vec![0x02, 0x01],
        "u16 0x0102 Le"
    );
    assert_eq!(
        encode_scalar(Value::UInt(0x0102), NumericWidth::U16, "Be"),
        vec![0x01, 0x02],
        "u16 0x0102 Be"
    );
    assert_eq!(
        encode_scalar(Value::Float(1.0), NumericWidth::F32, "Le"),
        vec![0x00, 0x00, 0x80, 0x3f],
        "f32 1.0 Le"
    );
    assert_eq!(
        encode_scalar(Value::Float(-2.0), NumericWidth::F64, "Be"),
        vec![0xc0, 0, 0, 0, 0, 0, 0, 0],
        "f64 -2.0 Be"
    );
}
