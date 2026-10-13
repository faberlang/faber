//! Runner pins for `octeti ↦ lista<E> via Le|Be` and its inverse
//! `lista<E> ↦ octeti via Le|Be`: `octeti` is `lista<b8>`, so
//! both directions are the bits-list width change.

use crate::host::BufferHost;
use crate::{Runner, RunnerError, Value};
use radix_lexer::{Interner, Span};
use radix_mir::{MirConversion, MirConversionFlavor, MirProgram, MirType, MirValidationContext};
use radix_types::{BitsWidth, NumericWidth, Primitive, Type, TypeTable};
use std::cell::RefCell;
use std::rc::Rc;

/// Every element type the tensor form admits that has a byte order.
const WIDE_ELEMENTS: [NumericWidth; 8] = [
    NumericWidth::U16,
    NumericWidth::I16,
    NumericWidth::U32,
    NumericWidth::I32,
    NumericWidth::U64,
    NumericWidth::I64,
    NumericWidth::F32,
    NumericWidth::F64,
];

fn element_type(types: &mut TypeTable, width: NumericWidth) -> radix_types::TypeId {
    let primitive = match width {
        NumericWidth::F32 | NumericWidth::F64 => Primitive::Fractus,
        _ => Primitive::Numerus,
    };
    types.sized_numeric(primitive, width)
}

fn byte_list(bytes: &[u8]) -> Value {
    Value::Array(Rc::new(RefCell::new(
        bytes.iter().map(|b| Value::UInt(u64::from(*b))).collect(),
    )))
}

fn list_bytes(value: &Value) -> Vec<u8> {
    let Value::Array(cell) = value else {
        panic!("expected a list, got {value:?}");
    };
    cell.borrow()
        .iter()
        .map(|item| match item {
            Value::UInt(b) => u8::try_from(*b).expect("byte pattern"),
            other => panic!("expected a byte pattern, got {other:?}"),
        })
        .collect()
}

/// `bytes ↦ lista<element> via hint`.
fn bytes_to_list(bytes: &[u8], element: NumericWidth, hint: &str) -> Result<Value, RunnerError> {
    let mut types = TypeTable::new();
    let b8 = types.intern(Type::Bits(BitsWidth::B8));
    let source = types.intern(Type::Array(b8));
    let element = element_type(&mut types, element);
    let target = types.intern(Type::Array(element));
    convert(&types, byte_list(bytes), source, target, hint)
}

/// `lista<element> ↦ octeti via hint`.
fn list_to_bytes(list: Value, element: NumericWidth, hint: &str) -> Result<Value, RunnerError> {
    let mut types = TypeTable::new();
    let b8 = types.intern(Type::Bits(BitsWidth::B8));
    let element = element_type(&mut types, element);
    let source = types.intern(Type::Array(element));
    let target = types.intern(Type::Array(b8));
    convert(&types, list, source, target, hint)
}

fn convert(
    types: &TypeTable,
    value: Value,
    source: radix_types::TypeId,
    target: radix_types::TypeId,
    hint: &str,
) -> Result<Value, RunnerError> {
    let mut interner = Interner::new();
    let param = interner.intern(hint);
    let context = MirValidationContext::new(types);
    let program = MirProgram::new();
    let mut host = BufferHost::default();
    let mut runner = Runner::new(&program, &context, types, &interner, &mut host);
    runner.eval_runtime_convert(
        &MirConversion {
            store: None,
            flavor: MirConversionFlavor::Runtime,
            source_ty: MirType::semantic(source),
            target_ty: MirType::semantic(target),
            params: vec![param],
            recovery: None,
            struct_defaults: vec![],
        },
        value,
        Span::default(),
    )
}

fn elements(value: &Value) -> Vec<Value> {
    let Value::Array(cell) = value else {
        panic!("expected a list, got {value:?}");
    };
    cell.borrow().clone()
}

#[test]
fn octeti_to_lista_reads_each_element_in_the_hinted_byte_order() {
    let cases: [(NumericWidth, &str, &[u8], Value); 10] = [
        (NumericWidth::U16, "Le", &[0x01, 0x02], Value::Int(0x0201)),
        (NumericWidth::U16, "Be", &[0x01, 0x02], Value::Int(0x0102)),
        (NumericWidth::I16, "Le", &[0xff, 0xff], Value::Int(-1)),
        (
            NumericWidth::U32,
            "Le",
            &[0xff; 4],
            Value::Int(4_294_967_295),
        ),
        (
            NumericWidth::I32,
            "Be",
            &[0x80, 0, 0, 0],
            Value::Int(i64::from(i32::MIN)),
        ),
        (NumericWidth::U64, "Le", &[0xff; 8], Value::UInt(u64::MAX)),
        (NumericWidth::I64, "Le", &[0xff; 8], Value::Int(-1)),
        (
            NumericWidth::F32,
            "Le",
            &[0x00, 0x00, 0x80, 0x3f],
            Value::Float(1.0),
        ),
        (
            NumericWidth::F32,
            "Be",
            &[0x3f, 0x80, 0x00, 0x00],
            Value::Float(1.0),
        ),
        (
            NumericWidth::F64,
            "Be",
            &[0x3f, 0xf0, 0, 0, 0, 0, 0, 0],
            Value::Float(1.0),
        ),
    ];
    for (element, hint, bytes, expected) in cases {
        let list = bytes_to_list(bytes, element, hint).expect("octeti to lista");
        assert_eq!(elements(&list), vec![expected], "{element:?} via {hint}");
    }
}

#[test]
fn octeti_to_lista_element_i_comes_from_bytes_i_times_width() {
    let bytes: Vec<u8> = (0..16).collect();
    let le = bytes_to_list(&bytes, NumericWidth::U32, "Le").expect("le");
    assert_eq!(
        elements(&le),
        vec![
            Value::Int(0x0302_0100),
            Value::Int(0x0706_0504),
            Value::Int(0x0b0a_0908),
            Value::Int(0x0f0e_0d0c),
        ]
    );
    let be = bytes_to_list(&bytes, NumericWidth::U32, "Be").expect("be");
    assert_eq!(
        elements(&be),
        vec![
            Value::Int(0x0001_0203),
            Value::Int(0x0405_0607),
            Value::Int(0x0809_0a0b),
            Value::Int(0x0c0d_0e0f),
        ]
    );
}

#[test]
fn octeti_lista_round_trip_is_bit_identical_in_both_orders() {
    // A deterministic spread of every byte value, 64 bytes (a multiple of
    // every element width).
    let mut state = 0x9e37_79b9_u32;
    let bytes: Vec<u8> = (0..64)
        .map(|_| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 24) as u8
        })
        .collect();
    for element in WIDE_ELEMENTS {
        for hint in ["Le", "Be"] {
            let list = bytes_to_list(&bytes, element, hint).expect("octeti to lista");
            let back = list_to_bytes(list, element, hint).expect("lista to octeti");
            assert_eq!(list_bytes(&back), bytes, "{element:?} via {hint}");
        }
    }
}

#[test]
fn octeti_lista_round_trip_keeps_float_nan_payloads_and_signed_zero() {
    // f32: signaling NaN, quiet NaN with payload, -0.0, smallest subnormal,
    // negative infinity. A hardware widen to f64 quiets a signaling NaN, so
    // these must not take that route.
    let f32_bits: [u32; 6] = [
        0x7f80_0001,
        0x7fc0_0001,
        0xffa0_0000,
        0x8000_0000,
        0x0000_0001,
        0xff80_0000,
    ];
    let f64_bits: [u64; 6] = [
        0x7ff0_0000_0000_0001,
        0x7ff8_0000_0000_0001,
        0xfff4_0000_0000_0000,
        0x8000_0000_0000_0000,
        0x0000_0000_0000_0001,
        0xfff0_0000_0000_0000,
    ];
    for hint in ["Le", "Be"] {
        let f32_bytes: Vec<u8> = f32_bits
            .iter()
            .flat_map(|bits| {
                if hint == "Le" {
                    bits.to_le_bytes()
                } else {
                    bits.to_be_bytes()
                }
            })
            .collect();
        let list = bytes_to_list(&f32_bytes, NumericWidth::F32, hint).expect("f32 list");
        let back = list_to_bytes(list, NumericWidth::F32, hint).expect("f32 octeti");
        assert_eq!(list_bytes(&back), f32_bytes, "f32 via {hint}");

        let f64_bytes: Vec<u8> = f64_bits
            .iter()
            .flat_map(|bits| {
                if hint == "Le" {
                    bits.to_le_bytes()
                } else {
                    bits.to_be_bytes()
                }
            })
            .collect();
        let list = bytes_to_list(&f64_bytes, NumericWidth::F64, hint).expect("f64 list");
        let back = list_to_bytes(list, NumericWidth::F64, hint).expect("f64 octeti");
        assert_eq!(list_bytes(&back), f64_bytes, "f64 via {hint}");
    }
}

#[test]
fn octeti_to_lista_non_multiple_length_is_a_recoverable_named_failure() {
    for (element, len) in [
        (NumericWidth::U16, 3),
        (NumericWidth::I32, 7),
        (NumericWidth::U64, 12),
        (NumericWidth::F32, 5),
        (NumericWidth::F64, 9),
    ] {
        for hint in ["Le", "Be"] {
            let err = bytes_to_list(&vec![0_u8; len], element, hint)
                .expect_err("a ragged byte length must fail");
            assert_eq!(err.code, "runner_conversion", "{element:?} via {hint}");
            assert_eq!(err.issue, "runner_bits_list_length_mismatch");
        }
    }
}

#[test]
fn octeti_lista_empty_converts_both_ways() {
    for element in WIDE_ELEMENTS {
        let list = bytes_to_list(&[], element, "Le").expect("empty octeti");
        assert!(elements(&list).is_empty());
        let back = list_to_bytes(list, element, "Be").expect("empty lista");
        assert!(list_bytes(&back).is_empty());
    }
}
