//! Regression suites for the device-safe lowerability gate, split by
//! gate theme (each theme module owns its fixture builders and tests).
//! Shared fixture builders live here.

use super::*;
use crate::{MirFunctionId, MirType};
use radix_lexer::Span;
use radix_types::{IndexExpr, NumericWidth, Primitive, Type, TypeTable};
use rustc_hash::FxHashMap;

mod companion;
mod entry_roles;
mod fold_artifact;
mod genus_param;
mod i64_axis_const;
mod i64_const_copy;
mod live_tuple;
mod multi_output;
mod register_conversio;
mod tensor_extent;
mod transformer;

fn test_span() -> Span {
    Span::default()
}

fn f32_ty(types: &mut TypeTable) -> MirType {
    MirType::semantic(types.sized_numeric(Primitive::Fractus, NumericWidth::F32))
}

/// A rank-2 `tf32[M, N]` tensor type with static literal dims (the MLP
/// operand shape the device path folds).
fn rank2_tensor_ty(types: &mut TypeTable, dims: [u64; 2]) -> MirType {
    let f32 = f32_ty(types).semantic_id();
    let dim_ids = dims
        .iter()
        .map(|&dim| types.intern_index(IndexExpr::Literal(dim)))
        .collect::<Vec<_>>();
    let shape = types.intern_index(IndexExpr::Tuple(dim_ids));
    MirType::semantic(types.intern(Type::Tensor(f32, shape)))
}

/// The `lista<numerus>` shape-list type `magnitudines()` returns.
fn shape_lista_ty(types: &mut TypeTable) -> MirType {
    MirType::semantic(types.array(types.primitive(Primitive::Numerus)))
}

/// A single-element tuple of one tensor type (the multi-output result
/// contract).
fn tuple_of_one_ty(types: &mut TypeTable, element: MirType) -> MirType {
    MirType::semantic(types.intern(Type::Tuple(vec![element.semantic_id()])))
}

/// A device context marking `MirFunctionId(0)` as a device kernel (the
/// target-entry gate's routing surface).
fn kernel_device_context() -> MirDeviceContext {
    MirDeviceContext {
        by_function_id: FxHashMap::from_iter([(MirFunctionId(0), MirDeviceRole::Kernel)]),
        ..MirDeviceContext::default()
    }
}

/// A kernel-surface `i64` type (the `LayerNorm` axis const's slot type).
fn i64_ty(types: &mut TypeTable) -> MirType {
    MirType::semantic(types.sized_numeric(Primitive::Numerus, NumericWidth::I64))
}
