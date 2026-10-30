//! Unit tests for typed dense storage (companion to `dense.rs`).
// Test fixtures: exact float expectations and small, known-in-range numeric casts.
#![allow(clippy::float_cmp, clippy::cast_possible_truncation)]

use crate::dense::{DenseCell, DenseElement, DenseKind, DenseStorage};
use crate::value::Value;
use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

const ALL_KINDS: [DenseKind; 8] = [
    DenseKind::F32,
    DenseKind::F64,
    DenseKind::I32,
    DenseKind::I64,
    DenseKind::U8,
    DenseKind::U32,
    DenseKind::U64,
    DenseKind::Bool,
];

/// One in-range sample per kind, in the carrier the runner reads it as.
fn sample(kind: DenseKind) -> Value {
    match kind {
        DenseKind::F32 => Value::Float(1.5),
        DenseKind::F64 => Value::Float(-2.25),
        DenseKind::I32 => Value::Int(-7),
        DenseKind::I64 => Value::Int(-9_000_000_000),
        DenseKind::U8 => Value::Int(200),
        DenseKind::U32 => Value::Int(4_000_000_000),
        DenseKind::U64 => Value::UInt(u64::MAX),
        DenseKind::Bool => Value::Bool(true),
    }
}

#[test]
fn per_kind_set_then_get_round_trips() {
    for kind in ALL_KINDS {
        let cell = DenseCell::zeros(kind, 3);
        assert_eq!(cell.kind(), Some(kind));
        assert_eq!(cell.len(), 3);
        assert!(cell.set(1, &sample(kind)), "{kind:?}");
        assert_eq!(cell.get(1), Some(sample(kind)), "{kind:?}");
        assert!(cell.get(3).is_none(), "{kind:?} out of bounds");
        assert!(!cell.set(3, &sample(kind)), "{kind:?} out of bounds");
    }
}

#[test]
fn zeros_read_as_zero_of_each_carrier() {
    let zero = |kind| DenseCell::zeros(kind, 1).get(0);
    assert_eq!(zero(DenseKind::F32), Some(Value::Float(0.0)));
    assert_eq!(zero(DenseKind::F64), Some(Value::Float(0.0)));
    assert_eq!(zero(DenseKind::I32), Some(Value::Int(0)));
    assert_eq!(zero(DenseKind::U8), Some(Value::Int(0)));
    assert_eq!(zero(DenseKind::U64), Some(Value::UInt(0)));
    assert_eq!(zero(DenseKind::Bool), Some(Value::Bool(false)));
}

#[test]
fn f32_set_rounds_and_f64_set_does_not() {
    let wide = 0.1_f64;
    assert_ne!(f64::from(wide as f32), wide);
    let narrow = DenseCell::zeros(DenseKind::F32, 1);
    assert!(narrow.set(0, &Value::Float(wide)));
    assert_eq!(narrow.get(0), Some(Value::Float(f64::from(wide as f32))));
    let exact = DenseCell::zeros(DenseKind::F64, 1);
    assert!(exact.set(0, &Value::Float(wide)));
    assert_eq!(exact.get(0), Some(Value::Float(wide)));
    // Overflow rounds to infinity, as `value as f32` does in the runner.
    assert!(narrow.set(0, &Value::Float(1e300)));
    assert_eq!(narrow.get(0), Some(Value::Float(f64::INFINITY)));
}

#[test]
fn integer_narrowing_failures_refuse_and_leave_storage_unchanged() {
    let refuse = |kind, value: Value| {
        let cell = DenseCell::zeros(kind, 1);
        assert!(cell.set(0, &sample(kind)));
        assert!(!cell.set(0, &value), "{kind:?} {value:?}");
        assert_eq!(cell.get(0), Some(sample(kind)), "{kind:?} unchanged");
    };
    refuse(DenseKind::I32, Value::Int(i64::from(i32::MAX) + 1));
    refuse(DenseKind::I32, Value::Int(i64::from(i32::MIN) - 1));
    refuse(DenseKind::U8, Value::Int(256));
    refuse(DenseKind::U8, Value::Int(-1));
    refuse(DenseKind::U32, Value::Int(i64::from(u32::MAX) + 1));
    refuse(DenseKind::U32, Value::UInt(u64::from(u32::MAX) + 1));
    refuse(DenseKind::U64, Value::Int(-1));
    // Wrong carrier is the same failure surface as a range miss.
    refuse(DenseKind::I64, Value::UInt(1));
    refuse(DenseKind::I64, Value::Float(1.0));
    refuse(DenseKind::F32, Value::Int(1));
    refuse(DenseKind::F64, Value::Bool(true));
    refuse(DenseKind::Bool, Value::Int(1));
    // Non-negative Int and UInt both land in the unsigned kinds.
    let wide = DenseCell::zeros(DenseKind::U64, 2);
    assert!(wide.set(0, &Value::Int(5)));
    assert!(wide.set(1, &Value::UInt(6)));
    assert_eq!(wide.get(0), Some(Value::UInt(5)));
    assert_eq!(wide.get(1), Some(Value::UInt(6)));
}

#[test]
fn values_kind_round_trips_scalars_and_vector_elements() {
    let vector = Value::Vector(Rc::new(RefCell::new(vec![
        Value::Float(1.0),
        Value::Float(2.0),
    ])));
    let items = vec![
        Value::Int(3),
        Value::Float(0.1),
        vector.clone(),
        Value::Str("x".into()),
    ];
    let cell = DenseCell::from_values(items.clone(), None);
    assert_eq!(cell.kind(), None);
    assert_eq!(cell.len(), 4);
    assert_eq!(cell.to_values(), items);
    assert_eq!(cell.get(2), Some(vector));
    // `Values` stores anything, unrounded.
    assert!(cell.set(1, &Value::Float(0.1)));
    assert_eq!(cell.get(1), Some(Value::Float(0.1)));
    assert!(!cell.set(4, &Value::Nil));
    // Bytes and typed slices do not exist for `Values`.
    assert!(cell.as_le_bytes(0..1).is_none());
    assert!(!cell.write_le_bytes(0..1, &[0; 8]));
    assert!(cell.with_slice::<f64, ()>(|_| ()).is_none());
}

#[test]
fn from_values_infers_or_narrows() {
    let floats = DenseStorage::from_values(vec![Value::Float(0.1), Value::Float(2.0)], None);
    assert_eq!(floats.kind(), Some(DenseKind::F64));
    assert_eq!(floats.get(0), Some(Value::Float(0.1)));
    let ints = DenseStorage::from_values(vec![Value::Int(1), Value::Int(-2)], None);
    assert_eq!(ints.kind(), Some(DenseKind::I64));
    let mixed = DenseStorage::from_values(vec![Value::Int(1), Value::Float(2.0)], None);
    assert_eq!(mixed.kind(), None);
    let empty = DenseStorage::from_values(Vec::new(), None);
    assert_eq!(empty.kind(), None);
    assert!(empty.is_empty());

    let narrowed = DenseStorage::from_values(vec![Value::Float(0.1)], Some(DenseKind::F32));
    assert_eq!(narrowed.kind(), Some(DenseKind::F32));
    assert_eq!(
        narrowed.get(0),
        Some(Value::Float(f64::from(0.1_f64 as f32)))
    );
    // A value that does not fit the requested kind keeps every value as-is.
    let kept = DenseStorage::from_values(vec![Value::Int(1), Value::Int(300)], Some(DenseKind::U8));
    assert_eq!(kept.kind(), None);
    assert_eq!(kept.to_values(), vec![Value::Int(1), Value::Int(300)]);
}

/// Expected little-endian bytes of `[sample(kind), zero]`, written out by hand
/// to match `ScalarKind::encode` in `faber/src/package/device/session_dispatch.rs`
/// (`f32` as `v as f32`, `i32`/`u8`/`u32`/`u64` range-checked, all LE).
fn expected_bytes(kind: DenseKind) -> Vec<u8> {
    match kind {
        // 1.5f32 = 0x3FC00000
        DenseKind::F32 => vec![0x00, 0x00, 0xC0, 0x3F, 0, 0, 0, 0],
        // -2.25f64 = 0xC002000000000000
        DenseKind::F64 => vec![0, 0, 0, 0, 0, 0, 0x02, 0xC0, 0, 0, 0, 0, 0, 0, 0, 0],
        // -7i32 = 0xFFFFFFF9
        DenseKind::I32 => vec![0xF9, 0xFF, 0xFF, 0xFF, 0, 0, 0, 0],
        // -9_000_000_000i64 = 0xFFFFFFFDE78EE600 (two's complement)
        DenseKind::I64 => vec![
            0x00, 0xE6, 0x8E, 0xE7, 0xFD, 0xFF, 0xFF, 0xFF, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
        DenseKind::U8 => vec![200, 0],
        // 4_000_000_000u32 = 0xEE6B2800
        DenseKind::U32 => vec![0x00, 0x28, 0x6B, 0xEE, 0, 0, 0, 0],
        DenseKind::U64 => vec![
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
        DenseKind::Bool => vec![1, 0],
    }
}

#[test]
fn le_bytes_equal_the_scalar_kind_layout() {
    for kind in ALL_KINDS {
        let cell = DenseCell::zeros(kind, 2);
        assert!(cell.set(0, &sample(kind)));
        let expected = expected_bytes(kind);
        assert_eq!(expected.len(), 2 * kind.width(), "{kind:?} fixture");
        assert_eq!(cell.as_le_bytes(0..2), Some(expected.clone()), "{kind:?}");
        // A sub-range reads only its elements.
        assert_eq!(
            cell.as_le_bytes(1..2),
            Some(expected[kind.width()..].to_vec()),
            "{kind:?} tail"
        );
        assert!(cell.as_le_bytes(0..3).is_none(), "{kind:?} out of bounds");
    }
}

#[test]
fn i64_fixture_matches_native_encoding() {
    // Guards the hand-written `I64` constant above against arithmetic slips.
    assert_eq!(
        (-9_000_000_000_i64).to_le_bytes()[..],
        expected_bytes(DenseKind::I64)[..8]
    );
    assert_eq!(
        4_000_000_000_u32.to_le_bytes()[..],
        expected_bytes(DenseKind::U32)[..4]
    );
    assert_eq!(
        (-2.25_f64).to_le_bytes()[..],
        expected_bytes(DenseKind::F64)[..8]
    );
}

#[test]
fn le_bytes_borrow_without_copy_on_little_endian_hosts() {
    let cell = DenseCell::zeros(DenseKind::F32, 4);
    let borrowed = cell.with_le_bytes(0..4, |bytes| matches!(bytes, std::borrow::Cow::Borrowed(_)));
    assert_eq!(borrowed, Some(cfg!(target_endian = "little")));
}

fn assert_byte_pointer_matches_elements<T: DenseElement>(cell: &DenseCell) {
    let byte_pointer = cell
        .with_byte_slice(0..2, |bytes| bytes.as_ptr())
        .expect("typed byte slice");
    let element_pointer = cell
        .with_slice::<T, _>(|items| items.as_ptr().cast::<u8>())
        .expect("matching typed slice");
    assert_eq!(byte_pointer, element_pointer);
}

#[test]
fn byte_slice_matches_as_le_bytes_for_every_kind() {
    for kind in ALL_KINDS {
        let cell = DenseCell::zeros(kind, 2);
        assert!(cell.set(0, &sample(kind)), "{kind:?}");
        let expected = expected_bytes(kind);
        assert_eq!(
            cell.with_byte_slice(0..2, <[u8]>::to_vec),
            cell.as_le_bytes(0..2),
            "{kind:?} full slice"
        );
        assert_eq!(
            cell.with_byte_slice(1..2, <[u8]>::to_vec),
            Some(expected[kind.width()..].to_vec()),
            "{kind:?} second element"
        );
        assert!(cell.with_byte_slice(0..3, <[u8]>::to_vec).is_none());

        if cfg!(target_endian = "little") {
            match kind {
                DenseKind::F32 => assert_byte_pointer_matches_elements::<f32>(&cell),
                DenseKind::F64 => assert_byte_pointer_matches_elements::<f64>(&cell),
                DenseKind::I32 => assert_byte_pointer_matches_elements::<i32>(&cell),
                DenseKind::I64 => assert_byte_pointer_matches_elements::<i64>(&cell),
                DenseKind::U8 => assert_byte_pointer_matches_elements::<u8>(&cell),
                DenseKind::U32 => assert_byte_pointer_matches_elements::<u32>(&cell),
                DenseKind::U64 => assert_byte_pointer_matches_elements::<u64>(&cell),
                DenseKind::Bool => assert_byte_pointer_matches_elements::<bool>(&cell),
            }
        }
    }

    let values = DenseCell::new(DenseStorage::Values(vec![Value::Nil]));
    assert!(values.with_byte_slice(0..1, <[u8]>::to_vec).is_none());
}

#[test]
fn zero_length_byte_slice_is_empty_and_not_none() {
    for cell in [
        DenseCell::zeros(DenseKind::U32, 2),
        DenseCell::zeros(DenseKind::U32, 0),
    ] {
        let before = cell.as_le_bytes(0..cell.len()).expect("typed bytes");
        assert_eq!(cell.with_byte_slice(0..0, <[u8]>::len), Some(0));
        assert!(cell.write_le_bytes(0..0, &[]));
        assert_eq!(cell.as_le_bytes(0..cell.len()), Some(before));
    }
}

#[test]
fn transport_counters_track_boxing_and_element_stores() {
    let typed = DenseCell::from_values(
        vec![sample(DenseKind::I32), Value::Int(12)],
        Some(DenseKind::I32),
    );
    let len = typed.len();
    let _ = typed.to_values();
    let _ = typed.to_values();
    assert_eq!(
        typed.boxed_elements(),
        2 * u64::try_from(len).expect("small test length")
    );
    assert_eq!(typed.element_stores(), 0);
    assert_eq!(typed.values_prefix(1), vec![Value::Int(-7)]);
    assert_eq!(
        typed.boxed_elements(),
        2 * u64::try_from(len).expect("small test length") + 1
    );

    assert!(typed.set(0, &Value::Int(5)));
    assert_eq!(typed.element_stores(), 1);
    let before = typed.boxed_elements();
    let _ = typed.with_byte_slice(0..len, <[u8]>::len);
    assert_eq!(typed.boxed_elements(), before);
    assert_eq!(typed.element_stores(), 1);

    let values = DenseCell::new(DenseStorage::Values(vec![Value::Int(3), Value::Int(4)]));
    let _ = values.to_values();
    assert_eq!(values.boxed_elements(), 0);
}

#[test]
fn write_le_bytes_round_trips_and_validates() {
    for kind in ALL_KINDS {
        let source = DenseCell::zeros(kind, 2);
        assert!(source.set(0, &sample(kind)));
        let bytes = source.as_le_bytes(0..2).expect("typed bytes");
        let target = DenseCell::zeros(kind, 3);
        assert!(target.write_le_bytes(1..3, &bytes), "{kind:?}");
        assert_eq!(target.get(0), DenseCell::zeros(kind, 1).get(0), "{kind:?}");
        assert_eq!(target.get(1), Some(sample(kind)), "{kind:?}");
        // Wrong byte count or range leaves the storage alone.
        assert!(!target.write_le_bytes(1..3, &bytes[1..]), "{kind:?} length");
        assert!(!target.write_le_bytes(2..4, &bytes), "{kind:?} range");
        assert_eq!(target.get(1), Some(sample(kind)), "{kind:?} unchanged");
    }
    let flags = DenseCell::zeros(DenseKind::Bool, 2);
    assert!(
        !flags.write_le_bytes(0..2, &[1, 2]),
        "bool byte must be 0 or 1"
    );
    assert_eq!(flags.get(0), Some(Value::Bool(false)), "no partial write");
}

#[test]
fn typed_slices_match_kind_or_return_none() {
    let cell = DenseCell::from_values(
        vec![Value::Float(1.0), Value::Float(2.0)],
        Some(DenseKind::F32),
    );
    assert_eq!(
        cell.with_slice::<f32, _>(<[f32]>::to_vec),
        Some(vec![1.0, 2.0])
    );
    assert!(cell.with_slice::<f64, ()>(|_| ()).is_none());
    assert!(cell.with_slice::<i32, ()>(|_| ()).is_none());
    assert!(cell.with_slice_mut::<u8, ()>(|_| ()).is_none());
    assert_eq!(
        cell.with_slice_mut::<f32, _>(|items| {
            items[0] = 0.5;
            items.len()
        }),
        Some(2)
    );
    assert_eq!(cell.get(0), Some(Value::Float(0.5)));
}

#[test]
fn clone_is_a_deep_typed_copy() {
    let cell = DenseCell::zeros(DenseKind::I32, 2);
    assert!(cell.set(0, &Value::Int(4)));
    let copy = cell.clone();
    assert!(cell.set(0, &Value::Int(9)));
    assert_eq!(copy.get(0), Some(Value::Int(4)));
    assert_eq!(copy.kind(), Some(DenseKind::I32));
}

fn assert_send_sync<T: Send + Sync>() {}

#[test]
fn dense_cell_is_send_and_sync() {
    assert_send_sync::<DenseCell>();
    assert_send_sync::<std::sync::Arc<DenseCell>>();
}

#[cfg(debug_assertions)]
#[test]
fn debug_borrow_flag_panics_on_overlapping_borrows() {
    let cell = DenseCell::zeros(DenseKind::F64, 2);
    let overlap = |run: &dyn Fn()| catch_unwind(AssertUnwindSafe(run)).is_err();
    // Mutable inside shared, shared inside mutable, mutable inside mutable.
    assert!(overlap(&|| {
        cell.with_slice::<f64, _>(|_| cell.set(0, &Value::Float(1.0)));
    }));
    assert!(overlap(&|| {
        cell.with_slice_mut::<f64, _>(|_| cell.get(0));
    }));
    assert!(overlap(&|| {
        cell.with_slice_mut::<f64, _>(|_| cell.with_slice_mut::<f64, _>(|_| ()));
    }));
    // Shared borrows nest, and the flag is released after each panic.
    assert!(!overlap(&|| {
        cell.with_slice::<f64, _>(|_| cell.get(0));
    }));
    assert!(cell.set(0, &Value::Float(2.0)));
    assert_eq!(cell.get(0), Some(Value::Float(2.0)));
}

// ---------------------------------------------------------------------------
// Tensors are born typed (unit U2)
// ---------------------------------------------------------------------------

mod born_typed {
    use crate::dense::{DenseKind, DenseStorage};
    use crate::host::BufferHost;
    use crate::value::{TensorValue, Value};
    use crate::{MirBlockId, Runner};
    use radix_lexer::{Interner, Span};
    use radix_mir::{
        MirCollectionOp, MirFunction, MirFunctionId, MirIntrinsic, MirOperand, MirProgram,
        MirRuntimeCall, MirType, MirValidationContext, MirValueId,
    };
    use radix_types::{IndexExpr, NumericWidth, Primitive, Type, TypeId, TypeTable};
    use std::cell::RefCell;
    use std::rc::Rc;

    fn function() -> MirFunction {
        MirFunction {
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
        }
    }

    fn array(values: Vec<Value>) -> Value {
        Value::Array(Rc::new(RefCell::new(values)))
    }

    fn tensor_of(values: Vec<Value>, shape: Vec<i64>) -> Value {
        Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
            values, shape,
        ))))
    }

    /// `tensor<element, [dims]>` with literal dims.
    fn tensor_type(types: &mut TypeTable, element: Type, dims: &[u64]) -> MirType {
        let element = types.intern(element);
        let dims = dims
            .iter()
            .map(|dim| types.intern_index(IndexExpr::Literal(*dim)))
            .collect();
        let shape = types.intern_index(IndexExpr::Tuple(dims));
        MirType::semantic(types.tensor_with_shape(element, shape))
    }

    /// Run one collection call over `inputs` (value ids 0..) and return the tensor.
    fn run(
        types: &TypeTable,
        op: MirCollectionOp,
        arg_count: usize,
        return_ty: MirType,
        inputs: Vec<Value>,
    ) -> Rc<RefCell<TensorValue>> {
        let mut host = BufferHost::default();
        let interner = Interner::new();
        let context = MirValidationContext::new(types);
        let mut program = MirProgram::new();
        program.functions.push(function());
        let mut runner = Runner::new(&program, &context, types, &interner, &mut host);
        let fn0 = program.functions.first().unwrap();
        runner.push_frame(fn0.id, MirBlockId(0));
        for (index, value) in inputs.into_iter().enumerate() {
            runner
                .frames
                .last_mut()
                .unwrap()
                .values
                .insert(MirValueId(index as u32), value);
        }
        let result = runner
            .eval_runtime_call(
                &MirRuntimeCall {
                    intrinsic: MirIntrinsic::Collection(op),
                    args: (0..arg_count)
                        .map(|index| MirOperand::Value(MirValueId(index as u32)))
                        .collect(),
                    return_ty,
                },
                Span::default(),
            )
            .expect("collection call");
        let Value::Tensor(tensor) = result else {
            panic!("expected a tensor, got {result:?}");
        };
        tensor
    }

    /// `lista ↦ tensor<element, [2,2]>` through the exact `TensorFromFlat` form.
    fn from_flat(element: Type, flat: Vec<Value>) -> Rc<RefCell<TensorValue>> {
        let mut types = TypeTable::new();
        let return_ty = tensor_type(&mut types, element, &[2, 2]);
        let shape = array(vec![Value::Int(2), Value::Int(2)]);
        // Receiver slot, flat, shape (the receiver is never read).
        run(
            &types,
            MirCollectionOp::TensorFromFlat,
            3,
            return_ty,
            vec![array(vec![]), array(flat), shape],
        )
    }

    fn floats(values: &[f64]) -> Vec<Value> {
        values.iter().copied().map(Value::Float).collect()
    }

    #[test]
    fn lista_f32_to_tensor_is_born_f32_with_the_old_reads() {
        // 0.5 .. 3.5 are f32-exact, so the typed reads equal the boxed reads.
        let flat = floats(&[0.5, 1.5, 2.5, 3.5]);
        let tensor = from_flat(
            Type::SizedNumeric(Primitive::Fractus, NumericWidth::F32),
            flat.clone(),
        );
        let tensor = tensor.borrow();
        assert_eq!(tensor.shape, vec![2, 2]);
        assert_eq!(tensor.data.kind(), Some(DenseKind::F32));
        assert_eq!(tensor.logical_values(), Some(flat));
    }

    #[test]
    fn a_boxed_value_that_is_not_f32_exact_keeps_boxed_storage() {
        // Born typed never changes a read: 0.1 is not an f32, so this stays boxed.
        let flat = floats(&[0.1, 1.5, 2.5, 3.5]);
        let tensor = from_flat(
            Type::SizedNumeric(Primitive::Fractus, NumericWidth::F32),
            flat.clone(),
        );
        let tensor = tensor.borrow();
        assert_eq!(tensor.data.kind(), None);
        assert_eq!(tensor.logical_values(), Some(flat));
    }

    #[test]
    fn i32_and_u8_tensors_are_born_typed() {
        let ints = |values: &[i64]| values.iter().copied().map(Value::Int).collect::<Vec<_>>();
        let i32_tensor = from_flat(
            Type::SizedNumeric(Primitive::Numerus, NumericWidth::I32),
            ints(&[-1, 2, 3, 4]),
        );
        assert_eq!(i32_tensor.borrow().data.kind(), Some(DenseKind::I32));
        assert_eq!(
            i32_tensor.borrow().logical_values(),
            Some(ints(&[-1, 2, 3, 4]))
        );
        let u8_tensor = from_flat(
            Type::SizedNumeric(Primitive::Numerus, NumericWidth::U8),
            ints(&[0, 7, 200, 255]),
        );
        assert_eq!(u8_tensor.borrow().data.kind(), Some(DenseKind::U8));
        assert_eq!(
            u8_tensor.borrow().logical_values(),
            Some(ints(&[0, 7, 200, 255]))
        );
        // Bare numerus and fractus are I64 and F64.
        let bare = from_flat(Type::Primitive(Primitive::Numerus), ints(&[1, 2, 3, 4]));
        assert_eq!(bare.borrow().data.kind(), Some(DenseKind::I64));
        let bare = from_flat(Type::Primitive(Primitive::Fractus), floats(&[1.0; 4]));
        assert_eq!(bare.borrow().data.kind(), Some(DenseKind::F64));
    }

    #[test]
    fn vector_elements_stay_boxed() {
        let mut types = TypeTable::new();
        let f32_ty = types.intern(Type::SizedNumeric(Primitive::Fractus, NumericWidth::F32));
        let width = types.intern_index(IndexExpr::Literal(2));
        let vector = types.vector_with_width(f32_ty, width);
        let dim = types.intern_index(IndexExpr::Literal(2));
        let shape = types.intern_index(IndexExpr::Tuple(vec![dim]));
        let return_ty = MirType::semantic(types.tensor_with_shape(vector, shape));
        let elements = vec![
            Value::Vector(Rc::new(RefCell::new(floats(&[1.0, 2.0])))),
            Value::Vector(Rc::new(RefCell::new(floats(&[3.0, 4.0])))),
        ];
        let tensor = run(
            &types,
            MirCollectionOp::TensorFromFlat,
            3,
            return_ty,
            vec![array(vec![]), array(elements), array(vec![Value::Int(2)])],
        );
        assert!(matches!(
            tensor.borrow().data.clone_storage(),
            DenseStorage::Values(_)
        ));
        assert_eq!(tensor.borrow().data.kind(), None);
    }

    #[test]
    fn matmul_of_f32_tensors_is_born_f32_with_rounded_cells() {
        let mut types = TypeTable::new();
        let return_ty = tensor_type(
            &mut types,
            Type::SizedNumeric(Primitive::Fractus, NumericWidth::F32),
            &[2, 2],
        );
        let left = tensor_of(floats(&[0.1, 0.2, 0.3, 0.4]), vec![2, 2]);
        let right = tensor_of(floats(&[0.5, 0.6, 0.7, 0.8]), vec![2, 2]);
        // The unrounded f64 contraction the old path stored.
        let old = [
            0.1 * 0.5 + 0.2 * 0.7,
            0.1 * 0.6 + 0.2 * 0.8,
            0.3 * 0.5 + 0.4 * 0.7,
            0.3 * 0.6 + 0.4 * 0.8,
        ];
        let tensor = run(
            &types,
            MirCollectionOp::TensorMatMul,
            2,
            return_ty,
            vec![left, right],
        );
        let tensor = tensor.borrow();
        assert_eq!(tensor.data.kind(), Some(DenseKind::F32));
        let cells = tensor.logical_values().expect("layout");
        for (cell, old) in cells.iter().zip(old) {
            let Value::Float(cell) = cell else {
                panic!("float cell, got {cell:?}");
            };
            assert_eq!(*cell, f64::from(old as f32), "f32-rounded old value");
            assert_eq!(*cell, f64::from(*cell as f32), "cell is f32-representable");
        }
    }

    #[test]
    fn copy_keeps_the_born_kind() {
        let tensor = from_flat(
            Type::SizedNumeric(Primitive::Numerus, NumericWidth::U8),
            vec![Value::Int(1), Value::Int(2), Value::Int(3), Value::Int(4)],
        );
        let Value::Tensor(copy) = Value::Tensor(tensor).deep_copy() else {
            panic!("tensor copy stays a tensor");
        };
        assert_eq!(copy.borrow().data.kind(), Some(DenseKind::U8));
    }
}

// --- Deferred zeros (fresh kernel-output host twins) ---

/// The zero element of a dense kind, as a deferred twin reads it.
fn zero_of(kind: DenseKind) -> Value {
    match kind {
        DenseKind::F32 | DenseKind::F64 => Value::Float(0.0),
        DenseKind::I32 | DenseKind::I64 | DenseKind::U8 | DenseKind::U32 => Value::Int(0),
        DenseKind::U64 => Value::UInt(0),
        DenseKind::Bool => Value::Bool(false),
    }
}

/// A one-field genus row layout (one i32, 4 bytes a row). Returns the layout
/// and the interned field name.
fn one_i32_row_layout() -> (
    std::sync::Arc<crate::dense::RecordLayout>,
    radix_lexer::Symbol,
) {
    const DEF: radix_hir::DefId = radix_hir::DefId(7);
    let mut types = radix_types::TypeTable::new();
    let genus = types.intern(radix_types::Type::Struct(DEF));
    let row_ty = types.sized_numeric(
        radix_types::Primitive::Numerus,
        radix_types::NumericWidth::I32,
    );
    let mut interner = radix_lexer::Interner::new();
    let name = interner.intern("n");
    let mut ctx = radix_mir::MirValidationContext::new(&types);
    ctx.struct_fields.insert(
        DEF,
        [(name, radix_mir::MirType::semantic(row_ty))]
            .into_iter()
            .collect(),
    );
    ctx.struct_field_order.insert(DEF, vec![name]);
    let layout = crate::dense::RecordLayout::resolve(genus, &ctx)
        .map(std::sync::Arc::new)
        .expect("fixed record layout");
    (layout, name)
}

#[test]
fn deferred_zeros_read_as_zeros_without_materializing() {
    for kind in ALL_KINDS {
        let cell = DenseCell::new(DenseStorage::fresh_zeros(kind, 3));
        assert_eq!(cell.len(), 3, "{kind:?}");
        assert_eq!(cell.kind(), Some(kind), "{kind:?}");
        assert_eq!(cell.backing_bytes(), 0, "{kind:?} holds no array yet");
        assert!(!cell.is_empty(), "{kind:?}");
        for index in 0..3 {
            assert_eq!(cell.get(index), Some(zero_of(kind)), "{kind:?} [{index}]");
        }
        assert_eq!(cell.get(3), None, "{kind:?} out of bounds");
        let zeros = cell.as_le_bytes(0..3).expect("deferred byte view");
        assert!(zeros.iter().all(|byte| *byte == 0), "{kind:?} zero bytes");
        assert_eq!(zeros.len(), 3 * kind.width(), "{kind:?}");
        assert_eq!(
            cell.values_prefix(2),
            vec![zero_of(kind), zero_of(kind)],
            "{kind:?} prefix"
        );
        assert_eq!(
            cell.backing_bytes(),
            0,
            "{kind:?} reads materialize nothing"
        );
        // A refused (out-of-bounds) store still takes the write path, which
        // materializes; it changes no element.
        assert!(!cell.set(3, &sample(kind)), "{kind:?} out of bounds");
        assert_eq!(cell.get(0), Some(zero_of(kind)), "{kind:?} unchanged");
    }
    // The empty deferred twin (an empty scan's output) stays trivially empty.
    let empty = DenseCell::new(DenseStorage::fresh_zeros(DenseKind::F32, 0));
    assert!(empty.is_empty());
    assert_eq!(empty.backing_bytes(), 0);
}

#[test]
fn deferred_zeros_materialize_on_the_first_write() {
    let cell = DenseCell::new(DenseStorage::fresh_zeros(DenseKind::F32, 4));
    assert!(cell.set(1, &Value::Float(7.5)), "store into deferred zeros");
    assert_eq!(
        cell.backing_bytes(),
        4 * std::mem::size_of::<f32>(),
        "array materialized"
    );
    assert_eq!(cell.get(1), Some(Value::Float(7.5)));
    assert_eq!(cell.get(0), Some(Value::Float(0.0)), "the rest is zeros");
    // A second write lands in the materialized array and grows nothing.
    assert!(cell.set(2, &Value::Float(-1.0)));
    assert_eq!(cell.backing_bytes(), 4 * std::mem::size_of::<f32>());

    // The byte-write path (a device download landing its read) materializes too.
    let cell = DenseCell::new(DenseStorage::fresh_zeros(DenseKind::U8, 4));
    assert!(cell.write_le_bytes(2..4, &[9, 200]));
    assert_eq!(cell.backing_bytes(), 4);
    assert_eq!(cell.get(2), Some(Value::Int(9)));
    assert_eq!(cell.get(3), Some(Value::Int(200)));
    assert_eq!(cell.get(0), Some(Value::Int(0)));
    // A bad byte write materializes (the store needs an array to refuse into)
    // but changes nothing.
    let cell = DenseCell::new(DenseStorage::fresh_zeros(DenseKind::Bool, 2));
    assert!(!cell.write_le_bytes(0..2, &[0, 2]), "2 is not a bool byte");
    assert_eq!(cell.get(1), Some(Value::Bool(false)));
}

#[test]
fn deferred_zeros_clone_stays_deferred_and_the_typed_slice_materializes() {
    let cell = DenseCell::new(DenseStorage::fresh_zeros(DenseKind::I32, 3));
    let copy = cell.clone_storage();
    assert_eq!(copy.backing_bytes(), 0, "a deferred copy defers too");
    assert_eq!(copy.get(1), Some(Value::Int(0)));
    let cell = DenseCell::new(DenseStorage::fresh_zeros(DenseKind::I32, 3));
    let seen = cell.with_slice::<i32, _>(|items| items.to_vec());
    assert_eq!(seen, Some(vec![0, 0, 0]), "a slice view needs real bytes");
    assert_eq!(cell.backing_bytes(), 3 * std::mem::size_of::<i32>());
}

#[test]
fn deferred_zeros_compare_equal_to_their_zeros_by_content() {
    let fresh = DenseCell::new(DenseStorage::fresh_zeros(DenseKind::F32, 2));
    let zeros = DenseCell::zeros(DenseKind::F32, 2);
    assert_eq!(fresh, zeros, "a deferred twin is its zeros");
    let written = DenseCell::new(DenseStorage::fresh_zeros(DenseKind::F32, 2));
    assert!(written.set(0, &Value::Float(1.0)));
    assert_ne!(written, zeros, "a stored element breaks the equality");
    let other = DenseCell::new(DenseStorage::fresh_zeros(DenseKind::F64, 2));
    assert_ne!(fresh, other, "kind is part of the deferral");
    let fresh = DenseCell::new(DenseStorage::fresh_zeros(DenseKind::F32, 2));
    let twin = DenseCell::new(DenseStorage::fresh_zeros(DenseKind::F32, 2));
    assert_eq!(fresh, twin, "two untouched twins are equal");
}

#[test]
fn deferred_record_zeros_read_and_materialize_like_record_zeros() {
    let (layout, name) = one_i32_row_layout();
    assert_eq!(layout.size(), 4);
    let fresh = DenseCell::new(DenseStorage::fresh_zeros_record(
        std::sync::Arc::clone(&layout),
        3,
    ));
    assert_eq!(fresh.len(), 3);
    assert_eq!(fresh.record_size(), Some(4));
    assert_eq!(fresh.kind(), None, "a record row is not one scalar kind");
    assert_eq!(fresh.backing_bytes(), 0, "no row bytes yet");
    let rows = fresh.as_le_bytes(0..3).expect("deferred row bytes");
    assert_eq!(rows.len(), 12);
    assert!(rows.iter().all(|byte| *byte == 0));
    assert_eq!(fresh.backing_bytes(), 0, "row reads materialize nothing");
    // A row stores as its genus struct value (the record layout encodes it).
    let mut fields = handroll::FxHashMap::default();
    fields.insert(name, Value::Int(-2));
    let row = Value::Struct(
        radix_hir::DefId(7),
        std::rc::Rc::new(std::cell::RefCell::new(fields)),
    );
    assert!(fresh.set(1, &row), "store into row 1");
    assert_eq!(fresh.backing_bytes(), 12, "rows materialized");
    assert_eq!(
        fresh.as_le_bytes(1..2).as_deref(),
        Some(&(-2_i32).to_le_bytes()[..]),
        "the stored row encodes little-endian"
    );
    let zeros = DenseCell::new(DenseStorage::zeros_record(
        std::sync::Arc::clone(&layout),
        3,
    ));
    let fresh = DenseCell::new(DenseStorage::fresh_zeros_record(layout, 3));
    assert_eq!(fresh, zeros, "deferred record zeros equal record zeros");
}
