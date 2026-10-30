//! MLC-1: `octeti<N>` byte `DeviceView` layout facts.
//!
//! A statically-bounded byte buffer resolves to a rank-1 `u8` device view
//! (host-staged per the existing tensor facts) with its static length; a
//! capacity that is not a compile-time literal stays opaque, as does the
//! unbounded `octeti` primitive.
//!
//! MLC-3 extends the census: `BoundedArray<T, N>` / `BoundedAscii<N>` /
//! `BoundedTextus<N>` moved from opaque-by-design to physical aggregate rows
//! (14+3 physical / 21 opaque-by-design); the pin-table counts below are the
//! corrected census, never weakened.

use crate::{
    MirAggregateCarrier, MirAggregateFamily, MirAggregateLayout, MirDeviceVectorLayout,
    MirDeviceViewElement, MirDeviceViewLayout, MirLayoutKind, MirLayoutTable, MirScalarLayout,
    MirTensorLocusLayout, MirTensorStorageLayout, MirTensorStrideLayout, MirTensorViewLayout,
    MirType,
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
            element: MirDeviceViewElement::Scalar(MirScalarLayout::U8),
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

#[test]
fn applied_generic_struct_uses_physical_aggregate_carrier() {
    let mut types = TypeTable::new();
    let int = types.primitive(Primitive::Numerus);
    let base = types.intern(Type::Struct(radix_types::DefId(500)));
    let applied = types.intern(Type::Applied(
        base,
        vec![radix_types::AppliedArg::Type(int)],
    ));
    let table = MirLayoutTable::from_types(&types);

    let layout = table
        .layout_for_type(MirType::semantic(applied))
        .expect("generic struct instance layout fact");
    assert_eq!(
        layout.kind,
        MirLayoutKind::Aggregate(MirAggregateLayout {
            family: MirAggregateFamily::Struct,
            carrier: MirAggregateCarrier::PhysicalValue,
            element: None,
            capacity: None,
        })
    );
}

// MLC-2: the fallthrough law and the pin table — the layout census made
// permanent.
//
// Every `Type` variant maps to an intended bucket (physical / opaque
// aggregate / opaque-by-design / compiler-internal; `Alias` is transparent),
// and every sized-numeric `(family, width)` combination maps to a named arm
// or a loud resolver error. The exhaustive matches below are the
// compile-time guards: a new variant or width without a row here stops
// compilation, and the runtime assertions pin the corrected census
// (14 physical / 6 opaque-agg / 24 OpaqueHandle-by-design / 5 internal).

use handroll::FxHashMap;
use radix_lexer::Symbol as LexSymbol;
use radix_types::{
    DefId, FuncSig, InferVar, InstansPrecision, MarkerFamily, NumericWidth, SemanticMutability,
};

/// The four census buckets (goal census table; the sized-numeric row is a
/// Physical census seat — its unmapped widths are loud errors, not opaques).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Bucket {
    Physical,
    OpaqueAggregate,
    OpaqueHandle,
    Internal,
}

/// Intended bucket for every `Type` variant. The exhaustive match is the
/// audit made permanent: a new variant without a row here fails to compile.
/// `Alias` is transparent and is refused a census seat on purpose.
fn intended_bucket(ty: &Type, types: &TypeTable) -> Bucket {
    match ty {
        Type::Primitive(
            Primitive::Bivalens
            | Primitive::Numerus
            | Primitive::Fractus
            | Primitive::Vacuum
            | Primitive::Numquam,
        ) => Bucket::Physical,
        Type::Primitive(
            Primitive::Ascii
            | Primitive::Littera
            | Primitive::Textus
            | Primitive::Nihil
            | Primitive::Ignotum
            | Primitive::Octeti
            | Primitive::Regex
            | Primitive::Match
            | Primitive::Json
            | Primitive::Valor
            | Primitive::Instans,
        ) => Bucket::OpaqueHandle,
        Type::SizedNumeric(_, _)
        | Type::ModularWord(_)
        | Type::Bits(_)
        | Type::RangedInt { .. }
        | Type::SaturatingWord(_) => Bucket::Physical,
        // MLC-4: `sparsa` with a scalar element and resolvable figura rank
        // resolves a sparse-marker `DeviceView` row.
        Type::Tensor(_, _)
        | Type::Sparsa(_, _)
        | Type::Vector(_, _)
        | Type::Matrix(_, _)
        | Type::Atomic(_) => Bucket::Physical,
        Type::Record(_) | Type::Struct(_) | Type::Tuple(_) => Bucket::Physical,
        Type::Applied(base, _) => match types.get(*base) {
            Type::Struct(_) => Bucket::Physical,
            _ => Bucket::OpaqueHandle,
        },
        // MLC-3: the bounded families with literal capacities are physical
        // aggregates (row-of-T / byte-storage + length member).
        Type::BoundedArray { .. } | Type::BoundedTextus { .. } | Type::BoundedAscii { .. } => {
            Bucket::Physical
        }
        Type::Array(_)
        | Type::RangedArray { .. }
        | Type::Map(_, _)
        | Type::Ledger(_, _)
        | Type::Set(_)
        | Type::Option(_)
        | Type::Enum(_)
        | Type::Union(_)
        | Type::Intersection(_) => Bucket::OpaqueAggregate,
        Type::Param(_)
        | Type::Infer(_)
        | Type::InferUnion(_)
        | Type::MarkerHole { .. }
        | Type::Error
        | Type::TensorEdgePolicy(_) => Bucket::Internal,
        Type::Alias(_, _) => panic!("alias is transparent; pinned separately, not a census row"),
        // N7E-1: the policy-float cell has no layout until N7E-3; it resolves to
        // a loud missing-fact error, pinned by `policy_float_has_no_mir_layout_*`.
        Type::PolicyFloat(_, _) => {
            panic!("policy-float has no MIR layout yet; pinned separately, not a census row")
        }
        Type::BoundedOcteti { .. }
        | Type::Intervallum(..)
        | Type::SizedInstans(_)
        | Type::Promissum(_)
        | Type::PromissumFailable(_, _)
        | Type::Cursor(_)
        | Type::AsyncCursor(_, _)
        | Type::Queue(_)
        | Type::BoundedQueue { .. }
        | Type::Stack(_)
        | Type::BoundedStack { .. }
        | Type::Ref(_, _)
        | Type::Interface(_)
        | Type::Func(_) => Bucket::OpaqueHandle,
    }
}

/// One representative `Type` per census row (53 rows: 15 Primitive-expanded
/// variants + 38 other variants; `Alias` excluded). Nested ids are interned
/// into `types` so the rows are resolvable.
fn census_rows(types: &mut TypeTable) -> Vec<Type> {
    let int64_ty = types.sized_numeric(Primitive::Numerus, NumericWidth::I64);
    let int32_ty = types.sized_numeric(Primitive::Numerus, NumericWidth::I32);
    let float64_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F64);
    let cap = types.intern_index(IndexExpr::Literal(8));
    let dynamic_cap = types.intern_index(IndexExpr::Unspecified);
    let interval_step = types.intern_index(IndexExpr::Literal(1));
    let tensor_shape = types.intern_index(IndexExpr::Literal(1));
    let vector_width = tensor_shape;
    let two = types.intern_index(IndexExpr::Literal(2));
    let matrix_shape = types.intern_index(IndexExpr::Tuple(vec![two, two]));

    let mut rows = Vec::with_capacity(51);
    // Physical (14): the five physical primitives, the sized/word numerics,
    // the device-class rows, and the physical aggregates.
    for prim in [
        Primitive::Bivalens,
        Primitive::Numerus,
        Primitive::Fractus,
        Primitive::Vacuum,
        Primitive::Numquam,
    ] {
        rows.push(Type::Primitive(prim));
    }
    rows.push(Type::SizedNumeric(Primitive::Numerus, NumericWidth::I64));
    rows.push(Type::ModularWord(NumericWidth::U32));
    rows.push(Type::SaturatingWord(NumericWidth::I16));
    // Need: a ranged cell lays out as its width.
    rows.push(Type::RangedInt {
        policy: radix_types::IntPolicy::Saturating,
        width: NumericWidth::U32,
        lo: 0,
        hi: 7,
    });
    rows.push(Type::Tensor(float64_ty, tensor_shape));
    rows.push(Type::Vector(float64_ty, vector_width));
    rows.push(Type::Matrix(float64_ty, matrix_shape));
    rows.push(Type::Atomic(int32_ty));
    rows.push(Type::Record(FxHashMap::from_iter([(
        LexSymbol(1),
        int64_ty,
    )])));
    rows.push(Type::Struct(DefId(10)));
    rows.push(Type::Tuple(vec![int64_ty]));
    // MLC-3: the bounded families with literal capacities are physical
    // aggregates — row-of-T and byte-storage + length member.
    rows.push(Type::BoundedArray {
        element: float64_ty,
        capacity: cap,
    });
    rows.push(Type::BoundedTextus { capacity: cap });
    rows.push(Type::BoundedAscii { capacity: cap });
    // MLC-4: the sparse seat is the scalar+literal-figura state; the
    // non-scalar / unresolvable-figura states stay opaque (MLC-6).
    rows.push(Type::Sparsa(float64_ty, tensor_shape));
    // Opaque aggregate (6).
    rows.push(Type::Array(float64_ty));
    rows.push(Type::Map(int64_ty, float64_ty));
    rows.push(Type::Set(float64_ty));
    rows.push(Type::Option(float64_ty));
    rows.push(Type::Enum(DefId(11)));
    rows.push(Type::Union(vec![int64_ty, float64_ty]));
    // OpaqueHandle by design (21): the nine non-physical primitives plus the
    // twelve opaque carriers. `octeti<N>` with a literal capacity refines to
    // a byte `DeviceView` (pinned by MLC-1) and the bounded array/ascii/
    // textus families to physical aggregates (MLC-3); the census seats here
    // are the runtime-length / non-literal-capacity states, which stay opaque
    // by design (MLC-6).
    for prim in [
        Primitive::Ascii,
        Primitive::Textus,
        Primitive::Nihil,
        Primitive::Ignotum,
        Primitive::Octeti,
        Primitive::Regex,
        Primitive::Match,
        Primitive::Json,
        Primitive::Valor,
        Primitive::Instans,
    ] {
        rows.push(Type::Primitive(prim));
    }
    rows.push(Type::BoundedOcteti {
        capacity: dynamic_cap,
    });
    rows.push(Type::Intervallum(float64_ty, interval_step));
    rows.push(Type::SizedInstans(InstansPrecision::Millis));
    rows.push(Type::Promissum(float64_ty));
    rows.push(Type::PromissumFailable(float64_ty, int64_ty));
    rows.push(Type::Cursor(float64_ty));
    rows.push(Type::AsyncCursor(float64_ty, int64_ty));
    rows.push(Type::Ref(SemanticMutability::Immutable, float64_ty));
    rows.push(Type::Interface(DefId(12)));
    rows.push(Type::Func(FuncSig {
        type_params: vec![],
        params: vec![],
        ret: float64_ty,
        err: None,
        is_async: false,
        is_generator: false,
    }));
    rows.push(Type::Applied(
        int64_ty,
        vec![radix_types::AppliedArg::Type(float64_ty)],
    ));
    // Internal (6): no runtime layout row, including type-only edge policies.
    rows.push(Type::Param(LexSymbol(2)));
    rows.push(Type::Infer(InferVar(0)));
    rows.push(Type::InferUnion(InferVar(1)));
    rows.push(Type::MarkerHole {
        family: MarkerFamily::Modulus,
        var: InferVar(2),
    });
    rows.push(Type::Error);
    rows.push(Type::TensorEdgePolicy(
        radix_types::BuiltinTensorEdgePolicy::Clamp,
    ));
    assert_eq!(
        rows.len(),
        53,
        "the census has 53 Primitive-expanded rows including the type-only policy marker"
    );
    rows
}

#[test]
fn pin_table_census_is_53_rows_across_the_four_buckets() {
    let mut types = TypeTable::new();
    let rows = census_rows(&mut types);

    let mut physical = 0;
    let mut aggregate = 0;
    let mut opaque = 0;
    let mut internal = 0;
    for ty in &rows {
        match intended_bucket(ty, &types) {
            Bucket::Physical => physical += 1,
            Bucket::OpaqueAggregate => aggregate += 1,
            Bucket::OpaqueHandle => opaque += 1,
            Bucket::Internal => internal += 1,
        }
    }
    assert_eq!(
        physical, 20,
        "physical census rows (MLC-4: +1 sparsa; D11.1: +1 saturatus; d5d00916: +1 ranged integer)"
    );
    assert_eq!(aggregate, 6, "opaque-aggregate census rows");
    assert_eq!(
        opaque, 21,
        "opaque-by-design census rows (MLC-4: -1 to physical; RX-3a: +1 match)"
    );
    assert_eq!(internal, 6, "internal census rows");
}

/// The resolver's nominal outcome agrees with each census bucket: opaque
/// seats resolve opaque, aggregate seats resolve aggregates, physical seats
/// resolve concrete layouts, and internal seats carry no layout row.
#[test]
fn pin_table_buckets_agree_with_the_resolver_on_nominal_rows() {
    let mut types = TypeTable::new();
    let rows = census_rows(&mut types);
    // Intern every census row first: the layout table is built once over the
    // complete arena, so row types are present when the resolver runs.
    let interned: Vec<(radix_types::TypeId, Bucket)> = rows
        .iter()
        .map(|ty| {
            let bucket = intended_bucket(ty, &types);
            (types.intern(ty.clone()), bucket)
        })
        .collect();
    let table = MirLayoutTable::from_types(&types);

    for (id, bucket) in interned {
        match bucket {
            Bucket::OpaqueHandle => {
                let layout = table
                    .layout_for_type(MirType::semantic(id))
                    .expect("opaque seat must resolve");
                assert_eq!(
                    layout.kind,
                    MirLayoutKind::OpaqueHandle,
                    "opaque seat resolved off-bucket"
                );
            }
            Bucket::OpaqueAggregate => {
                let layout = table
                    .layout_for_type(MirType::semantic(id))
                    .expect("aggregate seat must resolve");
                assert!(
                    matches!(layout.kind, MirLayoutKind::Aggregate(_)),
                    "aggregate seat resolved off-bucket: {layout:?}"
                );
            }
            Bucket::Physical => {
                let layout = table
                    .layout_for_type(MirType::semantic(id))
                    .expect("physical seat must resolve");
                assert!(
                    !matches!(layout.kind, MirLayoutKind::OpaqueHandle),
                    "physical seat must not resolve opaque: {layout:?}"
                );
            }
            Bucket::Internal => {
                table
                    .layout_for_type(MirType::semantic(id))
                    .expect_err("internal seats carry no layout row");
            }
        }
    }
}

#[test]
fn alias_is_transparent_and_not_a_census_row() {
    let mut types = TypeTable::new();
    let float64_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F64);
    let alias = Type::Alias(DefId(20), float64_ty);

    let refused = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        intended_bucket(&alias, &types)
    }));
    assert!(refused.is_err(), "alias must not occupy a census bucket");

    let alias_ty = types.intern(alias);
    let table = MirLayoutTable::from_types(&types);
    let layout = table
        .layout_for_type(MirType::semantic(alias_ty))
        .expect("alias resolves through its target");
    assert_eq!(layout.kind, MirLayoutKind::Scalar(MirScalarLayout::F64));
}

/// The derived `MirLayout` row carries a scalar's width, or `None` for a
/// kind with no fixed size (a device view here). Pins the size/align
/// derivation for both branches of the fixed-scalar classification.
#[test]
fn layout_rows_carry_size_and_align_for_scalars_and_none_for_unsized() {
    let mut types = TypeTable::new();
    let int64_ty = types.sized_numeric(Primitive::Numerus, NumericWidth::I64);
    let octeti8 = bounded_octeti(&mut types, IndexExpr::Literal(8));
    let table = MirLayoutTable::from_types(&types);

    let scalar = table
        .layout_for_type(MirType::semantic(int64_ty))
        .expect("i64 scalar layout");
    assert_eq!(scalar.size_bits, Some(64));
    assert_eq!(scalar.align_bits, Some(64));

    let view_row = table
        .layout_for_type(MirType::semantic(octeti8))
        .expect("octeti<8> device-view layout");
    assert!(matches!(view_row.kind, MirLayoutKind::DeviceView(_)));
    assert_eq!(view_row.size_bits, None);
    assert_eq!(view_row.align_bits, None);
}

/// Sized-numeric outcome under the fallthrough law.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SizedNumericOutcome {
    PhysicalScalar,
    LoudError,
}
/// Sized-numeric width sub-table: for each width, the outcome on `numerus`
/// and on `fractus`. A new width lands a row here or the match stops
/// compiling.
fn width_sub_table(width: NumericWidth) -> (SizedNumericOutcome, SizedNumericOutcome) {
    match width {
        NumericWidth::I8
        | NumericWidth::I16
        | NumericWidth::I32
        | NumericWidth::I64
        | NumericWidth::U8
        | NumericWidth::U16
        | NumericWidth::U32
        | NumericWidth::U64 => (
            SizedNumericOutcome::PhysicalScalar,
            SizedNumericOutcome::LoudError,
        ),
        NumericWidth::F16 | NumericWidth::F32 | NumericWidth::F64 => (
            SizedNumericOutcome::LoudError,
            SizedNumericOutcome::PhysicalScalar,
        ),
        // bf16-U3: `fractus<bf16>` resolves its own named scalar row;
        // `numerus<bf16>` stays a loud missing fact.
        NumericWidth::BF16 => (
            SizedNumericOutcome::LoudError,
            SizedNumericOutcome::PhysicalScalar,
        ),
        // Kernel-wide types ruling 2026-10-28: `numerus<d64>` is the `D64`
        // scalar layout (an `i64` mantissa at scale 10^-8); `fractus<d64>` is
        // not a type and stays a loud error.
        NumericWidth::D64 => (
            SizedNumericOutcome::PhysicalScalar,
            SizedNumericOutcome::LoudError,
        ),
        // inf track U4: `numerus<inf>` is the unsized `Magnus` scalar layout
        // (ruling 46); `fractus<inf>` is not a type and stays a loud error.
        NumericWidth::Inf => (
            SizedNumericOutcome::PhysicalScalar,
            SizedNumericOutcome::LoudError,
        ),
    }
}

fn all_numeric_widths() -> [NumericWidth; 14] {
    [
        NumericWidth::I8,
        NumericWidth::I16,
        NumericWidth::I32,
        NumericWidth::I64,
        NumericWidth::U8,
        NumericWidth::U16,
        NumericWidth::U32,
        NumericWidth::U64,
        NumericWidth::F16,
        NumericWidth::BF16,
        NumericWidth::F32,
        NumericWidth::F64,
        NumericWidth::D64,
        NumericWidth::Inf,
    ]
}

fn resolve_outcome(table: &MirLayoutTable, ty: radix_types::TypeId) -> SizedNumericOutcome {
    match table.layout_for_type(MirType::semantic(ty)) {
        Ok(layout) => match layout.kind {
            MirLayoutKind::Scalar(_) => SizedNumericOutcome::PhysicalScalar,
            other => panic!("sized numeric resolved to a non-scalar layout: {other:?}"),
        },
        Err(err) => {
            assert_eq!(
                err.issue, "mir_layout_missing_type_fact",
                "loud error issue: {err:?}"
            );
            // Named missing-fact row: the failing type id is the structured
            // identity of the fallthrough-law error (no named layout arm).
            assert_eq!(
                err.ty,
                Some(ty),
                "fallthrough error names the type: {err:?}"
            );
            SizedNumericOutcome::LoudError
        }
    }
}

/// Every one of the 24 sized-numeric combinations matches the sub-table: a
/// named scalar arm or a loud `mir_layout_missing_type_fact` error naming the
/// width. This is the fallthrough law at runtime.
#[test]
fn every_sized_numeric_width_matches_its_sub_table_row() {
    for width in all_numeric_widths() {
        let mut types = TypeTable::new();
        let numerus = types.sized_numeric(Primitive::Numerus, width);
        let fractus = types.sized_numeric(Primitive::Fractus, width);
        let table = MirLayoutTable::from_types(&types);
        let (numerus_outcome, fractus_outcome) = width_sub_table(width);
        let numerus_marker = width.faber_marker();
        assert_eq!(
            resolve_outcome(&table, numerus),
            numerus_outcome,
            "numerus<{numerus_marker}>"
        );
        if width == NumericWidth::D64 {
            assert_eq!(
                table
                    .layout_for_type(MirType::semantic(numerus))
                    .unwrap()
                    .kind,
                MirLayoutKind::Scalar(MirScalarLayout::D64)
            );
        }
        assert_eq!(
            resolve_outcome(&table, fractus),
            fractus_outcome,
            "fractus<{numerus_marker}>"
        );
    }
}

/// The named pending-error rows: `fractus<bf16>` stays loud until bf16 owns
/// `MirScalarLayout::BF16`, and family-mismatch widths are loud too.
#[test]
fn unmapped_sized_numerics_error_loudly_with_the_width_named() {
    for (prim, width) in [
        (Primitive::Numerus, NumericWidth::F32),
        (Primitive::Fractus, NumericWidth::U32),
    ] {
        let mut types = TypeTable::new();
        let ty = types.sized_numeric(prim, width);
        let table = MirLayoutTable::from_types(&types);
        let err = table
            .layout_for_type(MirType::semantic(ty))
            .expect_err("unmapped sized numeric must not resolve a layout");
        assert_eq!(
            err.issue, "mir_layout_missing_type_fact",
            "loud error issue: {err:?}"
        );
        // The width is named through the structured type id, not prose.
        assert_eq!(err.ty, Some(ty), "width named by type id: {err:?}");
    }
}

// MLC-3: the bounded families own physical layout rows — `BoundedArray<T, N>`
// is a fixed-capacity row of `T`, and `ascii<N>`/`textus<N>` are byte
// aggregates (`u8 × N` storage plus the runtime length member). These were
// the three OpaqueHandle resolutions the unit pinned red first; the rows
// below are the corrected facts. Scalar reads (OQ1 default) stay with AEU-7 —
// no read-surface change here.

#[test]
fn bounded_array_resolves_physical_row_of_t_with_element_fact() {
    let mut types = TypeTable::new();
    let numerus = types.sized_numeric(Primitive::Numerus, NumericWidth::I64);
    let cap = types.intern_index(IndexExpr::Literal(4));
    let array_ty = types.intern(Type::BoundedArray {
        element: numerus,
        capacity: cap,
    });
    let table = MirLayoutTable::from_types(&types);

    let layout = table
        .layout_for_type(MirType::semantic(array_ty))
        .expect("BoundedArray layout fact");
    assert_eq!(
        layout.kind,
        MirLayoutKind::Aggregate(MirAggregateLayout {
            family: MirAggregateFamily::BoundedArray,
            carrier: MirAggregateCarrier::PhysicalValue,
            element: Some(MirScalarLayout::I64),
            capacity: Some(4),
        })
    );
}

#[test]
fn bounded_array_with_non_scalar_element_keeps_physical_row_without_element_fact() {
    let mut types = TypeTable::new();
    let numerus = types.sized_numeric(Primitive::Numerus, NumericWidth::I64);
    let cap = types.intern_index(IndexExpr::Literal(2));
    let row = types.intern(Type::Tuple(vec![numerus]));
    let array_ty = types.intern(Type::BoundedArray {
        element: row,
        capacity: cap,
    });
    let table = MirLayoutTable::from_types(&types);

    let layout = table
        .layout_for_type(MirType::semantic(array_ty))
        .expect("BoundedArray layout fact");
    assert!(matches!(
        layout.kind,
        MirLayoutKind::Aggregate(MirAggregateLayout {
            family: MirAggregateFamily::BoundedArray,
            carrier: MirAggregateCarrier::PhysicalValue,
            element: None,
            capacity: Some(2),
        })
    ));
}

#[test]
fn bounded_ascii_and_textus_resolve_physical_byte_aggregates_with_length_member() {
    let mut types = TypeTable::new();
    let cap = types.intern_index(IndexExpr::Literal(16));
    let textus_ty = types.intern(Type::BoundedTextus { capacity: cap });
    let ascii_ty = types.intern(Type::BoundedAscii { capacity: cap });
    let table = MirLayoutTable::from_types(&types);

    let expected = |family| {
        MirLayoutKind::Aggregate(MirAggregateLayout {
            family,
            carrier: MirAggregateCarrier::PhysicalValue,
            element: Some(MirScalarLayout::U8),
            capacity: Some(16),
        })
    };
    let textus_layout = table
        .layout_for_type(MirType::semantic(textus_ty))
        .expect("textus<magnitudo 16> layout fact");
    assert_eq!(
        textus_layout.kind,
        expected(MirAggregateFamily::BoundedTextus)
    );
    let ascii_layout = table
        .layout_for_type(MirType::semantic(ascii_ty))
        .expect("ascii<magnitudo 16> layout fact");
    assert_eq!(
        ascii_layout.kind,
        expected(MirAggregateFamily::BoundedAscii)
    );
}

// MLC-4: `Sparsa<T, Figura>` — a scalar element with a resolvable figura
// rank resolves to a `DeviceView` row carrying the sparse storage marker
// (`MirTensorStorageLayout::Sparse`); non-scalar elements and unresolvable
// figurae stay opaque as today. Every kernel admission surface keeps
// rejecting the sparse marker with a named fail-closed row (pinned below).

#[test]
fn sparsa_resolves_sparse_storage_device_view_row() {
    let mut types = TypeTable::new();
    let float64_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F64);
    let dim3 = types.intern_index(IndexExpr::Literal(3));
    let dim4 = types.intern_index(IndexExpr::Literal(4));
    let figura = types.intern_index(IndexExpr::Tuple(vec![dim3, dim4]));
    let sparsa_ty = types.intern(Type::Sparsa(float64_ty, figura));
    let table = MirLayoutTable::from_types(&types);

    let layout = table
        .layout_for_type(MirType::semantic(sparsa_ty))
        .expect("Sparsa layout fact");
    assert_eq!(
        layout.kind,
        MirLayoutKind::DeviceView(MirDeviceViewLayout {
            element: MirDeviceViewElement::Scalar(MirScalarLayout::F64),
            rank: 2,
            storage: MirTensorStorageLayout::Sparse,
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
fn sparsa_with_non_scalar_element_or_unresolvable_figura_stays_opaque() {
    let mut types = TypeTable::new();
    let float64_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F64);
    let tuple_ty = types.intern(Type::Tuple(vec![float64_ty]));
    let figura = types.intern_index(IndexExpr::Literal(1));
    let unresolved_figura = types.intern_index(IndexExpr::Unspecified);
    let rows = [
        types.intern(Type::Sparsa(tuple_ty, figura)),
        types.intern(Type::Sparsa(float64_ty, unresolved_figura)),
    ];
    let table = MirLayoutTable::from_types(&types);

    for ty in rows {
        let layout = table
            .layout_for_type(MirType::semantic(ty))
            .expect("Sparsa layout fact");
        assert_eq!(
            layout.kind,
            MirLayoutKind::OpaqueHandle,
            "non-scalar/unresolvable sparsa stays opaque"
        );
    }
}

#[test]
fn bounded_families_with_non_literal_capacity_stay_opaque() {
    let mut types = TypeTable::new();
    let numerus = types.sized_numeric(Primitive::Numerus, NumericWidth::I64);
    let dynamic_cap = types.intern_index(IndexExpr::Unspecified);
    let interval_step = types.intern_index(IndexExpr::Literal(1));
    let bounded = [
        types.intern(Type::BoundedArray {
            element: numerus,
            capacity: dynamic_cap,
        }),
        types.intern(Type::BoundedTextus {
            capacity: dynamic_cap,
        }),
        types.intern(Type::BoundedAscii {
            capacity: dynamic_cap,
        }),
    ];
    let table = MirLayoutTable::from_types(&types);

    for ty in bounded {
        let layout = table
            .layout_for_type(MirType::semantic(ty))
            .expect("bounded family layout fact");
        assert_eq!(
            layout.kind,
            MirLayoutKind::OpaqueHandle,
            "non-literal capacity has no provable bounds"
        );
    }
}

// bf16-U3: `fractus<bf16>` resolves to its own named scalar layout — never
// MissingFact and never a silent f32 row.
#[test]
fn fractus_bf16_resolves_to_named_bf16_scalar_layout() {
    let mut types = TypeTable::new();
    let bf16 = types.intern(Type::SizedNumeric(
        Primitive::Fractus,
        radix_types::NumericWidth::BF16,
    ));
    let table = MirLayoutTable::from_types(&types);

    let layout = table
        .layout_for_type(MirType::semantic(bf16))
        .expect("bf16 layout fact");
    assert_eq!(layout.kind, MirLayoutKind::Scalar(MirScalarLayout::Bf16));
    assert_eq!(MirScalarLayout::Bf16.spelling(), "bf16");
    assert_eq!(MirScalarLayout::Bf16.size_bits(), 16);
    assert!(MirScalarLayout::Bf16.is_float());
}

// N7E-3 (D2): a policy-float cell lays out as the bare float of its width.
#[test]
fn policy_float_cells_share_the_bare_float_scalar_layout() {
    use radix_types::{FloatPolicy, NumericWidth};
    let mut types = TypeTable::new();
    let widths = [
        (NumericWidth::F16, MirScalarLayout::F16),
        (NumericWidth::BF16, MirScalarLayout::Bf16),
        (NumericWidth::F32, MirScalarLayout::F32),
        (NumericWidth::F64, MirScalarLayout::F64),
    ];
    let cells: Vec<_> = widths
        .iter()
        .flat_map(|&(width, scalar)| {
            [FloatPolicy::Trapping, FloatPolicy::Saturating]
                .into_iter()
                .map(move |policy| (policy, width, scalar))
        })
        .map(|(policy, width, scalar)| (types.intern(Type::PolicyFloat(policy, width)), scalar))
        .collect();
    let table = MirLayoutTable::from_types(&types);
    for (ty, scalar) in cells {
        let layout = table
            .layout_for_type(MirType::semantic(ty))
            .expect("policy-float layout fact");
        assert_eq!(layout.kind, MirLayoutKind::Scalar(scalar));
    }
}

// G15-1: vector-element tensors get a device-view layout whose element is the
// `DeviceVector` layout. Only widths with size == alignment (2 and 4) are
// admitted; width 3 fails closed with a named reason (MSL `float3` stride).

fn vector_element_tensor(types: &mut TypeTable, width: u64) -> radix_types::TypeId {
    let f32_ty = types.sized_numeric(Primitive::Fractus, radix_types::NumericWidth::F32);
    let width_index = types.intern_index(IndexExpr::Literal(width));
    let vector_ty = types.intern(Type::Vector(f32_ty, width_index));
    let dim = types.intern_index(IndexExpr::Literal(8));
    let shape = types.intern_index(IndexExpr::Tuple(vec![dim, dim]));
    types.tensor_with_shape(vector_ty, shape)
}

#[test]
fn vector_element_tensor_resolves_device_view_with_vector_element() {
    let mut types = TypeTable::new();
    let tensor = vector_element_tensor(&mut types, 4);
    let table = MirLayoutTable::from_types(&types);

    let layout = table
        .layout_for_type(MirType::semantic(tensor))
        .expect("tensor<vector<f32, size 4>> layout fact");
    let MirLayoutKind::DeviceView(view) = layout.kind else {
        panic!("expected DeviceView, got {:?}", layout.kind);
    };
    assert_eq!(
        view.element,
        MirDeviceViewElement::Vector(MirDeviceVectorLayout {
            element: MirScalarLayout::F32,
            width: 4,
            align: 16,
        })
    );
    assert_eq!(view.rank, 2);
    assert_eq!(view.element.scalar(), None);
}

#[test]
fn vector_element_tensor_width2_is_admitted() {
    let mut types = TypeTable::new();
    let tensor = vector_element_tensor(&mut types, 2);
    let table = MirLayoutTable::from_types(&types);
    let layout = table
        .layout_for_type(MirType::semantic(tensor))
        .expect("tensor<vector<f32, size 2>> layout fact");
    assert!(matches!(
        layout.kind,
        MirLayoutKind::DeviceView(MirDeviceViewLayout {
            element: MirDeviceViewElement::Vector(MirDeviceVectorLayout { width: 2, .. }),
            ..
        })
    ));
}

#[test]
fn vector_element_tensor_width3_fails_closed_with_named_reason() {
    let mut types = TypeTable::new();
    let tensor = vector_element_tensor(&mut types, 3);
    let table = MirLayoutTable::from_types(&types);
    let error = table
        .layout_for_type(MirType::semantic(tensor))
        .expect_err("vec3-element tensor must fail closed");
    assert_eq!(error.issue, "mir_layout_missing_type_fact");
    assert!(
        error
            .args
            .contains(&("reason", "vector_width_unsupported".to_owned())),
        "unexpected reason: {error:?}"
    );
    assert!(
        error.args.contains(&("width", "3".to_owned())),
        "the refused width is named: {error:?}"
    );
}

#[test]
fn scalar_element_tensor_layout_is_unchanged() {
    let mut types = TypeTable::new();
    let f32_ty = types.sized_numeric(Primitive::Fractus, radix_types::NumericWidth::F32);
    let dim = types.intern_index(IndexExpr::Literal(8));
    let shape = types.intern_index(IndexExpr::Tuple(vec![dim, dim]));
    let tensor = types.tensor_with_shape(f32_ty, shape);
    let table = MirLayoutTable::from_types(&types);
    let layout = table
        .layout_for_type(MirType::semantic(tensor))
        .expect("tensor<f32> layout fact");
    assert_eq!(
        layout.kind,
        MirLayoutKind::DeviceView(MirDeviceViewLayout::materialized_row_major(
            MirScalarLayout::F32,
            2
        ))
    );
}

// A record element carries only the element type id: the layout table is built
// from the `TypeTable` alone and cannot see struct fields, so consumers resolve
// the byte size, alignment and field offsets with the validation context.

#[test]
fn record_element_tensor_resolves_device_view_with_record_element() {
    let mut types = TypeTable::new();
    let struct_ty = types.intern(Type::Struct(radix_types::DefId(7)));
    let dim = types.intern_index(IndexExpr::Literal(8));
    let shape = types.intern_index(IndexExpr::Tuple(vec![dim]));
    let tensor = types.tensor_with_shape(struct_ty, shape);
    let table = MirLayoutTable::from_types(&types);

    let layout = table
        .layout_for_type(MirType::semantic(tensor))
        .expect("tensor<Struct, [8]> layout fact");
    let MirLayoutKind::DeviceView(view) = layout.kind else {
        panic!("expected DeviceView, got {:?}", layout.kind);
    };
    assert_eq!(view.element, MirDeviceViewElement::Record(struct_ty));
    assert_eq!(view.element.scalar(), None);
    assert_eq!(view.rank, 1);
}
