//! Library pruning must carry `device_function_metadata` through the
//! function-id reindex (register F-32).
//!
//! `prune_unreachable_library_functions` drops unreachable library functions
//! and renumbers the survivors. The library's device metadata rows are keyed
//! by function id, so a row left at an old id either describes a pruned
//! function or, after the merge shift, collides with another function's id.

use super::*;
use radix_mir::device::MirDeviceFunctionMetadata;
use radix_mir::{
    MirDeviceRole, MirFunctionOrigin, MirFunctionVisibility, MirKernelShaderStage,
    MirSyntheticFunctionIdentity,
};
use radix_module::lexer::Span;
use radix_types::{NumericWidth, TypeTable};

/// Ordinal carried by the row of the kept kernel.
const KERNEL_ORDINAL: u32 = 7;
/// Ordinal carried by the row of a pruned function.
const PRUNED_ORDINAL: u32 = 9;
/// Ordinal carried by a generic template's row (its key is no program function).
const TEMPLATE_ORDINAL: u32 = 5;
const TEMPLATE_KEY: u32 = 20;

fn function(id: u32, source: u32, unit: MirType) -> MirFunction {
    MirFunction {
        id: MirFunctionId(id),
        source: Some(DefId(source)),
        name: None,
        params: Vec::new(),
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: Vec::new(),
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(None),
                span: Span::default(),
            },
            span: Span::default(),
        }],
        return_ty: unit,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    }
}

fn row(parent: u32, ordinal: u32) -> MirDeviceFunctionMetadata {
    MirDeviceFunctionMetadata {
        identity: MirSyntheticFunctionIdentity {
            parent: MirFunctionId(parent),
            ordinal,
        },
        role: MirDeviceRole::Kernel,
        shader_stage: MirKernelShaderStage::Compute,
        visibility: MirFunctionVisibility::Private,
        origin: MirFunctionOrigin::SyntheticKernelClosure,
        lane_domain_local: None,
        refused_backends: Default::default(),
        tier: radix_mir::device::KernelTier::Default,
    }
}

fn library(
    functions: Vec<MirFunction>,
    metadata: Vec<(u32, MirDeviceFunctionMetadata)>,
) -> LibraryMirParts {
    let mut device_function_metadata = MirDeviceFunctionMetadataMap::default();
    for (key, row) in metadata {
        device_function_metadata.insert(MirFunctionId(key), row);
    }
    LibraryMirParts {
        program: MirProgram { functions },
        environments: Vec::new(),
        templates: Vec::new(),
        generic_params: Vec::new(),
        tensor_edge_policy_metadata: MirTensorEdgePolicyMetadata::default(),
        device_function_metadata,
        display_tokens: radix_mir::MirDisplayTokens::default(),
    }
}

fn unit_ty() -> MirType {
    let mut types = TypeTable::new();
    MirType::semantic(types.sized_numeric(Primitive::Fractus, NumericWidth::F32))
}

fn prune(parts: &mut LibraryMirParts, seed: &[u32]) {
    let mut reachable: HashSet<DefId> = seed.iter().copied().map(DefId).collect();
    let mut libraries = vec![std::mem::replace(parts, library(Vec::new(), Vec::new()))];
    prune_unreachable_library_functions(&mut libraries, &mut reachable);
    *parts = libraries.remove(0);
}

/// An unreachable function precedes the reachable kernel, so pruning shifts
/// the kernel's id (and a second unreachable function keeps a stale row).
#[test]
fn kernel_row_follows_its_function_and_pruned_rows_are_dropped() {
    let ty = unit_ty();
    // ids: 0 pruned, 1 pruned (has a row), 2 kept kernel (has a row),
    // 3 kept plain function the kernel row names as its parent.
    let mut parts = library(
        vec![
            function(0, 100, ty),
            function(1, 101, ty),
            function(2, 102, ty),
            function(3, 103, ty),
        ],
        vec![
            (1, row(0, PRUNED_ORDINAL)),
            (2, row(3, KERNEL_ORDINAL)),
            (TEMPLATE_KEY, row(TEMPLATE_KEY, TEMPLATE_ORDINAL)),
        ],
    );
    prune(&mut parts, &[102, 103]);

    let ids: Vec<(u32, Option<DefId>)> = parts
        .program
        .functions
        .iter()
        .map(|function| (function.id.0, function.source))
        .collect();
    assert_eq!(
        ids,
        vec![(0, Some(DefId(102))), (1, Some(DefId(103)))],
        "pruning keeps the two reachable functions, renumbered contiguously"
    );

    // (a) the kernel's row follows it to its new id, parent included.
    let kernel_row = parts
        .device_function_metadata
        .get(&MirFunctionId(0))
        .expect("kernel row keyed at the kernel's new id");
    assert_eq!(kernel_row.identity.ordinal, KERNEL_ORDINAL);
    assert_eq!(kernel_row.identity.parent, MirFunctionId(1));

    // (b) no row remains for the pruned function.
    assert!(
        parts
            .device_function_metadata
            .values()
            .all(|row| row.identity.ordinal != PRUNED_ORDINAL),
        "a pruned function's row must not survive"
    );

    // A generic template has no program function; its row keeps its key.
    assert_eq!(
        parts
            .device_function_metadata
            .get(&MirFunctionId(TEMPLATE_KEY))
            .map(|row| row.identity.ordinal),
        Some(TEMPLATE_ORDINAL)
    );

    // (c) no row is keyed at an id that now names a different function:
    // id 1 is the plain function (it never had a row), id 2 names nothing.
    assert_eq!(parts.device_function_metadata.len(), 2);
    assert!(
        !parts
            .device_function_metadata
            .contains_key(&MirFunctionId(1)),
        "old id 1 now names a function without device metadata"
    );
    for key in parts
        .device_function_metadata
        .keys()
        .filter(|key| key.0 != TEMPLATE_KEY)
    {
        assert!(
            parts.program.functions.iter().any(|f| f.id == *key),
            "row key {key:?} names no function"
        );
    }
}

/// Kept ids that are already contiguous (only a trailing function pruned)
/// still lose the pruned function's row.
#[test]
fn trailing_pruned_function_row_is_dropped_when_ids_stay_contiguous() {
    let ty = unit_ty();
    let mut parts = library(
        vec![
            function(0, 100, ty),
            function(1, 101, ty),
            function(2, 102, ty),
        ],
        vec![(0, row(0, KERNEL_ORDINAL)), (2, row(0, PRUNED_ORDINAL))],
    );
    prune(&mut parts, &[100, 101]);

    assert_eq!(parts.program.functions.len(), 2);
    assert_eq!(parts.device_function_metadata.len(), 1);
    assert_eq!(
        parts
            .device_function_metadata
            .get(&MirFunctionId(0))
            .map(|row| row.identity.ordinal),
        Some(KERNEL_ORDINAL)
    );
}
