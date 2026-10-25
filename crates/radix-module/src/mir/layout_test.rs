//! MirLayoutTable type-to-layout mapping: scalars, aggregates, tensors, device views.
//!
//! Tests: default/sized scalars, aliases, aggregates, tensor views, device vectors/matrices,
//! atomics, packed numeric, and unknown-rank fallback.

use super::{
    MirAggregateCarrier, MirAggregateFamily, MirAggregateLayout, MirAtomicLayout, MirDeviceMatrixLayout,
    MirDeviceVectorLayout, MirDeviceViewElement, MirDeviceViewLayout, MirLayoutKind, MirLayoutTable, MirPackedBitOrder,
    MirPackedNumericFormat, MirPackedNumericLayout, MirScalarLayout, MirTensorLocusLayout, MirTensorStorageLayout,
    MirTensorStrideLayout, MirTensorViewLayout,
};
use crate::hir::DefId;
use crate::mir::{MirLayoutId, MirType};
use crate::semantic::{IndexExpr, NumericWidth, Primitive, Type, TypeTable};
use handroll::FxHashMap;

#[test]
fn layout_table_maps_default_and_sized_scalars() {
    let mut types = TypeTable::new();
    let f16_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F16);
    let f32_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let numerus = types.primitive(Primitive::Numerus);
    let fractus = types.primitive(Primitive::Fractus);
    let table = MirLayoutTable::from_types(&types);

    assert_eq!(
        table
            .layout_for_type(MirType::semantic(numerus))
            .expect("numerus layout")
            .kind,
        MirLayoutKind::Scalar(MirScalarLayout::I64)
    );
    assert_eq!(
        table
            .layout_for_type(MirType::semantic(fractus))
            .expect("fractus layout")
            .kind,
        MirLayoutKind::Scalar(MirScalarLayout::F64)
    );
    assert_eq!(
        table
            .layout_for_type(MirType::semantic(f32_ty))
            .expect("f32 layout")
            .kind,
        MirLayoutKind::Scalar(MirScalarLayout::F32)
    );
    let f16_layout = table
        .layout_for_type(MirType::semantic(f16_ty))
        .expect("f16 layout");
    assert_eq!(f16_layout.kind, MirLayoutKind::Scalar(MirScalarLayout::F16));
    assert_eq!(f16_layout.size_bits, Some(16));
    assert_eq!(f16_layout.align_bits, Some(16));
}

#[test]
fn scalar_layout_classifies_integer_and_float_families() {
    assert!(!MirScalarLayout::Bool.is_integer());
    assert!(!MirScalarLayout::Bool.is_signed_integer());
    assert!(!MirScalarLayout::Bool.is_unsigned_integer());
    assert!(!MirScalarLayout::Bool.is_float());
    assert_eq!(MirScalarLayout::Bool.integer_bits(), None);

    assert!(MirScalarLayout::I32.is_integer());
    assert!(MirScalarLayout::I32.is_signed_integer());
    assert!(!MirScalarLayout::I32.is_unsigned_integer());
    assert!(!MirScalarLayout::I32.is_float());
    assert_eq!(MirScalarLayout::I32.integer_bits(), Some(32));

    assert!(MirScalarLayout::U64.is_integer());
    assert!(!MirScalarLayout::U64.is_signed_integer());
    assert!(MirScalarLayout::U64.is_unsigned_integer());
    assert!(!MirScalarLayout::U64.is_float());
    assert_eq!(MirScalarLayout::U64.integer_bits(), Some(64));

    assert!(!MirScalarLayout::F32.is_integer());
    assert!(!MirScalarLayout::F32.is_signed_integer());
    assert!(!MirScalarLayout::F32.is_unsigned_integer());
    assert!(MirScalarLayout::F32.is_float());
    assert_eq!(MirScalarLayout::F32.integer_bits(), None);
}

#[test]
fn scalar_layout_represents_integer_value_domains() {
    assert!(MirScalarLayout::I64.represents_integer(MirScalarLayout::I8));
    assert!(MirScalarLayout::I64.represents_integer(MirScalarLayout::U8));
    assert!(MirScalarLayout::U64.represents_integer(MirScalarLayout::U8));
    assert!(MirScalarLayout::I32.represents_integer(MirScalarLayout::I32));

    assert!(!MirScalarLayout::I32.represents_integer(MirScalarLayout::U32));
    assert!(!MirScalarLayout::U64.represents_integer(MirScalarLayout::I32));
    assert!(!MirScalarLayout::U16.represents_integer(MirScalarLayout::U32));
    assert!(!MirScalarLayout::F64.represents_integer(MirScalarLayout::I32));
    assert!(!MirScalarLayout::I32.represents_integer(MirScalarLayout::F32));
    assert!(!MirScalarLayout::Bool.represents_integer(MirScalarLayout::U8));
}

#[test]
fn layout_table_resolves_aliases_and_explicit_layout_ids() {
    let mut types = TypeTable::new();
    let f32_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let alias_ty = types.intern(Type::Alias(crate::hir::DefId(7), f32_ty));
    let table = MirLayoutTable::from_types(&types);
    let alias_layout = table
        .layout_for_type(MirType::semantic(alias_ty))
        .expect("alias layout");

    assert_eq!(alias_layout.kind, MirLayoutKind::Scalar(MirScalarLayout::F32));
    assert_eq!(
        table
            .layout_for_type(MirType::with_layout(alias_ty, alias_layout.id))
            .expect("explicit id layout")
            .kind,
        MirLayoutKind::Scalar(MirScalarLayout::F32)
    );
}

#[test]
fn layout_table_fails_closed_for_unknown_layout_rows() {
    let types = TypeTable::new();
    let table = MirLayoutTable::from_types(&types);
    let error = table
        .layout_for_type(MirType::with_layout(types.primitive(Primitive::Numerus), MirLayoutId(999)))
        .expect_err("unknown layout row should reject");

    assert_eq!(error.issue, "mir_layout_missing_row");
    assert_eq!(error.id, Some(MirLayoutId(999)));
}

#[test]
fn layout_table_classifies_aggregate_carriers_explicitly() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let array_ty = types.intern(Type::Array(numerus));
    let record_ty = types.intern(Type::Record(FxHashMap::default()));
    let struct_ty = types.intern(Type::Struct(DefId(7)));
    let enum_ty = types.intern(Type::Enum(DefId(8)));
    let tuple_ty = types.intern(Type::Tuple(vec![numerus]));
    let table = MirLayoutTable::from_types(&types);

    assert_eq!(
        table
            .layout_for_type(MirType::semantic(array_ty))
            .expect("array layout")
            .kind,
        MirLayoutKind::Aggregate(MirAggregateLayout {
            family: MirAggregateFamily::Array,
            carrier: MirAggregateCarrier::OpaqueRuntimeHandle,
            element: None,
            capacity: None,
        })
    );
    assert_eq!(
        table
            .layout_for_type(MirType::semantic(record_ty))
            .expect("record layout")
            .kind,
        MirLayoutKind::Aggregate(MirAggregateLayout {
            family: MirAggregateFamily::Record,
            carrier: MirAggregateCarrier::PhysicalValue,
            element: None,
            capacity: None,
        })
    );
    assert_eq!(
        table
            .layout_for_type(MirType::semantic(struct_ty))
            .expect("struct layout")
            .kind,
        MirLayoutKind::Aggregate(MirAggregateLayout {
            family: MirAggregateFamily::Struct,
            carrier: MirAggregateCarrier::PhysicalValue,
            element: None,
            capacity: None,
        })
    );
    assert_eq!(
        table
            .layout_for_type(MirType::semantic(enum_ty))
            .expect("enum layout")
            .kind,
        MirLayoutKind::Aggregate(MirAggregateLayout {
            family: MirAggregateFamily::Enum,
            carrier: MirAggregateCarrier::OpaqueRuntimeHandle,
            element: None,
            capacity: None,
        })
    );
    assert_eq!(
        table
            .layout_for_type(MirType::semantic(tuple_ty))
            .expect("tuple layout")
            .kind,
        MirLayoutKind::Aggregate(MirAggregateLayout {
            family: MirAggregateFamily::Tuple,
            carrier: MirAggregateCarrier::PhysicalValue,
            element: None,
            capacity: None,
        })
    );
}

#[test]
fn layout_table_classifies_numeric_tensors_as_device_views() {
    let mut types = TypeTable::new();
    let f32_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let dim = types.intern_index(IndexExpr::Literal(4));
    let tensor_ty = types.intern(Type::Tensor(f32_ty, dim));
    let table = MirLayoutTable::from_types(&types);

    assert_eq!(
        table
            .layout_for_type(MirType::semantic(tensor_ty))
            .expect("tensor layout")
            .kind,
        MirLayoutKind::DeviceView(MirDeviceViewLayout::materialized_row_major(MirScalarLayout::F32, 1))
    );
}

#[test]
fn dense_tensor_layout_records_materialized_row_major_facts() {
    let mut types = TypeTable::new();
    let f32_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let rows = types.intern_index(IndexExpr::Literal(2));
    let cols = types.intern_index(IndexExpr::Literal(3));
    let shape = types.intern_index(IndexExpr::Tuple(vec![rows, cols]));
    let tensor_ty = types.intern(Type::Tensor(f32_ty, shape));
    let table = MirLayoutTable::from_types(&types);

    let layout = table
        .layout_for_type(MirType::semantic(tensor_ty))
        .expect("tensor layout");

    assert_eq!(
        layout.kind,
        MirLayoutKind::DeviceView(MirDeviceViewLayout {
            element: MirDeviceViewElement::Scalar(MirScalarLayout::F32),
            rank: 2,
            storage: MirTensorStorageLayout::HostOwned,
            locus: MirTensorLocusLayout::Host,
            view: MirTensorViewLayout::Materialized,
            offset_elements: 0,
            contiguous: true,
            row_major: true,
            stride: MirTensorStrideLayout::RowMajor,
            length: None,
        })
    );
}

#[test]
fn dense_tensor_layout_can_represent_axis0_views() {
    assert_eq!(
        MirDeviceViewLayout::axis0_view(MirScalarLayout::F32, 2, 3, 3),
        MirDeviceViewLayout {
            element: MirDeviceViewElement::Scalar(MirScalarLayout::F32),
            rank: 2,
            storage: MirTensorStorageLayout::HostOwned,
            locus: MirTensorLocusLayout::Host,
            view: MirTensorViewLayout::Axis0View,
            offset_elements: 3,
            contiguous: false,
            row_major: true,
            stride: MirTensorStrideLayout::Axis0 { stride_elements: 3 },
            length: None,
        }
    );
}

#[test]
fn dense_tensor_layout_can_represent_device_handle_locus() {
    assert_eq!(
        MirDeviceViewLayout::device_handle_row_major(MirScalarLayout::F32, 1),
        MirDeviceViewLayout {
            element: MirDeviceViewElement::Scalar(MirScalarLayout::F32),
            rank: 1,
            storage: MirTensorStorageLayout::DeviceHandle,
            locus: MirTensorLocusLayout::Device,
            view: MirTensorViewLayout::Materialized,
            offset_elements: 0,
            contiguous: true,
            row_major: true,
            stride: MirTensorStrideLayout::RowMajor,
            length: None,
        }
    );
}

#[test]
fn layout_table_classifies_vectors_as_device_vectors() {
    let mut types = TypeTable::new();
    let f32_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let width = types.intern_index(IndexExpr::Literal(4));
    let vector_ty = types.intern(Type::Vector(f32_ty, width));
    let table = MirLayoutTable::from_types(&types);

    assert_eq!(
        table
            .layout_for_type(MirType::semantic(vector_ty))
            .expect("vector layout")
            .kind,
        MirLayoutKind::DeviceVector(MirDeviceVectorLayout { element: MirScalarLayout::F32, width: 4, align: 16 })
    );
}

#[test]
fn layout_table_classifies_matrices_as_device_matrices() {
    let mut types = TypeTable::new();
    let f32_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let rows = types.intern_index(IndexExpr::Literal(4));
    let columns = types.intern_index(IndexExpr::Literal(4));
    let shape = types.intern_index(IndexExpr::Tuple(vec![rows, columns]));
    let matrix_ty = types.intern(Type::Matrix(f32_ty, shape));
    let table = MirLayoutTable::from_types(&types);

    assert_eq!(
        table
            .layout_for_type(MirType::semantic(matrix_ty))
            .expect("matrix layout")
            .kind,
        MirLayoutKind::DeviceMatrix(MirDeviceMatrixLayout {
            element: MirScalarLayout::F32,
            rows: 4,
            columns: 4,
            align: 16,
        })
    );
}

#[test]
fn layout_table_classifies_i32_atomic_as_atomic_layout() {
    let mut types = TypeTable::new();
    let i32_ty = types.sized_numeric(Primitive::Numerus, NumericWidth::I32);
    let atomic_ty = types.atomic(i32_ty);
    let table = MirLayoutTable::from_types(&types);
    let layout = table
        .layout_for_type(MirType::semantic(atomic_ty))
        .expect("atomic layout");

    assert_eq!(
        layout.kind,
        MirLayoutKind::Atomic(MirAtomicLayout { element: MirScalarLayout::I32 })
    );
    assert_eq!(layout.size_bits, Some(32));
    assert_eq!(layout.align_bits, Some(32));
}

#[test]
fn layout_table_classifies_u32_vec3_with_register_alignment() {
    let mut types = TypeTable::new();
    let u32_ty = types.sized_numeric(Primitive::Numerus, NumericWidth::U32);
    let width = types.intern_index(IndexExpr::Literal(3));
    let vector_ty = types.intern(Type::Vector(u32_ty, width));
    let table = MirLayoutTable::from_types(&types);

    assert_eq!(
        table
            .layout_for_type(MirType::semantic(vector_ty))
            .expect("vec3 layout")
            .kind,
        MirLayoutKind::DeviceVector(MirDeviceVectorLayout { element: MirScalarLayout::U32, width: 3, align: 16 })
    );
}

#[test]
fn layout_table_classifies_unknown_rank_tensors_as_runtime_aggregates() {
    let mut types = TypeTable::new();
    let f32_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let shape = types.unspecified_shape();
    let tensor_ty = types.intern(Type::Tensor(f32_ty, shape));
    let table = MirLayoutTable::from_types(&types);

    assert_eq!(
        table
            .layout_for_type(MirType::semantic(tensor_ty))
            .expect("tensor runtime layout")
            .kind,
        MirLayoutKind::Aggregate(MirAggregateLayout {
            family: MirAggregateFamily::TensorRuntime,
            carrier: MirAggregateCarrier::OpaqueRuntimeHandle,
            element: None,
            capacity: None,
        })
    );
}

#[test]
fn packed_numeric_layout_records_toy_u4_descriptor_and_reject_issue() {
    let mut table = MirLayoutTable::default();
    let id = table.insert_layout(MirLayoutKind::PackedNumeric(MirPackedNumericLayout::toy_u4()));
    let layout = table.layout(id).expect("packed numeric layout row");

    assert_eq!(
        layout.kind,
        MirLayoutKind::PackedNumeric(MirPackedNumericLayout {
            format: MirPackedNumericFormat::ToyU4,
            packed_element_width_bits: 4,
            values_per_byte: 2,
            block_values: 8,
            bit_order: MirPackedBitOrder::LowNibbleFirst,
            widened_element: MirScalarLayout::F32,
            has_scale: true,
            has_zero_point: true,
        })
    );
    assert_eq!(layout.size_bits, None);
    assert_eq!(layout.align_bits, None);

    let MirLayoutKind::PackedNumeric(packed) = layout.kind else {
        panic!("expected packed numeric layout");
    };
    assert_eq!(
        packed.unsupported_execution_issue(),
        "mir_packed_numeric_toy_u4_execution_unsupported"
    );
}

#[test]
fn layout_table_resolves_multi_hop_alias_chains_through_the_memo() {
    let mut types = TypeTable::new();
    let f32_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let alias_7 = types.intern(Type::Alias(DefId(7), f32_ty));
    let alias_8 = types.intern(Type::Alias(DefId(8), alias_7));
    let table = MirLayoutTable::from_types(&types);

    let layout = table
        .layout_for_type(MirType::semantic(alias_8))
        .expect("multi-hop alias layout");

    assert_eq!(layout.kind, MirLayoutKind::Scalar(MirScalarLayout::F32));
}

#[test]
fn layout_table_resolves_shared_alias_element_across_sibling_vectors() {
    let mut types = TypeTable::new();
    let f32_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let alias_ty = types.intern(Type::Alias(DefId(7), f32_ty));
    let width_2 = types.intern_index(IndexExpr::Literal(2));
    let width_4 = types.intern_index(IndexExpr::Literal(4));
    let vec2_ty = types.intern(Type::Vector(alias_ty, width_2));
    let vec4_ty = types.intern(Type::Vector(alias_ty, width_4));
    let table = MirLayoutTable::from_types(&types);

    let vec2_layout = table
        .layout_for_type(MirType::semantic(vec2_ty))
        .expect("vec2 layout");
    let vec4_layout = table
        .layout_for_type(MirType::semantic(vec4_ty))
        .expect("vec4 layout");

    // The alias element is shared by both sibling vectors: the per-path
    // visiting set must not flag a false cycle on the second branch, and the
    // memo collapses that re-walk to an O(1) lookup.
    assert_eq!(
        vec2_layout.kind,
        MirLayoutKind::DeviceVector(MirDeviceVectorLayout { element: MirScalarLayout::F32, width: 2, align: 8 })
    );
    assert_eq!(
        vec4_layout.kind,
        MirLayoutKind::DeviceVector(MirDeviceVectorLayout { element: MirScalarLayout::F32, width: 4, align: 16 })
    );
}
