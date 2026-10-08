//! Unit tests for typed dense storage (companion to `dense.rs`).
// Test fixtures: exact float expectations and small, known-in-range numeric casts.
#![allow(clippy::float_cmp, clippy::cast_possible_truncation)]

use crate::dense::{DenseCell, DenseKind, DenseStorage};
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
