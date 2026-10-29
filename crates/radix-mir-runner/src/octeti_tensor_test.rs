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
    assert_eq!(err.issue, "runner_list_b8_tensor_length_mismatch");
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
    assert_eq!(err.issue, "list_b8_to_tensor_shape_operand_missing");
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
        assert_eq!(err.issue, "runner_list_b8_tensor_length_mismatch");
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

// --- record (fixed-layout genus) elements --------------------
//
// A neutral 34-byte record: an `f16` head and 32 `i8` slots, the same shape any
// block-quantized weight library would declare. Nothing here names a format.

mod record {
    use super::*;
    use handroll::FxHashMap;
    use radix_lexer::Symbol;
    use radix_mir::MirValidationContext;
    use std::io::{Read, Seek, SeekFrom, Write};

    const GENUS: DefId = DefId(9);
    const ROW: usize = 34;
    const HEAD: Symbol = Symbol(100_000);
    const SLOTS: Symbol = Symbol(99_999);
    const ISSUE_OVERFLOW: &str = "runner_list_b8_to_tensor_record_overflow";
    const ISSUE_MISALIGNED: &str = "runner_list_b8_to_tensor_record_misaligned";
    const ISSUE_TRUNCATED: &str = "runner_list_b8_to_tensor_record_truncated";
    const ISSUE_TRAILING: &str = "runner_list_b8_to_tensor_record_trailing_bytes";

    /// The IEEE half bits of the small multiples of 0.5 the fixtures use.
    fn half_bits(value: f32) -> u16 {
        crate::f32_to_f16_rn(value)
    }

    /// Row `i`: head `(i + 1) / 2` as an `f16`, slot `j` = `7 i + j - 40`.
    fn row(i: usize) -> Vec<u8> {
        let mut bytes = half_bits((i + 1) as f32 * 0.5).to_le_bytes().to_vec();
        bytes.extend((0..32).map(|j| (7 * i as i32 + j - 40) as i8 as u8));
        bytes
    }

    fn rows(count: usize) -> Vec<u8> {
        (0..count).flat_map(row).collect()
    }

    struct Fixture {
        types: TypeTable,
        element: TypeId,
    }

    fn q_record() -> Fixture {
        let mut types = TypeTable::new();
        let element = types.intern(radix_types::Type::Struct(GENUS));
        Fixture { types, element }
    }

    /// Run `octeti ↦ tensor<record, extents> via hint` over `bytes`.
    fn convert(
        fixture: &Fixture,
        field_types: &[(Symbol, TypeId)],
        bytes: Vec<u8>,
        extents: Vec<IndexExpr>,
        shape: Option<Vec<i64>>,
        hint: &str,
        recovery: Option<&MirOperand>,
    ) -> Result<Value, RunnerError> {
        let mut types = fixture.types.clone();
        let dim_ids: Vec<_> = extents
            .into_iter()
            .map(|extent| types.intern_index(extent))
            .collect();
        let shape_index = types.intern_index(IndexExpr::Tuple(dim_ids));
        let target = types.tensor_with_shape(fixture.element, shape_index);
        let mut interner = Interner::new();
        let hint_sym = interner.intern(hint);
        let mut context = MirValidationContext::new(&types);
        let order: Vec<Symbol> = field_types.iter().map(|(name, _)| *name).collect();
        let declared: FxHashMap<Symbol, MirType> = field_types
            .iter()
            .map(|(name, ty)| (*name, MirType::semantic(*ty)))
            .collect();
        context.struct_fields.insert(GENUS, declared);
        context.struct_field_order.insert(GENUS, order);
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
        runner.convert_octeti_to_tensor(
            bytes,
            target,
            shape,
            &[hint_sym],
            recovery,
            Span::default(),
        )
    }

    fn q_fields(fixture: &mut Fixture) -> Vec<(Symbol, TypeId)> {
        let f16 = fixture
            .types
            .sized_numeric(Primitive::Fractus, NumericWidth::F16);
        let i8 = fixture
            .types
            .sized_numeric(Primitive::Numerus, NumericWidth::I8);
        let capacity = fixture.types.intern_index(IndexExpr::Literal(32));
        let slots = fixture.types.bounded_array(i8, capacity);
        vec![(HEAD, f16), (SLOTS, slots)]
    }

    fn load(
        count: u64,
        bytes: Vec<u8>,
        hint: &str,
        recovery: Option<&MirOperand>,
    ) -> Result<Value, RunnerError> {
        let mut fixture = q_record();
        let fields = q_fields(&mut fixture);
        convert(
            &fixture,
            &fields,
            bytes,
            vec![IndexExpr::Literal(count)],
            None,
            hint,
            recovery,
        )
    }

    fn field(value: &Value, name: Symbol) -> Value {
        let Value::Struct(def, fields) = value else {
            panic!("expected a struct row, got {value:?}");
        };
        assert_eq!(*def, GENUS);
        fields.borrow().get(&name).cloned().expect("field")
    }

    fn slots(value: &Value) -> Vec<i64> {
        let Value::Array(items) = field(value, SLOTS) else {
            panic!("slots are an array");
        };
        items
            .borrow()
            .iter()
            .map(|item| match item {
                Value::Int(v) => *v,
                other => panic!("slot is {other:?}"),
            })
            .collect()
    }

    #[test]
    fn record_tensor_keeps_the_input_bytes_as_typed_rows() {
        let bytes = rows(5);
        let t = tensor(load(5, bytes.clone(), "Le", None).expect("record tensor"));
        let t = t.borrow();
        assert_eq!(t.shape, vec![5]);
        assert_eq!(t.data.len(), 5);
        assert_eq!(t.data.record_size(), Some(ROW));
        assert_eq!(
            t.data.kind(),
            None,
            "a record row is not a scalar dense kind"
        );
        // The host bytes are the input bytes, row for row.
        assert_eq!(t.data.as_le_bytes(0..5).expect("bytes"), bytes);
        assert_eq!(
            t.data.as_le_bytes(1..3).expect("rows 1..3"),
            bytes[ROW..3 * ROW]
        );
        // Nothing was boxed or stored element by element to get there.
        assert_eq!(t.data.boxed_elements(), 0);
        assert_eq!(t.data.element_stores(), 0);
        assert!(t.data.with_values(|_| ()).is_none(), "not boxed Values");
    }

    #[test]
    fn record_rows_read_back_as_struct_values() {
        let t = tensor(load(5, rows(5), "Le", None).expect("record tensor"));
        let t = t.borrow();
        for i in 0..5 {
            let row = t.data.get(i).expect("row");
            assert_eq!(
                field(&row, HEAD),
                Value::Float((i + 1) as f64 * 0.5),
                "head {i}"
            );
            let want: Vec<i64> = (0..32).map(|j| 7 * i as i64 + j - 40).collect();
            assert_eq!(slots(&row), want, "slots {i}");
        }
        assert!(t.data.get(5).is_none(), "past the end");
    }

    #[test]
    fn record_rows_store_back_through_the_layout() {
        let t = tensor(load(3, rows(3), "Le", None).expect("record tensor"));
        let t = t.borrow();
        let from = t.data.get(2).expect("row 2");
        assert!(t.data.set(0, &from));
        assert_eq!(t.data.as_le_bytes(0..1).expect("row 0"), row(2));
        // A slot outside i8 is refused and leaves the row alone.
        let Value::Struct(def, fields) = t.data.get(1).expect("row 1") else {
            panic!("struct row");
        };
        let slots_value = Value::Array(Rc::new(RefCell::new(vec![Value::Int(128); 32])));
        fields.borrow_mut().insert(SLOTS, slots_value);
        assert!(!t.data.set(1, &Value::Struct(def, fields)));
        assert_eq!(t.data.as_le_bytes(1..2).expect("row 1"), row(1));
        assert!(!t.data.set(3, &from), "out of bounds");
    }

    #[test]
    fn record_tensor_run_time_extent_reads_the_shape_operand() {
        let mut fixture = q_record();
        let fields = q_fields(&mut fixture);
        let value = convert(
            &fixture,
            &fields,
            rows(6),
            runtime_extents(),
            Some(vec![2, 3]),
            "Le",
            None,
        )
        .expect("run-time extent record decode");
        let t = tensor(value);
        assert_eq!(t.borrow().shape, vec![2, 3]);
        assert_eq!(t.borrow().data.len(), 6);
    }

    #[test]
    fn record_tensor_names_truncation_misalignment_trailing_bytes_and_overflow() {
        let cases: [(&str, Vec<u8>, &str); 3] = [
            ("truncated", rows(3)[..2 * ROW].to_vec(), ISSUE_TRUNCATED),
            (
                "misaligned",
                rows(3)[..3 * ROW - 1].to_vec(),
                ISSUE_MISALIGNED,
            ),
            ("trailing", rows(4), ISSUE_TRAILING),
        ];
        for (name, bytes, issue) in cases {
            let err = load(3, bytes, "Le", None).expect_err(name);
            assert_eq!(err.code, "runner_conversion", "{name}");
            assert_eq!(err.issue, issue, "{name}");
        }
        let err = load(3, Vec::new(), "Le", None).expect_err("empty input");
        assert_eq!(err.issue, ISSUE_TRUNCATED);
        assert_eq!(err.arg("record_bytes"), Some("34"));
        assert_eq!(err.arg("expected"), Some("102"));
        assert_eq!(err.arg("actual"), Some("0"));

        let mut fixture = q_record();
        let fields = q_fields(&mut fixture);
        for dims in [vec![i64::MAX, 2], vec![-1]] {
            let extents = dims
                .iter()
                .map(|_| IndexExpr::Runtime(RuntimeExtentId::ScalarBinding(DefId(7))))
                .collect();
            let err = convert(
                &fixture,
                &fields,
                rows(1),
                extents,
                Some(dims.clone()),
                "Le",
                None,
            )
            .expect_err("overflow");
            assert_eq!(err.issue, ISSUE_OVERFLOW, "{dims:?}");
            assert_eq!(err.arg("expected"), Some("overflow"));
        }
    }

    #[test]
    fn record_tensor_failures_recover_with_a_default() {
        let recovery = MirOperand::Constant(MirConstant::Int(42));
        for bytes in [
            rows(2)[..ROW].to_vec(),
            rows(2)[..ROW + 1].to_vec(),
            rows(3),
        ] {
            let value = load(2, bytes, "Le", Some(&recovery)).expect("recovered");
            assert_eq!(value, Value::Int(42));
        }
        let ok = load(2, rows(2), "Le", Some(&recovery)).expect("ok");
        assert!(matches!(ok, Value::Tensor(_)));
    }

    #[test]
    fn record_tensor_is_little_endian_only() {
        let err = load(1, rows(1), "Be", None).expect_err("Be");
        assert_eq!(err.issue, "list_b8_to_tensor_record_big_endian");
    }

    #[test]
    fn record_tensor_refuses_a_genus_without_a_fixed_layout() {
        let mut fixture = q_record();
        let f64_ty = fixture
            .types
            .sized_numeric(Primitive::Fractus, NumericWidth::F64);
        let err = convert(
            &fixture,
            &[(HEAD, f64_ty)],
            vec![0; 8],
            vec![IndexExpr::Literal(1)],
            None,
            "Le",
            None,
        )
        .expect_err("f64 field has no device layout");
        assert_eq!(err.issue, "list_b8_to_tensor_record_layout_refused");
    }

    /// A bounded range of a real file loads as record rows: the tensor's host
    /// bytes are exactly the range (no wider copy), and a range cut short by the
    /// end of the file is the named truncation.
    #[test]
    fn real_file_range_loads_as_record_rows_and_truncates_by_name() {
        let path = std::env::temp_dir().join(format!(
            "faber-record-range-{}-{}",
            std::process::id(),
            line!()
        ));
        let (lead, count, tail) = (13_usize, 4_usize, 9_usize);
        let body = rows(count);
        let mut file = std::fs::File::create(&path).expect("create fixture");
        file.write_all(&vec![0xee; lead]).expect("lead");
        file.write_all(&body).expect("rows");
        file.write_all(&vec![0xdd; tail]).expect("tail");
        drop(file);

        let read_range = |offset: u64, length: u64| -> Vec<u8> {
            let mut file = std::fs::File::open(&path).expect("open fixture");
            file.seek(SeekFrom::Start(offset)).expect("seek");
            let mut bytes = Vec::new();
            file.take(length)
                .read_to_end(&mut bytes)
                .expect("read range");
            bytes
        };

        let range = read_range(lead as u64, (count * ROW) as u64);
        assert_eq!(range, body, "the range is exactly the rows");
        let t = tensor(load(count as u64, range, "Le", None).expect("range loads"));
        {
            let t = t.borrow();
            assert_eq!(t.data.as_le_bytes(0..count).expect("bytes"), body);
            assert_eq!(t.data.boxed_elements(), 0);
            assert_eq!(t.data.element_stores(), 0);
        }

        // Ask for one row more than the file holds: the read stops at the end
        // of the file, short of the shape and off the row grid.
        let cut = read_range(lead as u64, ((count + 1) * ROW) as u64);
        assert_eq!(cut.len(), count * ROW + tail);
        let err = load(count as u64 + 1, cut, "Le", None).expect_err("cut short");
        assert_eq!(err.issue, ISSUE_MISALIGNED);

        // A range that ends on a row boundary but short of the shape.
        let short = read_range(lead as u64, ((count - 1) * ROW) as u64);
        let err = load(count as u64, short, "Le", None).expect_err("short");
        assert_eq!(err.issue, ISSUE_TRUNCATED);
        std::fs::remove_file(&path).expect("remove fixture");
    }
}
