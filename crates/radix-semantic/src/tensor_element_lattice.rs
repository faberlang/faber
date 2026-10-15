//! Canonical tensor element types and lattice pair enumeration for conversio tests.
//!
//! WHY: `tensor<A> ↦ tensor<B>` policy lives in `numeric_width::tensor_element_conversio_allowed`.
//! Tests enumerate every distinct element type and pair so typecheck/codegen cannot drift from
//! the scalar widening/narrowing lattice without a deliberate policy change.

use super::numeric_width;
use super::{NumericWidth, Primitive, TypeId, TypeTable};

/// One spellable `tensor<T>` element type in the v1 lattice (bare defaults, no alias duplicates).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TensorElementSpec {
    pub label: &'static str,
    pub faber: &'static str,
    pub rust: &'static str,
    prim: Primitive,
    width: Option<NumericWidth>,
}

/// Distinct element types: bare `numerus`/`fractus` plus non-default sized widths only.
pub const SPECS: &[TensorElementSpec] = &[
    TensorElementSpec {
        label: "numerus",
        faber: "numerus",
        rust: "i64",
        prim: Primitive::Numerus,
        width: None,
    },
    TensorElementSpec {
        label: "numerus_i8",
        faber: "i8",
        rust: "i8",
        prim: Primitive::Numerus,
        width: Some(NumericWidth::I8),
    },
    TensorElementSpec {
        label: "numerus_i16",
        faber: "i16",
        rust: "i16",
        prim: Primitive::Numerus,
        width: Some(NumericWidth::I16),
    },
    TensorElementSpec {
        label: "numerus_i32",
        faber: "i32",
        rust: "i32",
        prim: Primitive::Numerus,
        width: Some(NumericWidth::I32),
    },
    TensorElementSpec {
        label: "numerus_u8",
        faber: "u8",
        rust: "u8",
        prim: Primitive::Numerus,
        width: Some(NumericWidth::U8),
    },
    TensorElementSpec {
        label: "numerus_u16",
        faber: "u16",
        rust: "u16",
        prim: Primitive::Numerus,
        width: Some(NumericWidth::U16),
    },
    TensorElementSpec {
        label: "numerus_u32",
        faber: "u32",
        rust: "u32",
        prim: Primitive::Numerus,
        width: Some(NumericWidth::U32),
    },
    TensorElementSpec {
        label: "numerus_u64",
        faber: "u64",
        rust: "u64",
        prim: Primitive::Numerus,
        width: Some(NumericWidth::U64),
    },
    TensorElementSpec {
        label: "fractus",
        faber: "fractus",
        rust: "f64",
        prim: Primitive::Fractus,
        width: None,
    },
    TensorElementSpec {
        label: "fractus_f32",
        faber: "f32",
        rust: "f32",
        prim: Primitive::Fractus,
        width: Some(NumericWidth::F32),
    },
];

/// Resolve a lattice spec to a `TypeTable` entry.
pub fn element_type_id(types: &mut TypeTable, spec: &TensorElementSpec) -> TypeId {
    match spec.width {
        Some(width) => types.sized_numeric(spec.prim, width),
        None => types.primitive(spec.prim),
    }
}

/// Policy oracle: whether `tensor<from> ↦ tensor<to>` is accepted at typecheck.
pub fn conversio_allowed(
    types: &mut TypeTable,
    from: &TensorElementSpec,
    to: &TensorElementSpec,
) -> bool {
    let from_id = element_type_id(types, from);
    let to_id = element_type_id(types, to);
    numeric_width::tensor_element_conversio_allowed(types, from_id, to_id)
}

/// Whether codegen should emit `.convert_elements` (allowed and not assignable widening identity).
pub fn needs_convert_elements(
    types: &mut TypeTable,
    from: &TensorElementSpec,
    to: &TensorElementSpec,
) -> bool {
    let from_id = element_type_id(types, from);
    let to_id = element_type_id(types, to);
    numeric_width::tensor_element_conversio_allowed(types, from_id, to_id)
        && !types.equals(from_id, to_id)
}

/// Faber `incipit` source for one element-width conversio pair.
#[must_use]
pub fn conversio_source(from: &TensorElementSpec, to: &TensorElementSpec) -> String {
    format!(
        "incipit {{\n  sit tensor<{}, []> a ← vacua\n  sit b ← a ↦ tensor<{}, []>\n}}",
        from.faber, to.faber
    )
}

/// Count lattice pairs by acceptance and codegen emission class.
pub fn lattice_pair_counts(types: &mut TypeTable) -> (usize, usize, usize) {
    let mut allowed = 0usize;
    let mut rejected = 0usize;
    let mut emits_convert = 0usize;
    for from in SPECS {
        for to in SPECS {
            if conversio_allowed(types, from, to) {
                allowed += 1;
                if needs_convert_elements(types, from, to) {
                    emits_convert += 1;
                }
            } else {
                rejected += 1;
            }
        }
    }
    (allowed, rejected, emits_convert)
}
