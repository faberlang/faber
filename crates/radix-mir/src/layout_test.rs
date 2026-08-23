//! MLC-1: `octeti<N>` byte `DeviceView` layout facts.
//!
//! A statically-bounded byte buffer resolves to a rank-1 `u8` device view
//! (host-staged per the existing tensor facts) with its static length; a
//! capacity that is not a compile-time literal stays opaque, as does the
//! unbounded `octeti` primitive.

use crate::{
    MirDeviceViewLayout, MirLayoutKind, MirLayoutTable, MirScalarLayout, MirTensorLocusLayout,
    MirTensorStorageLayout, MirTensorStrideLayout, MirTensorViewLayout, MirType,
};
use radix_types::{IndexExpr, Primitive, Type, TypeTable};

fn bounded_octeti(types: &mut TypeTable, capacity: IndexExpr) -> radix_types::TypeId {
    let capacity = types.intern_index(capacity);
    types.intern(Type::BoundedOcteti { capacity })
}

#[test]
fn bounded_octeti_resolves_rank1_u8_device_view_with_static_length() {
    let mut types = TypeTable::new();
    let octeti8 = bounded_octeti(&mut types, IndexExpr::Literal(8));
    let table = MirLayoutTable::from_types(&types);

    let layout = table
        .layout_for_type(MirType::semantic(octeti8))
        .expect("octeti<8> layout fact");

    assert_eq!(
        layout.kind,
        MirLayoutKind::DeviceView(MirDeviceViewLayout {
            element: MirScalarLayout::U8,
            rank: 1,
            storage: MirTensorStorageLayout::HostOwned,
            locus: MirTensorLocusLayout::Host,
            view: MirTensorViewLayout::Materialized,
            offset_elements: 0,
            contiguous: true,
            row_major: true,
            stride: MirTensorStrideLayout::RowMajor,
            length: Some(8),
        })
    );
}

#[test]
fn bounded_octeti_with_non_literal_capacity_stays_opaque() {
    let mut types = TypeTable::new();
    let octeti_dyn = bounded_octeti(&mut types, IndexExpr::Unspecified);
    let table = MirLayoutTable::from_types(&types);

    let layout = table
        .layout_for_type(MirType::semantic(octeti_dyn))
        .expect("octeti layout fact");
    assert_eq!(layout.kind, MirLayoutKind::OpaqueHandle);
}

#[test]
fn unbounded_octeti_primitive_stays_opaque() {
    let types = TypeTable::new();
    let octeti = types.primitive(Primitive::Octeti);
    let table = MirLayoutTable::from_types(&types);

    let layout = table
        .layout_for_type(MirType::semantic(octeti))
        .expect("octeti layout fact");
    assert_eq!(layout.kind, MirLayoutKind::OpaqueHandle);
}
