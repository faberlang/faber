//! The dead fold-artifact slot scan (S5-W-A1): the local and temp
//! destinations of `Array`/`Tensor` constructs that no statement or
//! terminator reads — the static-shape fold's shape-list / vacua artifacts
//! the emitters skip (S5-U2c).

use crate::{MirFunction, MirLocalId, MirPlaceBase, MirTempId, MirType};
use handroll::FxHashSet;
use radix_types::{Type, TypeTable};

/// The dead fold-artifact slots of one function (S5-W-A1): the local and
/// temp destinations of `Array`/`Tensor` constructs that no statement or
/// terminator reads — the static-shape fold's shape-list / vacua artifacts
/// the emitters skip (S5-U2c).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct FoldArtifactSlots {
    pub(super) locals: FxHashSet<MirLocalId>,
    pub(super) temps: FxHashSet<MirTempId>,
}

impl FoldArtifactSlots {
    /// Whether a place base is one of the dead fold-artifact slots.
    pub(super) fn contains(&self, base: &MirPlaceBase) -> bool {
        match base {
            MirPlaceBase::Local(local) => self.locals.contains(local),
            MirPlaceBase::Temp(temp) => self.temps.contains(temp),
        }
    }
}

/// The dead fold-artifact slots of a function (S5-W-A1) — the S5-U2c
/// liveness scan the gate shares with the emitters.
///
/// A slot is collected when it is a fold-artifact slot shape
/// (`Array`/`Tensor` — the static-shape fold's shape-list / vacua types)
/// and DEAD: no statement or terminator reads it, and a local slot is not a
/// function param (a param is a side effect — always live). A temp slot
/// traces through the lowered body's forward copy (`Local(l) ← Temp(t)`):
/// the temp is dead exactly when the local it forwards into (or a direct
/// operand consumer) is never read — mirrors the Metal/CUDA
/// `destination_is_read` (S5-U2c). The forward-copy target local is
/// collected too (the fold keeps the untraceable `temp→local` forward, so
/// the shape-list local reaches the gate as a declaration even though no
/// statement reads it). The emitters skip exactly these slots; a live
/// `Array`/`Tensor` construct or slot stays fail-closed with the aggregate
/// diagnostics.
pub(super) fn dead_fold_artifact_slots(
    function: &MirFunction,
    types: &TypeTable,
) -> FoldArtifactSlots {
    let read_locals = crate::read_analysis::function_read_locals(function);
    let mut dead = FoldArtifactSlots::default();
    for local in &function.locals {
        if is_fold_artifact_type(types, local.ty)
            && !function.params.iter().any(|param| param.local == local.id)
            && !read_locals.contains(&local.id)
        {
            dead.locals.insert(local.id);
        }
    }
    for temp in &function.temps {
        if !is_fold_artifact_type(types, temp.ty)
            || crate::read_analysis::temp_directly_read(function, temp.id)
        {
            continue;
        }
        match crate::read_analysis::temp_forward_local(function, temp.id) {
            None => {
                dead.temps.insert(temp.id);
            }
            Some(local)
                if !function.params.iter().any(|param| param.local == local)
                    && !read_locals.contains(&local) =>
            {
                dead.temps.insert(temp.id);
            }
            Some(_) => {}
        }
    }
    dead
}

/// Whether a slot type is a fold-artifact slot shape (`Array`/`Tensor` —
/// the static-shape fold's shape-list and vacua-seed types; a static-rank
/// shape list is the bounded `lista<numerus, magnitudo R>`). Only these
/// types may be dead-skipped at the gate, mirroring the emitters' S5-U2c
/// skip surface; they are structurally non-scalar, satisfying the narrowed
/// "non-scalar only" dead-skip rule (radix-mir-metal 78e4440e7).
pub(super) fn is_fold_artifact_type(types: &TypeTable, ty: MirType) -> bool {
    matches!(
        types.get(ty.semantic_id()),
        Type::Array(_) | Type::BoundedArray { .. } | Type::RangedArray { .. } | Type::Tensor(_, _)
    )
}
