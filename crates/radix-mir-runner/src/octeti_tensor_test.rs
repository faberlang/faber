//! Runner pins for `octeti ↦ tensor<E, [shape]> via Le|Be`.
//!
//! Identity is asserted by `code` and `issue`, never by message text.

use super::*;
use crate::dense::DenseKind;
use crate::value::TensorValue;
use radix_lexer::{Interner, Span};
use radix_mir::{
    MirBlockId, MirConstant, MirFunction, MirFunctionId, MirOperand, MirProgram, MirType,
    MirValidationContext,
};
use radix_types::{DefId, IndexExpr, NumericWidth, Primitive, RuntimeExtentId, TypeId, TypeTable};

/// Run one `octeti ↦ tensor<element, dims> via hint` conversion.
fn convert(
    bytes: Vec<u8>,
    element: (Primitive, NumericWidth),
    dims: &[u64],
    hint: &str,
    recovery: Option<&MirOperand>,
) -> Result<Value, RunnerError> {
    let extents: Vec<_> = dims.iter().map(|dim| IndexExpr::Literal(*dim)).collect();
    convert_extents(bytes, element, extents, None, hint, recovery)
}

/// Run the conversion over a target whose extents are `extents`, with the
/// evaluated shape operand `shape` a run-time extent needs.
fn convert_extents(
    bytes: Vec<u8>,
    element: (Primitive, NumericWidth),
    extents: Vec<IndexExpr>,
    shape: Option<Vec<i64>>,
    hint: &str,
    recovery: Option<&MirOperand>,
) -> Result<Value, RunnerError> {
    let mut types = TypeTable::new();
    let element_ty = types.sized_numeric(element.0, element.1);
    let dim_ids: Vec<_> = extents
        .into_iter()
        .map(|extent| types.intern_index(extent))
        .collect();
    let shape_index = types.intern_index(IndexExpr::Tuple(dim_ids));
    let target = types.tensor_with_shape(element_ty, shape_index);
    let mut interner = Interner::new();
    let hint_sym = interner.intern(hint);
    let context = MirValidationContext::new(&types);
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
    let mut runner = crate::Runner::new(&program, &context, &types, &interner, &mut host);
    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));
    runner.convert_octeti_to_tensor(bytes, target, shape, &[hint_sym], recovery, Span::default())
}

const F32: (Primitive, NumericWidth) = (Primitive::Fractus, NumericWidth::F32);

fn tensor(value: Value) -> Rc<RefCell<TensorValue>> {
    let Value::Tensor(tensor) = value else {
        panic!("expected a tensor, got {value:?}");
    };
    tensor
}

fn f32_bits(value: Value) -> Vec<u32> {
    let tensor = tensor(value);
    let tensor = tensor.borrow();
    tensor
        .data
        .with_slice::<f32, _>(|items| items.iter().map(|v| v.to_bits()).collect())
        .expect("f32 storage")
}

fn le_bytes_of(bits: &[u32]) -> Vec<u8> {
    bits.iter().flat_map(|b| b.to_le_bytes()).collect()
}

/// A `[2, size n]` target: the second extent is a binding the type table
/// cannot resolve, so the decode reads it from the shape operand.
fn runtime_extents() -> Vec<IndexExpr> {
    vec![
        IndexExpr::Literal(2),
        IndexExpr::Runtime(RuntimeExtentId::ScalarBinding(DefId(7))),
    ]
}

#[test]
fn octeti_tensor_runtime_extent_reads_the_shape_operand() {
    let bits: Vec<u32> = (0..6).map(|i| 0x4000_0000 + i).collect();
    let value = convert_extents(
        le_bytes_of(&bits),
        F32,
        runtime_extents(),
        Some(vec![2, 3]),
        "Le",
        None,
    )
    .expect("run-time extent decode");
    let t = tensor(value);
    assert_eq!(t.borrow().shape, vec![2, 3]);
    assert_eq!(t.borrow().data.kind(), Some(DenseKind::F32));
    assert_eq!(f32_bits(Value::Tensor(t)), bits);
}

#[test]
fn octeti_tensor_runtime_extent_length_mismatch_is_recoverable() {
    // 24 bytes against a [2, 2] shape operand: 16 bytes expected.
    let err = convert_extents(
        vec![0_u8; 24],
        F32,
        runtime_extents(),
        Some(vec![2, 2]),
        "Le",
        None,
    )
    .expect_err("wrong length");
    assert_eq!(err.code, "runner_conversion");
    assert_eq!(err.issue, "runner_octeti_tensor_length_mismatch");
    let recovery = MirOperand::Constant(MirConstant::Int(42));
    let value = convert_extents(
        vec![0_u8; 24],
        F32,
        runtime_extents(),
        Some(vec![2, 2]),
        "Le",
        Some(&recovery),
    )
    .expect("recovered");
    assert_eq!(value, Value::Int(42));
}

#[test]
fn octeti_tensor_runtime_extent_without_shape_operand_is_internal() {
    let err = convert_extents(vec![0_u8; 24], F32, runtime_extents(), None, "Le", None)
        .expect_err("no shape operand");
    assert_eq!(err.code, "runner_internal");
    assert_eq!(err.issue, "octeti_tensor_shape_operand_missing");
}

#[test]
fn octeti_tensor_f32_le_is_bit_exact() {
    // +1.5, quiet NaN with payload, -0.0, smallest subnormal, +inf, -inf.
    let bits = [
        0x3fc0_0000_u32,
        0x7fc0_1234,
        0x8000_0000,
        0x0000_0001,
        0x7f80_0000,
        0xff80_0000,
    ];
    let value = convert(le_bytes_of(&bits), F32, &[6], "Le", None).expect("le f32");
    assert_eq!(f32_bits(value), bits);
}

#[test]
fn octeti_tensor_f32_kind_and_shape() {
    let value = convert(le_bytes_of(&[0, 0]), F32, &[2], "Le", None).expect("le f32");
    let tensor = tensor(value);
    let tensor = tensor.borrow();
    assert_eq!(tensor.data.kind(), Some(DenseKind::F32));
    assert_eq!(tensor.shape, vec![2]);
}

#[test]
fn octeti_tensor_be_is_byte_swapped_le() {
    let le = [0x3f80_0000_u32, 0x4000_0000, 0x7fc0_1234];
    let be_bytes: Vec<u8> = le.iter().flat_map(|b| b.to_be_bytes()).collect();
    let value = convert(be_bytes.clone(), F32, &[3], "Be", None).expect("be f32");
    assert_eq!(f32_bits(value), le);
    // The same bytes read as Le give each element byte-swapped.
    let value = convert(be_bytes, F32, &[3], "Le", None).expect("le f32");
    let swapped: Vec<u32> = le.iter().map(|b| b.swap_bytes()).collect();
    assert_eq!(f32_bits(value), swapped);
}

#[test]
fn octeti_tensor_dense_kinds() {
    let cases: [((Primitive, NumericWidth), usize, DenseKind); 6] = [
        ((Primitive::Fractus, NumericWidth::F64), 8, DenseKind::F64),
        ((Primitive::Numerus, NumericWidth::I32), 4, DenseKind::I32),
        ((Primitive::Numerus, NumericWidth::U32), 4, DenseKind::U32),
        ((Primitive::Numerus, NumericWidth::I64), 8, DenseKind::I64),
        ((Primitive::Numerus, NumericWidth::U64), 8, DenseKind::U64),
        ((Primitive::Numerus, NumericWidth::U8), 1, DenseKind::U8),
    ];
    for (element, width, kind) in cases {
        let value = convert(vec![1; width * 2], element, &[2], "Le", None).expect("decode");
        assert_eq!(tensor(value).borrow().data.kind(), Some(kind), "{kind:?}");
    }
}

#[test]
fn octeti_tensor_integer_values_and_endianness() {
    let value = convert(
        vec![0x78, 0x56, 0x34, 0x12, 0xff, 0xff, 0xff, 0xff],
        (Primitive::Numerus, NumericWidth::I32),
        &[2],
        "Le",
        None,
    )
    .expect("le i32");
    let tensor = tensor(value);
    assert_eq!(tensor.borrow().data.get(0), Some(Value::Int(0x1234_5678)));
    assert_eq!(tensor.borrow().data.get(1), Some(Value::Int(-1)));
    let value = convert(
        vec![0x12, 0x34, 0x56, 0x78],
        (Primitive::Numerus, NumericWidth::U32),
        &[1],
        "Be",
        None,
    )
    .expect("be u32");
    assert_eq!(
        tensor_first(&value),
        Value::Int(0x1234_5678),
        "u32 reads as Int"
    );
    let value = convert(
        vec![0xff; 8],
        (Primitive::Numerus, NumericWidth::U64),
        &[1],
        "Le",
        None,
    )
    .expect("u64");
    assert_eq!(tensor_first(&value), Value::UInt(u64::MAX));
}

fn tensor_first(value: &Value) -> Value {
    let Value::Tensor(tensor) = value else {
        panic!("expected a tensor");
    };
    tensor.borrow().data.get(0).expect("first element")
}

#[test]
fn octeti_tensor_boxed_small_widths() {
    let i8s = convert(
        vec![0x01, 0xff, 0x80],
        (Primitive::Numerus, NumericWidth::I8),
        &[3],
        "Le",
        None,
    )
    .expect("i8");
    let t = tensor(i8s);
    assert_eq!(t.borrow().data.kind(), None);
    assert_eq!(
        t.borrow().data.to_values(),
        vec![Value::Int(1), Value::Int(-1), Value::Int(-128)]
    );
    let i16s = convert(
        vec![0xff, 0xff, 0x00, 0x80],
        (Primitive::Numerus, NumericWidth::I16),
        &[2],
        "Le",
        None,
    )
    .expect("i16");
    assert_eq!(
        tensor(i16s).borrow().data.to_values(),
        vec![Value::Int(-1), Value::Int(-32768)]
    );
    let u16s = convert(
        vec![0x12, 0x34, 0xff, 0xfe],
        (Primitive::Numerus, NumericWidth::U16),
        &[2],
        "Be",
        None,
    )
    .expect("u16");
    assert_eq!(
        tensor(u16s).borrow().data.to_values(),
        vec![Value::Int(0x1234), Value::Int(0xfffe)]
    );
}

#[test]
fn octeti_tensor_two_d_is_row_major() {
    let bits: Vec<u32> = (0..6).map(|i| 0x4000_0000 + i).collect();
    let bytes = le_bytes_of(&bits);
    let value = convert(bytes.clone(), F32, &[2, 3], "Le", None).expect("2-d");
    let t = tensor(value);
    assert_eq!(t.borrow().shape, vec![2, 3]);
    // Element [1, 2] is flat index 5 = bytes 20..24.
    let expected = f32::from_le_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    let t = t.borrow();
    let (row, col, cols) = (1_usize, 2_usize, 3_usize);
    let at = t
        .data
        .with_slice::<f32, _>(|items| items[row * cols + col])
        .expect("f32");
    assert_eq!(at.to_bits(), expected.to_bits());
    assert_eq!(at.to_bits(), bits[5]);
}

#[test]
fn octeti_tensor_length_mismatch_is_recoverable_conversion() {
    for bytes in [vec![0_u8; 7], vec![0_u8; 9], Vec::new()] {
        let err = convert(bytes, F32, &[2], "Le", None).expect_err("wrong length");
        assert_eq!(err.code, "runner_conversion");
        assert_eq!(err.issue, "runner_octeti_tensor_length_mismatch");
    }
}

#[test]
fn octeti_tensor_length_mismatch_recovers_with_default() {
    let recovery = MirOperand::Constant(MirConstant::Int(42));
    let value = convert(vec![0_u8; 7], F32, &[2], "Le", Some(&recovery)).expect("recovered");
    assert_eq!(value, Value::Int(42));
    // The success path never evaluates the recovery.
    let ok = convert(le_bytes_of(&[0, 0]), F32, &[2], "Le", Some(&recovery)).expect("ok");
    assert!(matches!(ok, Value::Tensor(_)));
}

#[test]
fn octeti_tensor_empty_shape_axis_accepts_empty_bytes() {
    let value = convert(Vec::new(), F32, &[0, 4], "Le", None).expect("empty");
    assert_eq!(tensor(value).borrow().data.len(), 0);
}

#[test]
fn octeti_tensor_large_case_is_a_bulk_pass() {
    // token_embd-sized: 49152 x 960 f32 = 188,743,680 bytes of a byte pattern.
    let rows = 49_152_u64;
    let cols = 960_u64;
    let n = usize::try_from(rows * cols * 4).unwrap();
    let bytes: Vec<u8> = (0..n).map(|i| (i % 251) as u8).collect();
    let expected_first = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let expected_last =
        u32::from_le_bytes([bytes[n - 4], bytes[n - 3], bytes[n - 2], bytes[n - 1]]);
    let start = std::time::Instant::now();
    let value = convert(bytes, F32, &[rows, cols], "Le", None).expect("large");
    let elapsed = start.elapsed();
    println!("octeti_tensor large case ({n} bytes): {elapsed:?}");
    let t = tensor(value);
    let t = t.borrow();
    assert_eq!(t.shape, vec![49_152, 960]);
    let (first, last) = t
        .data
        .with_slice::<f32, _>(|items| (items[0].to_bits(), items[items.len() - 1].to_bits()))
        .expect("f32");
    assert_eq!(first, expected_first);
    assert_eq!(last, expected_last);
}

/// Run `bytes ↦ tensor via hint` then `tensor ↦ octeti via hint` in one
/// runner and return the round-tripped bytes.
fn round_trip(
    bytes: Vec<u8>,
    element: (Primitive, NumericWidth),
    dims: &[u64],
    hint: &str,
) -> Vec<u8> {
    let mut types = TypeTable::new();
    let element_ty = types.sized_numeric(element.0, element.1);
    let dim_ids: Vec<_> = dims
        .iter()
        .map(|dim| types.intern_index(IndexExpr::Literal(*dim)))
        .collect();
    let shape = types.intern_index(IndexExpr::Tuple(dim_ids));
    let tensor_ty = types.tensor_with_shape(element_ty, shape);
    let mut interner = Interner::new();
    let hint_sym = interner.intern(hint);
    let context = MirValidationContext::new(&types);
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
    let mut runner = crate::Runner::new(&program, &context, &types, &interner, &mut host);
    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));
    let decoded = runner
        .convert_octeti_to_tensor(bytes, tensor_ty, None, &[hint_sym], None, Span::default())
        .expect("octeti to tensor");
    let tensor = tensor(decoded);
    let encoded = runner
        .convert_tensor_to_octeti(&tensor.borrow(), tensor_ty, &[hint_sym], Span::default())
        .expect("tensor to octeti");
    let Value::Octeti(bytes) = encoded else {
        panic!("expected octeti, got {encoded:?}");
    };
    bytes
}

#[test]
fn tensor_octeti_round_trips_bit_for_bit_for_every_element_and_order() {
    // 2 x 2 grid of every element type; the byte pattern includes f32/f64
    // NaN payload bytes, -0.0, and subnormal-shaped bit patterns.
    let elements = [
        (Primitive::Numerus, NumericWidth::U8, 1),
        (Primitive::Numerus, NumericWidth::I8, 1),
        (Primitive::Numerus, NumericWidth::I16, 2),
        (Primitive::Numerus, NumericWidth::U16, 2),
        (Primitive::Numerus, NumericWidth::I32, 4),
        (Primitive::Numerus, NumericWidth::U32, 4),
        (Primitive::Numerus, NumericWidth::I64, 8),
        (Primitive::Numerus, NumericWidth::U64, 8),
        (Primitive::Fractus, NumericWidth::F32, 4),
        (Primitive::Fractus, NumericWidth::F64, 8),
    ];
    for (primitive, width, bytes_per) in elements {
        for hint in ["Le", "Be"] {
            let n = 4 * bytes_per;
            let mut bytes: Vec<u8> = (0..n).map(|i| (i as u8).wrapping_mul(37) ^ 0x5a).collect();
            if primitive == Primitive::Fractus {
                // Quiet NaN with a payload, then -0.0, in the first two cells.
                let (nan, neg_zero): (Vec<u8>, Vec<u8>) = match (width, hint) {
                    (NumericWidth::F32, "Le") => (
                        0x7fc0_1234_u32.to_le_bytes().to_vec(),
                        0x8000_0000_u32.to_le_bytes().to_vec(),
                    ),
                    (NumericWidth::F32, _) => (
                        0x7fc0_1234_u32.to_be_bytes().to_vec(),
                        0x8000_0000_u32.to_be_bytes().to_vec(),
                    ),
                    (_, "Le") => (
                        0x7ff8_0000_0000_1234_u64.to_le_bytes().to_vec(),
                        0x8000_0000_0000_0000_u64.to_le_bytes().to_vec(),
                    ),
                    _ => (
                        0x7ff8_0000_0000_1234_u64.to_be_bytes().to_vec(),
                        0x8000_0000_0000_0000_u64.to_be_bytes().to_vec(),
                    ),
                };
                bytes[..bytes_per].copy_from_slice(&nan);
                bytes[bytes_per..2 * bytes_per].copy_from_slice(&neg_zero);
            }
            let back = round_trip(bytes.clone(), (primitive, width), &[2, 2], hint);
            assert_eq!(back, bytes, "{primitive:?} {width:?} via {hint}");
        }
    }
}
