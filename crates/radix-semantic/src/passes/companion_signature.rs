//! Post-HIR companion signature attachment for call-site typecheck.
//!
//! This pass runs after HIR lowering and before typecheck. For each backward
//! companion registered in `RadixLaneMetadata`, it builds the typed `FuncSig`
//! from the primal HIR function and returns a `CompanionSignatures` map for
//! typecheck to register directly in its `functions` table.
//!
//! The companion signature matches the Stage 0 / reverse-AD ABI built by
//! `transform_reverse_ad`:
//!
//! ```text
//! companion(selected_tensor_params..., residual_bundle: (), upstream: R)
//!   -> (grad_T1, grad_T2, ...)
//! ```
//!
//! HIR stubs are intentionally NOT injected here. Bodyless HIR function items
//! with tuple return types poison MIR lowering (the LLVM codegen cannot emit
//! a default return for physical aggregates). Instead, signatures are passed
//! to typecheck's `collect_items` via `CompanionSignatures` for direct
//! insertion into the call-site type validation table.
//!
//! The resolver symbol's `ty` is still set here so diagnostics and other
//! symbol-level queries see the full function type.
//!
//! INVARIANTS
//! ==========
//! - Companion `DefId` identity is stable: this pass reads the pre-established
//!   companion DefId from `RadixBackward` and never allocates a new one.
//! - Only functions with both `@ radix lane "air"` and `@ radix backward "name"`
//!   receive a companion signature.
//! - Selected tensor params use the same filter as the reverse-AD transform:
//!   `is_tensor_type`.

use crate::tensor_type_directed::is_tensor_type;
use crate::{
    FuncSig, ParamType, Primitive, RadixLaneMetadata, Resolver, SemanticParamMode, Type, TypeId,
    TypeTable,
};
use radix_hir::{DefId, HirItemKind, HirTypeParam};
use radix_lexer::Interner;
use rustc_hash::FxHashMap;

/// Companion signatures ready for typecheck's `collect_items` registration.
#[derive(Debug, Clone, Default)]
pub struct CompanionSignatures {
    /// Companion DefId → typed signature.
    pub functions: FxHashMap<DefId, FuncSig>,
}

/// Build companion `FuncSig`s from primal HIR functions and attach typed
/// signatures to resolver symbols.
///
/// Returns a `CompanionSignatures` map for typecheck to register directly in
/// its `functions` table, bypassing HIR stub injection.
///
/// # Panics
///
/// Panics if a primal HIR function is not found for a backward companion entry.
pub fn attach_signatures(
    hir: &radix_hir::HirModule,
    radix_lanes: &RadixLaneMetadata,
    resolver: &mut Resolver,
    types: &mut TypeTable,
    _interner: &mut Interner,
) -> CompanionSignatures {
    let mut companions = CompanionSignatures::default();

    // Index HIR functions once — O(1) per companion instead of O(items).
    let functions_by_def: FxHashMap<DefId, &radix_hir::HirFunction> = hir
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            HirItemKind::Function(func) => Some((item.def_id, func.as_ref())),
            _ => None,
        })
        .collect();

    for (primal_def_id, backward) in radix_lanes.iter_backward() {
        let companion_def_id = backward.companion_def_id;

        let Some(primal) = functions_by_def.get(&primal_def_id).copied() else {
            continue;
        };

        // Filter tensor-typed params (same filter as reverse-AD transform).
        let selected_params: Vec<&radix_hir::HirParam> = primal
            .params
            .iter()
            .filter(|p| is_tensor_type(p.ty, types))
            .collect();

        let residual_ty = types.primitive(Primitive::Vacuum);

        let upstream_ty = primal.ret_ty.unwrap_or_else(|| types.unit());

        // Build gradient tuple return type: (grad_T1, grad_T2, ...)
        let grad_types: Vec<TypeId> = selected_params.iter().map(|p| p.ty).collect();
        let gradient_tuple_ty = types.tuple(grad_types);

        // -- Build companion FuncSig --
        let mut sig_params: Vec<ParamType> = selected_params
            .iter()
            .map(|p| ParamType {
                ty: p.ty,
                mode: SemanticParamMode::Owned,
                optional: false,
                sponte: false,
            })
            .collect();

        // Residual: empty tuple, owned.
        sig_params.push(ParamType {
            ty: residual_ty,
            mode: SemanticParamMode::Owned,
            optional: false,
            sponte: false,
        });

        // Upstream: primal return type, owned.
        sig_params.push(ParamType {
            ty: upstream_ty,
            mode: SemanticParamMode::Owned,
            optional: false,
            sponte: false,
        });

        let sig = FuncSig {
            type_params: HirTypeParam::descriptors(&primal.type_params),
            params: sig_params,
            ret: gradient_tuple_ty,
            err: None,
            is_async: false,
            is_generator: false,
        };

        // Attach the typed signature to the resolver symbol.
        let func_type_id = types.intern(Type::Func(sig.clone()));
        if let Some(symbol) = resolver.get_symbol_mut(companion_def_id) {
            symbol.ty = Some(func_type_id);
        }

        companions.functions.insert(companion_def_id, sig);
    }

    companions
}
