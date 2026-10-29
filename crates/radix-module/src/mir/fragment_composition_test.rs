//! Coverage for nucleum-fragment MIR composition.

use super::{
    ExpansionBudget, FragmentCompositionError, ISSUE_CYCLE, ISSUE_EXPANSION, compose_with_metadata,
    reject_surviving_fragment_calls_with_metadata, surviving_fragment_calls_with_metadata, with_budget_override,
};
use crate::driver::{self, AnalyzeRequest, Config, Session};
use crate::hir::DefId;
use crate::lexer::{Interner, Span};
use crate::mir::MirLoweringOptions;
use crate::mir::{
    MirBinOp, MirBlock, MirBlockId, MirCallee, MirDeviceFunctionMetadata, MirDeviceFunctionMetadataMap, MirDeviceRole,
    MirFunction, MirFunctionId, MirFunctionOrigin, MirFunctionVisibility, MirKernelShaderStage, MirLocal, MirLocalId,
    MirMonomorphization, MirOperand, MirParam, MirParamMode, MirPlace, MirStatement, MirStatementKind,
    MirSyntheticFunctionIdentity, MirTerminator, MirTerminatorKind, MirType, MirValue, MirValueId, MirValueKind,
};
use crate::semantic::TypeTable;
use crate::test_support::{test_span, test_span_between};
use handroll::{FxHashMap, FxHashSet};

/// Plain-wrapper stand-in for the deleted production `compose` (merge-16):
/// the tests exercise composition without an explicit metadata carrier.
fn compose(
    functions: &mut Vec<MirFunction>,
    monomorphizations: &[MirMonomorphization],
    roles: &FxHashMap<DefId, MirDeviceRole>,
    budget: ExpansionBudget,
    interner: Option<&Interner>,
    launch_entries: &FxHashSet<DefId>,
) -> Vec<FragmentCompositionError> {
    compose_with_metadata(
        functions,
        monomorphizations,
        roles,
        &MirDeviceFunctionMetadataMap::default(),
        budget,
        interner,
        launch_entries,
    )
}

const ENTRY: DefId = DefId(1);
const FRAGMENT: DefId = DefId(2);
const FRAGMENT_INNER: DefId = DefId(6);
const FRAGMENT_B: DefId = DefId(7);
const KERNEL_HELPER: DefId = DefId(8);
const KERNEL_INNER: DefId = DefId(9);
const HOST: DefId = DefId(10);

const CALL_SPAN: fn() -> Span = || test_span_between(40, 52);
const DEF_SPAN: fn() -> Span = || test_span_between(8, 20);

fn f32_ty(types: &mut TypeTable) -> MirType {
    crate::mir::test_support::f32_type(types)
}

fn param(local: u32, ty: MirType) -> MirParam {
    MirParam { local: MirLocalId(local), name: None, ty, mode: MirParamMode::Owned, span: test_span() }
}

fn empty_block(id: u32, ret: Option<MirOperand>) -> MirBlock {
    MirBlock {
        id: MirBlockId(id),
        statements: Vec::new(),
        terminator: MirTerminator { kind: MirTerminatorKind::Return(ret), span: test_span() },
        span: test_span(),
    }
}

fn call_stmt(dest: Option<MirPlace>, callee: MirFunctionId, args: Vec<MirOperand>) -> MirStatement {
    MirStatement {
        kind: MirStatementKind::Call { destination: dest, callee: MirCallee::Function(callee), args },
        span: CALL_SPAN(),
    }
}

fn function(
    id: u32,
    source: DefId,
    params: Vec<MirParam>,
    return_ty: MirType,
    locals: Vec<MirLocal>,
    blocks: Vec<MirBlock>,
    shader_stage: Option<MirKernelShaderStage>,
) -> MirFunction {
    MirFunction {
        id: MirFunctionId(id),
        source: Some(source),
        name: None,
        params,
        locals,
        temps: Vec::new(),
        blocks,
        return_ty,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage,
        span: if source == FRAGMENT || source == FRAGMENT_INNER || source == FRAGMENT_B {
            DEF_SPAN()
        } else {
            test_span()
        },
    }
}

fn identity_fragment(id: u32, source: DefId, ty: MirType) -> MirFunction {
    let mut block = empty_block(0, Some(MirOperand::Place(MirPlace::local(MirLocalId(0)))));
    block.span = DEF_SPAN();
    block.terminator.span = DEF_SPAN();
    function(id, source, vec![param(0, ty)], ty, Vec::new(), vec![block], None)
}

fn entry_calling(
    dest_local: u32,
    callee: u32,
    ty: MirType,
    extra_locals: Vec<MirLocal>,
    extra_stmts_after: Vec<MirStatement>,
) -> MirFunction {
    let mut statements = vec![call_stmt(
        Some(MirPlace::local(MirLocalId(dest_local))),
        MirFunctionId(callee),
        vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
    )];
    statements.extend(extra_stmts_after);
    let mut locals = vec![MirLocal { id: MirLocalId(dest_local), name: None, ty, mutable: false, span: test_span() }];
    locals.extend(extra_locals);
    function(
        0,
        ENTRY,
        vec![param(0, ty)],
        ty,
        locals,
        vec![MirBlock {
            id: MirBlockId(0),
            statements,
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(dest_local))))),
                span: test_span(),
            },
            span: test_span(),
        }],
        Some(MirKernelShaderStage::Compute),
    )
}

fn roles() -> FxHashMap<DefId, MirDeviceRole> {
    let mut roles = FxHashMap::default();
    roles.insert(ENTRY, MirDeviceRole::Kernel);
    roles.insert(FRAGMENT, MirDeviceRole::Fragment);
    roles.insert(FRAGMENT_INNER, MirDeviceRole::Fragment);
    roles.insert(FRAGMENT_B, MirDeviceRole::Fragment);
    roles.insert(KERNEL_HELPER, MirDeviceRole::Kernel);
    roles.insert(KERNEL_INNER, MirDeviceRole::Kernel);
    roles
}

fn launch_entries() -> FxHashSet<DefId> {
    std::iter::once(ENTRY).collect()
}

fn compose_fns(functions: &mut Vec<MirFunction>) -> Vec<super::FragmentCompositionError> {
    compose(
        functions,
        &[],
        &roles(),
        ExpansionBudget::DOCUMENTED_DEFAULTS,
        None,
        &launch_entries(),
    )
}

fn fragment_call_count(functions: &[MirFunction]) -> usize {
    surviving_fragment_calls_with_metadata(functions, &[], &roles(), &MirDeviceFunctionMetadataMap::default()).len()
}

#[test]
fn mutation_surviving_call_fires_the_gate() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let helper = identity_fragment(1, FRAGMENT, ty);
    let entry = entry_calling(1, 1, ty, Vec::new(), Vec::new());
    let functions = vec![entry, helper];
    assert_eq!(
        fragment_call_count(&functions),
        1,
        "uncomposed MIR must retain the fragment call"
    );
    let errors = reject_surviving_fragment_calls_with_metadata(
        &functions,
        &[],
        &roles(),
        &MirDeviceFunctionMetadataMap::default(),
    );
    assert!(
        errors
            .iter()
            .any(|err| err.issue == ISSUE_EXPANSION && err.reason == "survived"),
        "a surviving fragment call must fire the composition gate, got {errors:?}"
    );
}

#[test]
fn composition_eliminates_every_fragment_call() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let helper = identity_fragment(1, FRAGMENT, ty);
    let entry = entry_calling(1, 1, ty, Vec::new(), Vec::new());
    let mut functions = vec![entry, helper];
    let errors = compose_fns(&mut functions);
    assert!(errors.is_empty(), "legal composition must succeed: {errors:?}");
    assert_eq!(fragment_call_count(&functions), 0, "no fragment call may survive");
}

#[test]
fn eval_once_binds_argument_to_a_single_local() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let helper = function(
        1,
        FRAGMENT,
        vec![param(0, ty)],
        ty,
        vec![MirLocal { id: MirLocalId(1), name: None, ty, mutable: false, span: DEF_SPAN() }],
        vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![MirStatement {
                kind: MirStatementKind::Assign {
                    place: MirPlace::local(MirLocalId(1)),
                    value: MirValue {
                        id: MirValueId(0),
                        kind: MirValueKind::Binary {
                            op: MirBinOp::Add,
                            lhs: MirOperand::Place(MirPlace::local(MirLocalId(0))),
                            rhs: MirOperand::Place(MirPlace::local(MirLocalId(0))),
                        },
                        ty,
                        span: DEF_SPAN(),
                    },
                },
                span: DEF_SPAN(),
            }],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(1))))),
                span: DEF_SPAN(),
            },
            span: DEF_SPAN(),
        }],
        None,
    );
    let entry = entry_calling(1, 1, ty, Vec::new(), Vec::new());
    let mut functions = vec![entry, helper];
    assert!(compose_fns(&mut functions).is_empty());
    let composed = &functions[0];
    let bind_locals: Vec<_> = composed
        .blocks
        .iter()
        .flat_map(|block| block.statements.iter())
        .filter_map(|stmt| match &stmt.kind {
            MirStatementKind::Assign {
                place,
                value: MirValue { kind: MirValueKind::Operand(MirOperand::Place(src)), .. },
            } if matches!(src.base, crate::mir::MirPlaceBase::Local(MirLocalId(0)))
                && matches!(place.base, crate::mir::MirPlaceBase::Local(_)) =>
            {
                match place.base {
                    crate::mir::MirPlaceBase::Local(id) => Some(id),
                    _ => None,
                }
            }
            _ => None,
        })
        .collect();
    assert_eq!(bind_locals.len(), 1, "argument must be bound exactly once: {composed:#?}");
    let bound = bind_locals[0];
    let add_uses = composed
        .blocks
        .iter()
        .flat_map(|block| block.statements.iter())
        .filter_map(|stmt| match &stmt.kind {
            MirStatementKind::Assign {
                value: MirValue { kind: MirValueKind::Binary { lhs, rhs, .. }, .. }, ..
            } => Some((lhs.clone(), rhs.clone())),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(add_uses.len(), 1);
    let expect = MirOperand::Place(MirPlace::local(bound));
    assert_eq!(add_uses[0].0, expect);
    assert_eq!(add_uses[0].1, expect);
}

#[test]
fn two_call_sites_alpha_rename() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let helper = identity_fragment(1, FRAGMENT, ty);
    let mut entry = entry_calling(1, 1, ty, Vec::new(), Vec::new());
    entry
        .locals
        .push(MirLocal { id: MirLocalId(2), name: None, ty, mutable: false, span: test_span() });
    entry.blocks[0].statements.push(call_stmt(
        Some(MirPlace::local(MirLocalId(2))),
        MirFunctionId(1),
        vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
    ));
    let mut functions = vec![entry, helper];
    assert!(compose_fns(&mut functions).is_empty());
    assert_eq!(fragment_call_count(&functions), 0);
    let composed = &functions[0];
    let mut local_ids = composed
        .params
        .iter()
        .map(|param| param.local)
        .collect::<Vec<_>>();
    local_ids.extend(composed.locals.iter().map(|local| local.id));
    let unique = local_ids
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(unique.len(), local_ids.len(), "cloned locals must not alias: {local_ids:?}");
    let block_ids = composed
        .blocks
        .iter()
        .map(|block| block.id)
        .collect::<Vec<_>>();
    let unique_blocks = block_ids
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        unique_blocks.len(),
        block_ids.len(),
        "cloned blocks must not alias: {block_ids:?}"
    );
}

#[test]
fn cloned_return_keeps_callee_span() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let helper = identity_fragment(1, FRAGMENT, ty);
    let entry = entry_calling(1, 1, ty, Vec::new(), Vec::new());
    let mut functions = vec![entry, helper];
    assert!(compose_fns(&mut functions).is_empty());
    let has_callee_span = functions[0].blocks.iter().any(|block| {
        block
            .statements
            .iter()
            .any(|stmt| stmt.span == DEF_SPAN() || block.terminator.span == DEF_SPAN())
    });
    assert!(
        has_callee_span,
        "cloned return path must keep callee provenance: {:#?}",
        functions[0]
    );
}

#[test]
fn transitive_fragment_calls_compose() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let inner = identity_fragment(2, FRAGMENT_INNER, ty);
    let outer = function(
        1,
        FRAGMENT,
        vec![param(0, ty)],
        ty,
        vec![MirLocal { id: MirLocalId(1), name: None, ty, mutable: false, span: test_span() }],
        vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![call_stmt(
                Some(MirPlace::local(MirLocalId(1))),
                MirFunctionId(2),
                vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
            )],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(1))))),
                span: test_span(),
            },
            span: test_span(),
        }],
        None,
    );
    let entry = entry_calling(1, 1, ty, Vec::new(), Vec::new());
    let mut functions = vec![entry, outer, inner];
    let errors = compose_fns(&mut functions);
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(fragment_call_count(&functions), 0);
}

#[test]
fn direct_cycle_is_rejected_with_full_cycle() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let helper = function(
        1,
        FRAGMENT,
        vec![param(0, ty)],
        ty,
        vec![MirLocal { id: MirLocalId(1), name: None, ty, mutable: false, span: test_span() }],
        vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![call_stmt(
                Some(MirPlace::local(MirLocalId(1))),
                MirFunctionId(1),
                vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
            )],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(1))))),
                span: test_span(),
            },
            span: test_span(),
        }],
        None,
    );
    let entry = entry_calling(1, 1, ty, Vec::new(), Vec::new());
    let mut functions = vec![entry, helper];
    let errors = compose_fns(&mut functions);
    let err = errors
        .iter()
        .find(|err| err.issue == ISSUE_CYCLE)
        .unwrap_or_else(|| panic!("expected cycle, got {errors:?}"));
    let cycle = err
        .args
        .iter()
        .find(|arg| arg.name == "cycle")
        .map_or("", |arg| arg.value.as_str());
    assert!(cycle.contains('#'), "full cycle must be in the diagnostic: {cycle}");
    assert!(cycle.contains('→') || cycle.contains("->") || cycle.matches('#').count() >= 1);
}

#[test]
fn transitive_cycle_is_rejected() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let a = function(
        1,
        FRAGMENT,
        vec![param(0, ty)],
        ty,
        vec![MirLocal { id: MirLocalId(1), name: None, ty, mutable: false, span: test_span() }],
        vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![call_stmt(
                Some(MirPlace::local(MirLocalId(1))),
                MirFunctionId(2),
                vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
            )],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(1))))),
                span: test_span(),
            },
            span: test_span(),
        }],
        None,
    );
    let b = function(
        2,
        FRAGMENT_B,
        vec![param(0, ty)],
        ty,
        vec![MirLocal { id: MirLocalId(1), name: None, ty, mutable: false, span: test_span() }],
        vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![call_stmt(
                Some(MirPlace::local(MirLocalId(1))),
                MirFunctionId(1),
                vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
            )],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(1))))),
                span: test_span(),
            },
            span: test_span(),
        }],
        None,
    );
    let entry = entry_calling(1, 1, ty, Vec::new(), Vec::new());
    let mut functions = vec![entry, a, b];
    let errors = compose_fns(&mut functions);
    let err = errors
        .iter()
        .find(|err| err.issue == ISSUE_CYCLE)
        .unwrap_or_else(|| panic!("expected cycle, got {errors:?}"));
    let cycle = err
        .args
        .iter()
        .find(|arg| arg.name == "cycle")
        .map_or("", |arg| arg.value.as_str());
    assert!(cycle.contains('#') && cycle.contains('→'), "full cycle required: {cycle}");
}

#[test]
fn depth_budget_fails_closed() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let inner = identity_fragment(2, FRAGMENT_INNER, ty);
    let outer = function(
        1,
        FRAGMENT,
        vec![param(0, ty)],
        ty,
        vec![MirLocal { id: MirLocalId(1), name: None, ty, mutable: false, span: test_span() }],
        vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![call_stmt(
                Some(MirPlace::local(MirLocalId(1))),
                MirFunctionId(2),
                vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
            )],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(1))))),
                span: test_span(),
            },
            span: test_span(),
        }],
        None,
    );
    let entry = entry_calling(1, 1, ty, Vec::new(), Vec::new());
    let mut functions = vec![entry, outer, inner];
    let budget = ExpansionBudget { max_depth: 1, ..ExpansionBudget::DOCUMENTED_DEFAULTS };
    let errors = compose(&mut functions, &[], &roles(), budget, None, &launch_entries());
    assert!(
        errors
            .iter()
            .any(|err| err.issue == ISSUE_EXPANSION && err.reason == "depth"),
        "depth cap must fail closed, got {errors:?}"
    );
    let err = errors.iter().find(|err| err.reason == "depth").unwrap();
    assert!(
        err.args
            .iter()
            .any(|arg| arg.name == "limit" && arg.value == "1")
    );
    assert!(err.args.iter().any(|arg| arg.name == "observed"));
    assert!(err.args.iter().any(|arg| arg.name == "chain"));
}

#[test]
fn node_budget_fails_closed() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let helper = identity_fragment(1, FRAGMENT, ty);
    let entry = entry_calling(1, 1, ty, Vec::new(), Vec::new());
    let mut functions = vec![entry, helper];
    let budget = ExpansionBudget { max_cloned_nodes_per_entry: 1, ..ExpansionBudget::DOCUMENTED_DEFAULTS };
    let errors = compose(&mut functions, &[], &roles(), budget, None, &launch_entries());
    assert!(
        errors
            .iter()
            .any(|err| err.issue == ISSUE_EXPANSION && err.reason == "nodes"),
        "node cap must fail closed, got {errors:?}"
    );
}

#[test]
fn callsite_node_budget_fails_closed() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let inner = identity_fragment(2, FRAGMENT_INNER, ty);
    let outer = function(
        1,
        FRAGMENT,
        vec![param(0, ty)],
        ty,
        vec![MirLocal { id: MirLocalId(1), name: None, ty, mutable: false, span: test_span() }],
        vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![call_stmt(
                Some(MirPlace::local(MirLocalId(1))),
                MirFunctionId(2),
                vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
            )],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(1))))),
                span: test_span(),
            },
            span: test_span(),
        }],
        None,
    );
    let entry = entry_calling(1, 1, ty, Vec::new(), Vec::new());
    let mut functions = vec![entry, outer, inner];
    let budget = ExpansionBudget { max_cloned_nodes_per_root_callsite: 1, ..ExpansionBudget::DOCUMENTED_DEFAULTS };
    let errors = compose(&mut functions, &[], &roles(), budget, None, &launch_entries());
    let err = errors
        .iter()
        .find(|err| err.issue == ISSUE_EXPANSION && err.reason == "callsite_nodes")
        .unwrap_or_else(|| panic!("callsite node cap must fail closed, got {errors:?}"));
    assert!(
        err.args
            .iter()
            .any(|arg| arg.name == "limit" && arg.value == "1")
    );
    assert!(err.args.iter().any(|arg| arg.name == "observed"));
    assert!(err.args.iter().any(|arg| arg.name == "chain"));
}

#[test]
fn callsite_node_budget_is_per_root_site() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let (locals, block) = sequential_calls_block(2, 1);
    let entry = function(
        0,
        ENTRY,
        vec![param(0, ty)],
        ty,
        locals,
        vec![block],
        Some(MirKernelShaderStage::Compute),
    );
    let mut functions = vec![entry, identity_fragment(1, FRAGMENT, ty)];
    // An identity expansion clones three nodes; each sibling root site gets
    // its own allowance, so two sites compose under a cap of three.
    let budget = ExpansionBudget { max_cloned_nodes_per_root_callsite: 3, ..ExpansionBudget::DOCUMENTED_DEFAULTS };
    let errors = compose(&mut functions, &[], &roles(), budget, None, &launch_entries());
    assert!(errors.is_empty(), "each root call site gets its own budget: {errors:?}");
    assert_eq!(fragment_call_count(&functions), 0);
}

#[test]
fn instance_budget_fails_closed() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let helper = identity_fragment(1, FRAGMENT, ty);
    let entry = entry_calling(1, 1, ty, Vec::new(), Vec::new());
    let mut functions = vec![entry, helper];
    let budget = ExpansionBudget { max_concrete_instances: 0, ..ExpansionBudget::DOCUMENTED_DEFAULTS };
    let errors = compose(&mut functions, &[], &roles(), budget, None, &launch_entries());
    assert!(
        errors
            .iter()
            .any(|err| err.issue == ISSUE_EXPANSION && err.reason == "instances"),
        "instance cap must fail closed, got {errors:?}"
    );
}

#[test]
fn legal_entry_to_fragment_composes_through_lowering() {
    let source = r"
@ nucleum fragment
functio helper(f32 x) → f32 {
    redde x
}

@ nucleum
functio k(f32 x) → f32 {
    redde helper(x)
}
";
    let session = Session::new(Config::default().with_latin_locale());
    let analysis =
        driver::analyze_source(&session, "device.fab", source, AnalyzeRequest::default()).expect("analyze source");
    let roles = crate::mir::device_roles_from_hir(&analysis.hir);
    let lowered = crate::mir::lower_analyzed_unit(analysis.application_view(), MirLoweringOptions::default())
        .expect("composition must succeed");
    assert!(
        surviving_fragment_calls_with_metadata(
            &lowered.validated.program().functions,
            &[],
            &roles,
            &MirDeviceFunctionMetadataMap::default(),
        )
        .is_empty(),
        "no fragment call may survive lowering: {:#?}",
        lowered.validated.program().functions
    );
}

#[test]
fn source_cycle_is_rejected_at_lower() {
    let source = r"
@ nucleum fragment
functio ping(f32 x) → f32 {
    redde pong(x)
}

@ nucleum fragment
functio pong(f32 x) → f32 {
    redde ping(x)
}

@ nucleum
functio k(f32 x) → f32 {
    redde ping(x)
}
";
    let session = Session::new(Config::default().with_latin_locale());
    let analysis =
        driver::analyze_source(&session, "device.fab", source, AnalyzeRequest::default()).expect("analyze source");
    let errors = crate::mir::lower_analyzed_unit(analysis.application_view(), MirLoweringOptions::default())
        .expect_err("cycle must fail closed");
    assert!(
        errors.iter().any(|err| err.issue == ISSUE_CYCLE),
        "expected nucleum_fragment_cycle, got {errors:?}"
    );
}

#[test]
fn budget_override_rejects_source_expansion() {
    let source = r"
@ nucleum fragment
functio helper(f32 x) → f32 {
    redde x
}

@ nucleum
functio k(f32 x) → f32 {
    redde helper(x)
}
";
    let budget = ExpansionBudget { max_concrete_instances: 0, ..ExpansionBudget::DOCUMENTED_DEFAULTS };
    with_budget_override(budget, || {
        let session = Session::new(Config::default().with_latin_locale());
        let analysis =
            driver::analyze_source(&session, "device.fab", source, AnalyzeRequest::default()).expect("analyze source");
        let errors = crate::mir::lower_analyzed_unit(analysis.application_view(), MirLoweringOptions::default())
            .expect_err("budget must fail closed");
        assert!(
            errors.iter().any(|err| err.issue == ISSUE_EXPANSION),
            "expected nucleum_fragment_expansion, got {errors:?}"
        );
    });
}

// --- DFV2-4: concrete device-call composition (kernel callers/helpers) ---

fn kernel_helper(id: u32, source: DefId, ty: MirType) -> MirFunction {
    identity_fragment(id, source, ty)
}

fn kernel_calling_helper(helper_id: u32, ty: MirType) -> MirFunction {
    entry_calling(1, helper_id, ty, Vec::new(), Vec::new())
}

/// (a) one call: a device entry calling a concrete local private `@ nucleum`
/// helper composes; no device call survives and no helper symbol remains.
#[test]
fn kernel_helper_call_composes() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let helper = kernel_helper(1, KERNEL_HELPER, ty);
    let entry = kernel_calling_helper(1, ty);
    let mut functions = vec![entry, helper];
    assert!(compose_fns(&mut functions).is_empty(), "kernel helper must compose");
    assert_eq!(fragment_call_count(&functions), 0, "no device call may survive");
    assert!(
        functions
            .iter()
            .all(|function| function.source != Some(KERNEL_HELPER)),
        "the composed private helper must be pruned: {functions:#?}"
    );
}

/// (a)/(d) transitive: entry → kernel helper → kernel helper reaches fixpoint.
#[test]
fn transitive_kernel_calls_compose() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let inner = kernel_helper(2, KERNEL_INNER, ty);
    let outer = function(
        1,
        KERNEL_HELPER,
        vec![param(0, ty)],
        ty,
        vec![MirLocal { id: MirLocalId(1), name: None, ty, mutable: false, span: test_span() }],
        vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![call_stmt(
                Some(MirPlace::local(MirLocalId(1))),
                MirFunctionId(2),
                vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
            )],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(1))))),
                span: test_span(),
            },
            span: test_span(),
        }],
        None,
    );
    let entry = kernel_calling_helper(1, ty);
    let mut functions = vec![entry, outer, inner];
    let errors = compose_fns(&mut functions);
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(fragment_call_count(&functions), 0);
    assert!(
        functions
            .iter()
            .all(|function| function.source != Some(KERNEL_HELPER)),
        "composed transitive helpers must be pruned: {functions:#?}"
    );
}

/// (e) a direct kernel cycle reports the full chain.
#[test]
fn kernel_cycle_is_rejected_with_full_cycle() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let helper = function(
        1,
        KERNEL_HELPER,
        vec![param(0, ty)],
        ty,
        vec![MirLocal { id: MirLocalId(1), name: None, ty, mutable: false, span: test_span() }],
        vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![call_stmt(
                Some(MirPlace::local(MirLocalId(1))),
                MirFunctionId(1),
                vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
            )],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(1))))),
                span: test_span(),
            },
            span: test_span(),
        }],
        None,
    );
    let entry = kernel_calling_helper(1, ty);
    let mut functions = vec![entry, helper];
    let errors = compose_fns(&mut functions);
    let err = errors
        .iter()
        .find(|err| err.issue == ISSUE_CYCLE)
        .unwrap_or_else(|| panic!("expected kernel cycle, got {errors:?}"));
    let cycle = err
        .args
        .iter()
        .find(|arg| arg.name == "cycle")
        .map_or("", |arg| arg.value.as_str());
    assert!(cycle.contains('#') && cycle.contains('→'), "full cycle required: {cycle}");
}

/// (f) budget overflow on a kernel chain reports observed value and limit.
#[test]
fn kernel_budget_fails_closed() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let helper = kernel_helper(1, KERNEL_HELPER, ty);
    let entry = kernel_calling_helper(1, ty);
    let mut functions = vec![entry, helper];
    let budget = ExpansionBudget { max_depth: 0, ..ExpansionBudget::DOCUMENTED_DEFAULTS };
    let errors = compose(&mut functions, &[], &roles(), budget, None, &launch_entries());
    let err = errors
        .iter()
        .find(|err| err.issue == ISSUE_EXPANSION && err.reason == "depth")
        .unwrap_or_else(|| panic!("expected kernel depth overflow, got {errors:?}"));
    assert!(err.args.iter().any(|arg| arg.name == "observed"));
    assert!(
        err.args
            .iter()
            .any(|arg| arg.name == "limit" && arg.value == "0")
    );
    assert!(err.args.iter().any(|arg| arg.name == "chain"));
}

/// The host launch boundary: a host function's call to a public entry is not
/// a composition input and not a survivor — the entry symbol stays.
#[test]
fn host_call_to_kernel_entry_is_not_composed() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let entry = function(
        1,
        ENTRY,
        vec![param(0, ty)],
        ty,
        Vec::new(),
        vec![MirBlock {
            id: MirBlockId(0),
            statements: Vec::new(),
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(0))))),
                span: test_span(),
            },
            span: test_span(),
        }],
        Some(MirKernelShaderStage::Compute),
    );
    let host = function(
        0,
        HOST,
        vec![param(0, ty)],
        ty,
        vec![MirLocal { id: MirLocalId(1), name: None, ty, mutable: false, span: test_span() }],
        vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![call_stmt(
                Some(MirPlace::local(MirLocalId(1))),
                MirFunctionId(1),
                vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
            )],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(1))))),
                span: test_span(),
            },
            span: test_span(),
        }],
        None,
    );
    let mut functions = vec![host, entry];
    assert!(compose_fns(&mut functions).is_empty(), "host boundary calls must not error");
    assert!(
        functions
            .iter()
            .any(|function| function.id == MirFunctionId(1)),
        "the called entry must stay a program symbol: {functions:#?}"
    );
}

/// (b) eval-once for a kernel helper: the argument is bound exactly once.
#[test]
fn kernel_helper_binds_argument_once() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let helper = function(
        1,
        KERNEL_HELPER,
        vec![param(0, ty)],
        ty,
        vec![MirLocal { id: MirLocalId(1), name: None, ty, mutable: false, span: DEF_SPAN() }],
        vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![MirStatement {
                kind: MirStatementKind::Assign {
                    place: MirPlace::local(MirLocalId(1)),
                    value: MirValue {
                        id: MirValueId(0),
                        kind: MirValueKind::Binary {
                            op: MirBinOp::Add,
                            lhs: MirOperand::Place(MirPlace::local(MirLocalId(0))),
                            rhs: MirOperand::Place(MirPlace::local(MirLocalId(0))),
                        },
                        ty,
                        span: DEF_SPAN(),
                    },
                },
                span: DEF_SPAN(),
            }],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(1))))),
                span: DEF_SPAN(),
            },
            span: DEF_SPAN(),
        }],
        None,
    );
    let entry = kernel_calling_helper(1, ty);
    let mut functions = vec![entry, helper];
    assert!(compose_fns(&mut functions).is_empty());
    let composed = &functions[0];
    let bind_locals: Vec<_> = composed
        .blocks
        .iter()
        .flat_map(|block| block.statements.iter())
        .filter_map(|stmt| match &stmt.kind {
            MirStatementKind::Assign {
                place,
                value: MirValue { kind: MirValueKind::Operand(MirOperand::Place(src)), .. },
            } if matches!(src.base, crate::mir::MirPlaceBase::Local(MirLocalId(0)))
                && matches!(place.base, crate::mir::MirPlaceBase::Local(_)) =>
            {
                match place.base {
                    crate::mir::MirPlaceBase::Local(id) => Some(id),
                    _ => None,
                }
            }
            _ => None,
        })
        .collect();
    assert_eq!(bind_locals.len(), 1, "argument must be bound exactly once: {composed:#?}");
}

/// Source-level: a private `@ nucleum` helper called from a public entry
/// composes through lowering and leaves no helper symbol in the program.
#[test]
fn kernel_helper_source_composes_through_lowering() {
    let source = r"
@ nucleum
functio helper(f32 x) → f32 {
    redde x * 2.0
}

@ nucleum
functio k(f32 x) → f32 {
    redde helper(x)
}
";
    let session = Session::new(Config::default().with_latin_locale());
    let analysis =
        driver::analyze_source(&session, "device.fab", source, AnalyzeRequest::default()).expect("analyze source");
    let roles = crate::mir::device_roles_from_hir(&analysis.hir);
    let lowered = crate::mir::lower_analyzed_unit(analysis.application_view(), MirLoweringOptions::default())
        .expect("kernel helper must compose");
    assert!(
        surviving_fragment_calls_with_metadata(
            &lowered.validated.program().functions,
            &[],
            &roles,
            &MirDeviceFunctionMetadataMap::default(),
        )
        .is_empty(),
        "no device call may survive lowering: {:#?}",
        lowered.validated.program().functions
    );
    let kernel_symbols = lowered
        .validated
        .program()
        .functions
        .iter()
        .filter(|function| {
            function
                .source
                .is_some_and(|source| roles.get(&source) == Some(&MirDeviceRole::Kernel))
        })
        .count();
    assert_eq!(
        kernel_symbols,
        1,
        "only the uncalled entry kernel may remain; the composed helper is pruned: {:#?}",
        lowered.validated.program().functions
    );
}

// --- CDC-3: single-block splice shape at the composition home ---

/// done_when (a) shape: a single-block callee splices in place — the caller
/// keeps exactly its one block, keeps its original Return terminator, and
/// the splice leaves no Goto and no continue block.
#[test]
fn single_block_callee_splices_in_place() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let helper = identity_fragment(1, FRAGMENT, ty);
    let entry = entry_calling(1, 1, ty, Vec::new(), Vec::new());
    let mut functions = vec![entry, helper];
    assert!(compose_fns(&mut functions).is_empty());
    let composed = &functions[0];
    assert_eq!(
        composed.blocks.len(),
        1,
        "a single-block callee must splice without adding blocks: {composed:#?}"
    );
    assert!(
        matches!(composed.blocks[0].terminator.kind, MirTerminatorKind::Return(_)),
        "the caller's original terminator must survive the splice: {composed:#?}"
    );
    assert!(
        composed
            .blocks
            .iter()
            .all(|block| !matches!(block.terminator.kind, MirTerminatorKind::Goto(_))),
        "no synthetic Goto may remain: {composed:#?}"
    );
}

/// done_when (b): the in-place splice leaves no orphan or dead block ids —
/// the composed entry owns exactly one block id and the pruned helper's ids
/// left the program with it.
#[test]
fn in_place_splice_leaves_no_dead_block_ids() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let helper = identity_fragment(1, KERNEL_HELPER, ty);
    let entry = kernel_calling_helper(1, ty);
    let mut functions = vec![entry, helper];
    assert!(compose_fns(&mut functions).is_empty());
    let composed = &functions[0];
    assert_eq!(composed.blocks.len(), 1, "no continue or cloned block ids: {composed:#?}");
    let mut ids: Vec<_> = composed.blocks.iter().map(|block| block.id).collect();
    ids.sort_by_key(|id| id.0);
    let unique = ids
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(unique.len(), ids.len(), "block ids must stay unique: {ids:?}");
    assert!(
        ids.iter().all(|id| id.0 == 0),
        "the in-place splice must not allocate block ids: {ids:?}"
    );
}

/// done_when (c): a transitive single-block chain (caller → callee →
/// callee) composes hop by hop and stays single-block.
#[test]
fn transitive_single_block_chain_stays_single_block() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let inner = identity_fragment(2, FRAGMENT_INNER, ty);
    let outer = function(
        1,
        FRAGMENT,
        vec![param(0, ty)],
        ty,
        vec![MirLocal { id: MirLocalId(1), name: None, ty, mutable: false, span: test_span() }],
        vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![call_stmt(
                Some(MirPlace::local(MirLocalId(1))),
                MirFunctionId(2),
                vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
            )],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(1))))),
                span: test_span(),
            },
            span: test_span(),
        }],
        None,
    );
    let entry = entry_calling(1, 1, ty, Vec::new(), Vec::new());
    let mut functions = vec![entry, outer, inner];
    let errors = compose_fns(&mut functions);
    assert!(errors.is_empty(), "{errors:?}");
    let composed = &functions[0];
    assert_eq!(
        composed.blocks.len(),
        1,
        "every single-block hop must splice in place: {composed:#?}"
    );
    assert!(
        matches!(composed.blocks[0].terminator.kind, MirTerminatorKind::Return(_)),
        "the caller's original terminator must survive both hops: {composed:#?}"
    );
}

/// done_when (e): a multi-block (structured) callee keeps today's Goto-join
/// splice and the composed caller still fails the shared device-safe gate
/// with `SHAPE_DEVICE_MULTI_BLOCK_CFG` — asserted, not assumed.
#[test]
fn multi_block_callee_keeps_goto_join_and_fails_device_safe() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let add = |local: u32| MirStatement {
        kind: MirStatementKind::Assign {
            place: MirPlace::local(MirLocalId(local)),
            value: MirValue {
                id: MirValueId(0),
                kind: MirValueKind::Binary {
                    op: MirBinOp::Add,
                    lhs: MirOperand::Place(MirPlace::local(MirLocalId(0))),
                    rhs: MirOperand::Place(MirPlace::local(MirLocalId(0))),
                },
                ty,
                span: DEF_SPAN(),
            },
        },
        span: DEF_SPAN(),
    };
    let helper = function(
        1,
        FRAGMENT,
        vec![param(0, ty)],
        ty,
        vec![MirLocal { id: MirLocalId(1), name: None, ty, mutable: false, span: DEF_SPAN() }],
        vec![
            MirBlock {
                id: MirBlockId(0),
                statements: vec![add(1)],
                terminator: MirTerminator { kind: MirTerminatorKind::Goto(MirBlockId(1)), span: DEF_SPAN() },
                span: DEF_SPAN(),
            },
            MirBlock {
                id: MirBlockId(1),
                statements: Vec::new(),
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(1))))),
                    span: DEF_SPAN(),
                },
                span: DEF_SPAN(),
            },
        ],
        None,
    );
    let entry = entry_calling(1, 1, ty, Vec::new(), Vec::new());
    let mut functions = vec![entry, helper];
    assert!(compose_fns(&mut functions).is_empty(), "multi-block splice must keep composing");
    let composed = &functions[0];
    assert!(
        composed.blocks.len() > 1,
        "a multi-block callee must keep the Goto-join splice: {composed:#?}"
    );
    assert!(
        matches!(composed.blocks[0].terminator.kind, MirTerminatorKind::Goto(_)),
        "the Goto-join splice keeps its entry Goto: {composed:#?}"
    );

    // The composed caller stays fail-closed at the shared device-safe gate.
    let program = crate::mir::MirProgram { functions: functions.clone() };
    let mut context = crate::mir::MirDeviceContext::from_hir_roles(roles());
    context.attach_program(&program);
    let validated = crate::mir::ValidatedMir::new(program, crate::mir::MirValidationContext::new(&types))
        .expect("composed test MIR must validate");
    let verdict = crate::mir::classify_device_safe(&validated, &context);
    let radix_types::Lowerability::Rejected(gaps) = verdict else {
        panic!("a composed multi-block caller must stay fail-closed, got {verdict:?}");
    };
    assert!(
        gaps.iter()
            .any(|gap| gap.shape() == "device multi-block control flow"),
        "expected SHAPE_DEVICE_MULTI_BLOCK_CFG, got {gaps:?}"
    );
}

/// done_when (f): the single-unit route (no package registrations) composes
/// to the same single-block shape.
#[test]
fn single_unit_route_composes_single_block() {
    let source = r"
@ nucleum fragment
functio helper(f32 x) → f32 {
    redde x
}

@ nucleum
functio k(f32 x) → f32 {
    redde helper(x)
}
";
    let session = Session::new(Config::default().with_latin_locale());
    let analysis =
        driver::analyze_source(&session, "device.fab", source, AnalyzeRequest::default()).expect("analyze source");
    let roles = crate::mir::device_roles_from_hir(&analysis.hir);
    let lowered = crate::mir::lower_analyzed_unit(analysis.application_view(), MirLoweringOptions::default())
        .expect("composition must succeed");
    let kernel = lowered
        .validated
        .program()
        .functions
        .iter()
        .find(|function| {
            function
                .source
                .is_some_and(|source| roles.get(&source) == Some(&MirDeviceRole::Kernel))
        })
        .expect("the entry kernel must survive composition");
    assert_eq!(
        kernel.blocks.len(),
        1,
        "the single-unit route must compose single-block: {kernel:#?}"
    );
    assert!(
        matches!(kernel.blocks[0].terminator.kind, MirTerminatorKind::Return(_)),
        "the kernel's original Return terminator must survive: {kernel:#?}"
    );
}

// --- GEA3-A2b: composed mut-output caller-place preservation ---

use crate::mir::test_support::{indexed_local_place, rank1_tf32_type, u32_type};

/// A single-block callee whose `in` (MutRef) output argument is an indexed
/// caller place: the composed MIR must contain a write reaching that caller
/// place — the write may not terminate at a fresh `mutable: false` clone
/// local (the probe defect: `buf[idx]` composed parents emitted no output
/// write at all).
#[test]
fn mut_output_composition_reaches_caller_place() {
    let mut types = TypeTable::new();
    let (tensor_ty, elem_ty) = rank1_tf32_type(&mut types);
    let index_ty = u32_type(&mut types);
    let mut_param = |local: u32| MirParam { mode: MirParamMode::MutRef, ..param(local, elem_ty) };
    let helper = function(
        1,
        FRAGMENT,
        vec![param(0, elem_ty), mut_param(1)],
        elem_ty,
        vec![
            MirLocal { id: MirLocalId(0), name: None, ty: elem_ty, mutable: false, span: DEF_SPAN() },
            MirLocal { id: MirLocalId(1), name: None, ty: elem_ty, mutable: true, span: DEF_SPAN() },
        ],
        vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![MirStatement {
                kind: MirStatementKind::Assign {
                    place: MirPlace::local(MirLocalId(1)),
                    value: MirValue {
                        id: MirValueId(0),
                        kind: MirValueKind::Binary {
                            op: MirBinOp::Add,
                            lhs: MirOperand::Place(MirPlace::local(MirLocalId(0))),
                            rhs: MirOperand::Place(MirPlace::local(MirLocalId(1))),
                        },
                        ty: elem_ty,
                        span: DEF_SPAN(),
                    },
                },
                span: DEF_SPAN(),
            }],
            terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: DEF_SPAN() },
            span: DEF_SPAN(),
        }],
        None,
    );
    let output_place = indexed_local_place(MirLocalId(1), MirLocalId(2));
    let entry = function(
        0,
        ENTRY,
        vec![param(0, tensor_ty)],
        elem_ty,
        vec![
            MirLocal { id: MirLocalId(1), name: None, ty: tensor_ty, mutable: true, span: test_span() },
            MirLocal { id: MirLocalId(2), name: None, ty: index_ty, mutable: false, span: test_span() },
            MirLocal { id: MirLocalId(3), name: None, ty: elem_ty, mutable: false, span: test_span() },
        ],
        vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![call_stmt(
                None,
                MirFunctionId(1),
                vec![
                    MirOperand::Place(MirPlace::local(MirLocalId(3))),
                    MirOperand::Place(output_place.clone()),
                ],
            )],
            terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: test_span() },
            span: test_span(),
        }],
        Some(MirKernelShaderStage::Compute),
    );
    let mut functions = vec![entry, helper];
    assert!(
        compose_fns(&mut functions).is_empty(),
        "legal mut-output composition must compose"
    );
    let composed = &functions[0];
    assert_eq!(
        composed.blocks.len(),
        1,
        "the mut-output splice must stay single-block: {composed:#?}"
    );
    let statements = &composed.blocks[0].statements;
    let writeback_at = statements
        .iter()
        .position(|stmt| matches!(&stmt.kind, MirStatementKind::Assign { place, .. } if *place == output_place))
        .unwrap_or_else(|| panic!("a mut-output write must reach the caller place: {composed:#?}"));
    // The intermediate clone that carries the callee's mutation must be a
    // declared mutable local — never the fresh immutable binding of the
    // probe defect.
    let clone_local = statements
        .iter()
        .find_map(|stmt| match &stmt.kind {
            MirStatementKind::Assign {
                place,
                value: MirValue { kind: MirValueKind::Operand(MirOperand::Place(src)), .. },
            } if *src == output_place => match place.base {
                crate::mir::MirPlaceBase::Local(id) => Some(id),
                _ => None,
            },
            _ => None,
        })
        .unwrap_or_else(|| panic!("the mut-output clone bind must exist: {composed:#?}"));
    assert!(
        composed
            .locals
            .iter()
            .any(|local| local.id == clone_local && local.mutable),
        "the mut-output clone local must be declared mutable: {composed:#?}"
    );
    // Call semantics: the callee body's write precedes the caller-place
    // publish, which precedes the caller's own tail.
    let body_write_at = statements
        .iter()
        .position(|stmt| {
            matches!(&stmt.kind, MirStatementKind::Assign {
            place: MirPlace { base: crate::mir::MirPlaceBase::Local(id), projections }, ..
        } if *id == clone_local && projections.is_empty() && stmt.span == DEF_SPAN())
        })
        .unwrap_or_else(|| panic!("the callee body write must splice before the writeback: {composed:#?}"));
    assert!(
        body_write_at < writeback_at,
        "callee body write must precede the caller-place writeback: {composed:#?}"
    );
}

/// A bare caller local passed as an `in` (MutRef) argument aliases in place:
/// the callee's writes splice directly onto the caller's own local — no
/// fresh clone binding for the mut parameter at all.
#[test]
fn mut_output_aliases_bare_caller_local_in_place() {
    let mut types = TypeTable::new();
    let (_, elem_ty) = rank1_tf32_type(&mut types);
    let helper = function(
        1,
        FRAGMENT,
        vec![
            param(0, elem_ty),
            MirParam { mode: MirParamMode::MutRef, ..param(1, elem_ty) },
        ],
        elem_ty,
        vec![
            MirLocal { id: MirLocalId(0), name: None, ty: elem_ty, mutable: false, span: DEF_SPAN() },
            MirLocal { id: MirLocalId(1), name: None, ty: elem_ty, mutable: true, span: DEF_SPAN() },
        ],
        vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![MirStatement {
                kind: MirStatementKind::Assign {
                    place: MirPlace::local(MirLocalId(1)),
                    value: MirValue {
                        id: MirValueId(0),
                        kind: MirValueKind::Binary {
                            op: MirBinOp::Add,
                            lhs: MirOperand::Place(MirPlace::local(MirLocalId(0))),
                            rhs: MirOperand::Place(MirPlace::local(MirLocalId(1))),
                        },
                        ty: elem_ty,
                        span: DEF_SPAN(),
                    },
                },
                span: DEF_SPAN(),
            }],
            terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: DEF_SPAN() },
            span: DEF_SPAN(),
        }],
        None,
    );
    let entry = function(
        0,
        ENTRY,
        vec![param(0, elem_ty)],
        elem_ty,
        vec![MirLocal { id: MirLocalId(1), name: None, ty: elem_ty, mutable: true, span: test_span() }],
        vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![call_stmt(
                None,
                MirFunctionId(1),
                vec![
                    MirOperand::Place(MirPlace::local(MirLocalId(0))),
                    MirOperand::Place(MirPlace::local(MirLocalId(1))),
                ],
            )],
            terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: test_span() },
            span: test_span(),
        }],
        Some(MirKernelShaderStage::Compute),
    );
    let mut functions = vec![entry, helper];
    assert!(
        compose_fns(&mut functions).is_empty(),
        "legal mut-output composition must compose"
    );
    let composed = &functions[0];
    let reaches_caller_local = composed.blocks.iter().any(|block| {
        block.statements.iter().any(|stmt| {
            matches!(
                &stmt.kind,
                MirStatementKind::Assign { place: MirPlace { base: crate::mir::MirPlaceBase::Local(id), projections }, .. }
                    if *id == MirLocalId(1) && projections.is_empty()
            )
        })
    });
    assert!(
        reaches_caller_local,
        "the aliased mut-output write must land on the caller's local: {composed:#?}"
    );
    assert_eq!(
        composed.locals.len(),
        2,
        "only the immutable input clone may be added — the mut parameter aliases without a fresh local: {composed:#?}"
    );
    assert!(
        composed
            .locals
            .iter()
            .filter(|local| local.id != MirLocalId(1))
            .all(|local| !local.mutable),
        "the one fresh local is the immutable input clone: {composed:#?}"
    );
}

/// A multi-block (Goto-join) callee with an `in` (MutRef) indexed output
/// still publishes the mutation: the writeback lands in the cloned return
/// block, before control joins the caller's continue block.
#[test]
fn multi_block_mut_output_writes_back_on_return_join() {
    let mut types = TypeTable::new();
    let (tensor_ty, elem_ty) = rank1_tf32_type(&mut types);
    let index_ty = u32_type(&mut types);
    let write = |local: u32| MirStatement {
        kind: MirStatementKind::Assign {
            place: MirPlace::local(MirLocalId(local)),
            value: MirValue {
                id: MirValueId(0),
                kind: MirValueKind::Binary {
                    op: MirBinOp::Add,
                    lhs: MirOperand::Place(MirPlace::local(MirLocalId(0))),
                    rhs: MirOperand::Place(MirPlace::local(MirLocalId(local))),
                },
                ty: elem_ty,
                span: DEF_SPAN(),
            },
        },
        span: DEF_SPAN(),
    };
    let helper = function(
        1,
        FRAGMENT,
        vec![
            param(0, elem_ty),
            MirParam { mode: MirParamMode::MutRef, ..param(1, elem_ty) },
        ],
        elem_ty,
        vec![
            MirLocal { id: MirLocalId(0), name: None, ty: elem_ty, mutable: false, span: DEF_SPAN() },
            MirLocal { id: MirLocalId(1), name: None, ty: elem_ty, mutable: true, span: DEF_SPAN() },
        ],
        vec![
            MirBlock {
                id: MirBlockId(0),
                statements: vec![write(1)],
                terminator: MirTerminator { kind: MirTerminatorKind::Goto(MirBlockId(1)), span: DEF_SPAN() },
                span: DEF_SPAN(),
            },
            MirBlock {
                id: MirBlockId(1),
                statements: Vec::new(),
                terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: DEF_SPAN() },
                span: DEF_SPAN(),
            },
        ],
        None,
    );
    let output_place = indexed_local_place(MirLocalId(1), MirLocalId(2));
    let entry = function(
        0,
        ENTRY,
        vec![param(0, tensor_ty)],
        elem_ty,
        vec![
            MirLocal { id: MirLocalId(1), name: None, ty: tensor_ty, mutable: true, span: test_span() },
            MirLocal { id: MirLocalId(2), name: None, ty: index_ty, mutable: false, span: test_span() },
            MirLocal { id: MirLocalId(3), name: None, ty: elem_ty, mutable: false, span: test_span() },
        ],
        vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![call_stmt(
                None,
                MirFunctionId(1),
                vec![
                    MirOperand::Place(MirPlace::local(MirLocalId(3))),
                    MirOperand::Place(output_place.clone()),
                ],
            )],
            terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: test_span() },
            span: test_span(),
        }],
        Some(MirKernelShaderStage::Compute),
    );
    let mut functions = vec![entry, helper];
    assert!(
        compose_fns(&mut functions).is_empty(),
        "multi-block mut-output composition must compose"
    );
    let composed = &functions[0];
    assert!(
        composed.blocks.len() > 1,
        "a multi-block callee keeps the Goto-join splice: {composed:#?}"
    );
    assert!(
        composed.blocks.iter().any(|block| {
            block
                .statements
                .iter()
                .any(|stmt| matches!(&stmt.kind, MirStatementKind::Assign { place, .. } if *place == output_place))
        }),
        "the mut-output writeback must reach the caller place on the return join: {composed:#?}"
    );
    assert!(
        composed
            .blocks
            .iter()
            .any(|block| matches!(block.terminator.kind, MirTerminatorKind::Return(_))),
        "the caller's original Return terminator must survive the join: {composed:#?}"
    );
}

// ---------------------------------------------------------------------------
// KCM-05: synthetic kernel closures ride the explicit metadata carrier.
// ---------------------------------------------------------------------------

/// A source-less synthetic kernel closure body: identity over its one param.
fn synthetic_kernel_closure(id: u32, ty: MirType) -> MirFunction {
    let mut block = empty_block(0, Some(MirOperand::Place(MirPlace::local(MirLocalId(0)))));
    block.span = DEF_SPAN();
    block.terminator.span = DEF_SPAN();
    MirFunction {
        id: MirFunctionId(id),
        source: None,
        name: None,
        params: vec![param(0, ty)],
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: vec![block],
        return_ty: ty,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: Some(MirKernelShaderStage::Compute),
        span: DEF_SPAN(),
    }
}

fn synthetic_row(function_id: u32, parent: u32, ordinal: u32) -> (MirFunctionId, MirDeviceFunctionMetadata) {
    (
        MirFunctionId(function_id),
        MirDeviceFunctionMetadata {
            identity: MirSyntheticFunctionIdentity { parent: MirFunctionId(parent), ordinal },
            role: MirDeviceRole::Kernel,
            shader_stage: MirKernelShaderStage::Compute,
            visibility: MirFunctionVisibility::Private,
            origin: MirFunctionOrigin::SyntheticKernelClosure,
            lane_domain_local: None,
            refused_backends: Default::default(),
            tier: radix_mir::device::KernelTier::Default,
        },
    )
}

fn synthetic_metadata(rows: &[(MirFunctionId, MirDeviceFunctionMetadata)]) -> MirDeviceFunctionMetadataMap {
    rows.iter().copied().collect()
}

fn call_count_to(functions: &[MirFunction], callee: MirFunctionId) -> usize {
    functions
        .iter()
        .flat_map(|function| &function.blocks)
        .flat_map(|block| &block.statements)
        .filter(|statement| {
            matches!(
                &statement.kind,
                MirStatementKind::Call { callee: MirCallee::Function(id), .. } if *id == callee
            )
        })
        .count()
}

/// A kernel caller's direct call to a synthetic kernel closure composes: no
/// device call survives, and the composed private closure is pruned — no
/// standalone helper symbol survives.
#[test]
fn kernel_caller_composes_synthetic_kernel_closure() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let closure = synthetic_kernel_closure(2, ty);
    let entry = kernel_calling_helper(2, ty);
    let mut functions = vec![entry, closure];
    let metadata = synthetic_metadata(&[synthetic_row(2, 0, 0)]);
    let errors = compose_with_metadata(
        &mut functions,
        &[],
        &roles(),
        &metadata,
        ExpansionBudget::DOCUMENTED_DEFAULTS,
        None,
        &launch_entries(),
    );
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(fragment_call_count(&functions), 0, "no device call may survive");
    assert_eq!(
        call_count_to(&functions, MirFunctionId(2)),
        0,
        "the direct closure call composes"
    );
    assert!(
        functions
            .iter()
            .all(|function| function.id != MirFunctionId(2)),
        "the composed private closure must be pruned: {functions:#?}"
    );
}

/// A host caller's direct call to a synthetic kernel closure is the launch
/// boundary: it composes nothing, and the still-referenced closure stays.
#[test]
fn host_call_to_synthetic_closure_is_the_launch_boundary() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let closure = synthetic_kernel_closure(2, ty);
    let host = function(
        1,
        HOST,
        vec![param(0, ty)],
        ty,
        vec![MirLocal { id: MirLocalId(1), name: None, ty, mutable: false, span: test_span() }],
        vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![call_stmt(
                Some(MirPlace::local(MirLocalId(1))),
                MirFunctionId(2),
                vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
            )],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(1))))),
                span: test_span(),
            },
            span: test_span(),
        }],
        None,
    );
    let mut functions = vec![host, closure];
    let metadata = synthetic_metadata(&[synthetic_row(2, 1, 0)]);
    let errors = compose_with_metadata(
        &mut functions,
        &[],
        &roles(),
        &metadata,
        ExpansionBudget::DOCUMENTED_DEFAULTS,
        None,
        &launch_entries(),
    );
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(call_count_to(&functions, MirFunctionId(2)), 1, "the host boundary call stays");
    assert!(
        functions
            .iter()
            .any(|function| function.id == MirFunctionId(2)),
        "a host-referenced closure is not pruned"
    );
    let survivors = surviving_fragment_calls_with_metadata(&functions, &[], &roles(), &metadata);
    assert!(
        survivors.is_empty(),
        "the launch boundary is not a composition survivor: {survivors:?}"
    );
}

/// Without the carrier a source-less compute-stage function has no proven
/// device identity: the kernel caller's call is not a device site, so
/// composition leaves it for the fail-closed survivor gates downstream.
#[test]
fn uncarrid_synthetic_compute_is_not_a_device_callee() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let uncarried = synthetic_kernel_closure(2, ty);
    let entry = kernel_calling_helper(2, ty);
    let mut functions = vec![entry, uncarried];
    let errors = compose_with_metadata(
        &mut functions,
        &[],
        &roles(),
        &MirDeviceFunctionMetadataMap::default(),
        ExpansionBudget::DOCUMENTED_DEFAULTS,
        None,
        &launch_entries(),
    );
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(
        call_count_to(&functions, MirFunctionId(2)),
        1,
        "an uncarried call never composes"
    );
    assert!(
        functions
            .iter()
            .any(|function| function.id == MirFunctionId(2)),
        "an uncarried function is never pruned as a composed helper"
    );
}

// ---------------------------------------------------------------------------
// Depth is nesting, not the number of calls: sibling calls in one block share
// their enclosing depth; only a true call chain deepens it.
// ---------------------------------------------------------------------------

const CHAIN_BASE_SOURCE: u32 = 100;
const CHAIN_BASE_FUNCTION: u32 = 10;

fn chain_roles(links: u32) -> FxHashMap<DefId, MirDeviceRole> {
    let mut roles = roles();
    for link in 1..=links {
        roles.insert(DefId(CHAIN_BASE_SOURCE + link), MirDeviceRole::Fragment);
    }
    roles
}

/// A caller block that calls `callee` once per local `1..=calls` in sequence,
/// then returns the last result.
fn sequential_calls_block(calls: u32, callee: u32) -> (Vec<MirLocal>, MirBlock) {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let locals = (1..=calls)
        .map(|local| MirLocal { id: MirLocalId(local), name: None, ty, mutable: false, span: test_span() })
        .collect();
    let statements = (1..=calls)
        .map(|local| {
            call_stmt(
                Some(MirPlace::local(MirLocalId(local))),
                MirFunctionId(callee),
                vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
            )
        })
        .collect();
    let block = MirBlock {
        id: MirBlockId(0),
        statements,
        terminator: MirTerminator {
            kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(calls))))),
            span: test_span(),
        },
        span: test_span(),
    };
    (locals, block)
}

fn compose_with_roles(
    functions: &mut Vec<MirFunction>,
    roles: &FxHashMap<DefId, MirDeviceRole>,
) -> Vec<FragmentCompositionError> {
    compose(
        functions,
        &[],
        roles,
        ExpansionBudget::DOCUMENTED_DEFAULTS,
        None,
        &launch_entries(),
    )
}

#[test]
fn sequential_single_block_calls_do_not_count_as_nesting() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let (locals, block) = sequential_calls_block(20, 1);
    let entry = function(
        0,
        ENTRY,
        vec![param(0, ty)],
        ty,
        locals,
        vec![block],
        Some(MirKernelShaderStage::Compute),
    );
    let mut functions = vec![entry, identity_fragment(1, FRAGMENT, ty)];
    assert_eq!(
        ExpansionBudget::DOCUMENTED_DEFAULTS.max_depth,
        16,
        "test premise: the documented depth limit"
    );
    let errors = compose_fns(&mut functions);
    assert!(errors.is_empty(), "20 sequential calls are depth 1, not depth 20: {errors:?}");
    assert_eq!(fragment_call_count(&functions), 0, "every sequential call composes");
}

#[test]
fn sequential_calls_inside_a_called_fragment_nest_one_level() {
    // entry -> wrapper -> (20 sequential identity calls): depth 2, not 21.
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let (locals, block) = sequential_calls_block(20, 2);
    let wrapper = function(1, FRAGMENT, vec![param(0, ty)], ty, locals, vec![block], None);
    let identity = identity_fragment(2, FRAGMENT_INNER, ty);
    let entry = entry_calling(1, 1, ty, Vec::new(), Vec::new());
    let mut functions = vec![entry, wrapper, identity];
    let errors = compose_fns(&mut functions);
    assert!(
        errors.is_empty(),
        "sequential siblings in a callee are one nesting level: {errors:?}"
    );
    assert_eq!(fragment_call_count(&functions), 0);
}

#[test]
fn sequential_calls_do_not_hide_a_real_depth_overflow() {
    // The same 20-wide caller, but its callee is itself a depth-limit-deep
    // chain: nesting is still charged along the chain, siblings add nothing.
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let (mut functions, roles) = chain_program(ty, 16);
    // Replace the entry body: 20 sequential calls into the head of a 16-link
    // chain reach depth 16 each time (at the limit).
    let (locals, block) = sequential_calls_block(20, CHAIN_BASE_FUNCTION + 1);
    functions[0] = function(
        0,
        ENTRY,
        vec![param(0, ty)],
        ty,
        locals,
        vec![block],
        Some(MirKernelShaderStage::Compute),
    );
    let errors = compose_with_roles(&mut functions, &roles);
    assert!(
        errors.is_empty(),
        "16 nested levels reached 20 times in sequence still fit: {errors:?}"
    );
    let (mut functions, roles) = chain_program(ty, 17);
    let (locals, block) = sequential_calls_block(20, CHAIN_BASE_FUNCTION + 1);
    functions[0] = function(
        0,
        ENTRY,
        vec![param(0, ty)],
        ty,
        locals,
        vec![block],
        Some(MirKernelShaderStage::Compute),
    );
    let errors = compose_with_roles(&mut functions, &roles);
    assert!(
        errors
            .iter()
            .any(|err| err.issue == ISSUE_EXPANSION && err.reason == "depth"),
        "a 17-deep chain overflows however many siblings precede it: {errors:?}"
    );
}

/// `entry -> f1 -> f2 -> ... -> f<links>`, every link a single-block fragment
/// and the last an identity. The call to `f<k>` sits at depth `k`.
fn chain_program(ty: MirType, links: u32) -> (Vec<MirFunction>, FxHashMap<DefId, MirDeviceRole>) {
    let mut functions = vec![entry_calling(1, CHAIN_BASE_FUNCTION + 1, ty, Vec::new(), Vec::new())];
    for link in 1..=links {
        let id = CHAIN_BASE_FUNCTION + link;
        let source = DefId(CHAIN_BASE_SOURCE + link);
        if link == links {
            functions.push(identity_fragment(id, source, ty));
        } else {
            functions.push(function(
                id,
                source,
                vec![param(0, ty)],
                ty,
                vec![MirLocal { id: MirLocalId(1), name: None, ty, mutable: false, span: test_span() }],
                vec![MirBlock {
                    id: MirBlockId(0),
                    statements: vec![call_stmt(
                        Some(MirPlace::local(MirLocalId(1))),
                        MirFunctionId(id + 1),
                        vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
                    )],
                    terminator: MirTerminator {
                        kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(1))))),
                        span: test_span(),
                    },
                    span: test_span(),
                }],
                None,
            ));
        }
    }
    (functions, chain_roles(links))
}

#[test]
fn sixteen_deep_chain_composes() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let (mut functions, roles) = chain_program(ty, 16);
    let errors = compose_with_roles(&mut functions, &roles);
    assert!(errors.is_empty(), "a chain exactly at the limit composes: {errors:?}");
    assert_eq!(functions[0].blocks.len(), 1, "single-block hops splice in place");
}

#[test]
fn seventeen_deep_chain_is_rejected_with_the_chain_named() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let (mut functions, roles) = chain_program(ty, 17);
    let errors = compose_with_roles(&mut functions, &roles);
    let err = errors
        .iter()
        .find(|err| err.issue == ISSUE_EXPANSION && err.reason == "depth")
        .unwrap_or_else(|| panic!("a 17-deep chain must overflow the depth limit, got {errors:?}"));
    let arg = |name: &str| {
        err.args
            .iter()
            .find(|arg| arg.name == name)
            .map(|arg| arg.value.clone())
    };
    assert_eq!(arg("observed").as_deref(), Some("17"));
    assert_eq!(arg("limit").as_deref(), Some("16"));
    let chain = arg("chain").expect("chain named");
    assert!(chain.starts_with("#1 → #101 → #102"), "chain starts at the entry: {chain}");
    assert!(chain.ends_with("#116 → #117"), "chain names the overflowing call: {chain}");
}

#[test]
fn multi_block_chain_still_counts_nesting_along_the_goto_join() {
    // A multi-block middle link takes the Goto-join splice; the chain below
    // it is still charged one level per hop (tail depth is restored, callee
    // blocks stay deeper).
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let (mut functions, roles) = chain_program(ty, 17);
    // Link 2 (function 12) gets a second block: Goto(1) then Return(local1).
    let link = functions
        .iter_mut()
        .find(|function| function.id == MirFunctionId(CHAIN_BASE_FUNCTION + 2))
        .expect("link 2");
    let mut tail = link.blocks.remove(0);
    let ret = std::mem::replace(
        &mut tail.terminator,
        MirTerminator { kind: MirTerminatorKind::Goto(MirBlockId(1)), span: test_span() },
    );
    link.blocks.push(tail);
    link.blocks
        .push(MirBlock { id: MirBlockId(1), statements: Vec::new(), terminator: ret, span: test_span() });
    let errors = compose_with_roles(&mut functions, &roles);
    assert!(
        errors
            .iter()
            .any(|err| err.issue == ISSUE_EXPANSION && err.reason == "depth"),
        "a 17-deep chain through a Goto-join link must still overflow: {errors:?}"
    );
}

/// A-129: a generic `Definition` callee whose explicit type args match no
/// monomorphization of its source must not resolve to an arbitrary same-source
/// function. The unmatched call stays in place and the expansion reports it
/// `unresolved` instead of inlining the wrong instance.
#[test]
fn unmatched_type_args_do_not_fall_back_to_a_same_source_function() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let other = MirType::semantic(types.primitive(crate::semantic::Primitive::Textus));
    let template = identity_fragment(1, FRAGMENT, ty);
    let instance = identity_fragment(2, FRAGMENT, ty);
    let mut entry = entry_calling(1, 1, ty, Vec::new(), Vec::new());
    let MirStatementKind::Call { callee, .. } = &mut entry.blocks[0].statements[0].kind else {
        panic!("entry starts with a call");
    };
    *callee = MirCallee::Definition { source: FRAGMENT, type_args: vec![other] };
    let monomorphizations = [MirMonomorphization {
        key: crate::mir::MirMonomorphizationKey { source: FRAGMENT, type_args: vec![ty], shape_args: Vec::new() },
        function: MirFunctionId(2),
    }];
    let mut functions = vec![entry, template, instance];
    let errors = compose(
        &mut functions,
        &monomorphizations,
        &roles(),
        ExpansionBudget::DOCUMENTED_DEFAULTS,
        None,
        &launch_entries(),
    );
    assert!(
        errors
            .iter()
            .any(|err| err.issue == ISSUE_EXPANSION && err.reason == "unresolved"),
        "an unmatched generic device call must fail closed as unresolved, got {errors:?}"
    );
    assert!(
        matches!(
            &functions[0].blocks[0].statements[0].kind,
            MirStatementKind::Call { callee: MirCallee::Definition { .. }, .. }
        ),
        "the unmatched call must stay in place, not be inlined from a same-source function"
    );
}

// --- Declared `filum` reduction inside a composed kernel helper ---

/// A declared `filum` carries its binder locals and its captured term body in
/// the id space of the function that wrote it. Composing that function into a
/// caller renames them with the rest of the callee: a clone that left them
/// behind would point the reduction at whatever the caller declares at those
/// ids (here the caller's own total and the bound tensor copy).
#[test]
fn kernel_helper_filum_declaration_follows_the_alpha_rename() {
    use radix_mir::{
        MirFilumCoordinate, MirFilumForm, MirFilumReduceOp, MirIntrinsic, MirPlaceBase, MirReduceOp, MirRuntimeCall,
        MirTemp,
    };

    let mut types = TypeTable::new();
    let (tensor, element) = crate::mir::test_support::rank1_tf32_type(&mut types);
    let index_ty = crate::mir::test_support::u32_type(&mut types);
    let local =
        |id: u32, ty: MirType| MirLocal { id: MirLocalId(id), name: None, ty, mutable: false, span: DEF_SPAN() };
    let assign = |place: MirPlace, value_id: u32, operand: MirOperand| MirStatement {
        kind: MirStatementKind::Assign {
            place,
            value: MirValue {
                id: MirValueId(value_id),
                kind: MirValueKind::Operand(operand),
                ty: element,
                span: DEF_SPAN(),
            },
        },
        span: DEF_SPAN(),
    };

    // helper(a): total = filum sum over a, coordinate i (1), element binder v
    // (2), term result (3), total (4); the term body routes v through temp 0.
    let term_body = vec![
        assign(
            MirPlace::temp(radix_mir::MirTempId(0)),
            0,
            MirOperand::Place(MirPlace::local(MirLocalId(2))),
        ),
        assign(MirPlace::local(MirLocalId(3)), 1, MirOperand::Temp(radix_mir::MirTempId(0))),
    ];
    let filum = MirFilumReduceOp {
        op: MirReduceOp::Sum,
        form: MirFilumForm::Declared,
        filum_binder: None,
        term_binder: MirLocalId(2),
        term_binder_name: None,
        coordinates: vec![MirFilumCoordinate { local: MirLocalId(1), name: None }],
        lane_width: 32,
        term_body,
        term_captures: None,
        term_result: MirLocalId(3),
        term_ty: element,
        group: None,
    };
    let mut helper = function(
        1,
        KERNEL_HELPER,
        vec![param(0, tensor)],
        element,
        vec![
            local(1, index_ty),
            local(2, element),
            local(3, element),
            local(4, element),
        ],
        vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![MirStatement {
                kind: MirStatementKind::RuntimeCall {
                    destination: Some(MirPlace::local(MirLocalId(4))),
                    call: MirRuntimeCall {
                        intrinsic: MirIntrinsic::FilumReduce(filum),
                        args: vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
                        return_ty: element,
                    },
                },
                span: DEF_SPAN(),
            }],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(4))))),
                span: DEF_SPAN(),
            },
            span: DEF_SPAN(),
        }],
        None,
    );
    helper
        .temps
        .push(MirTemp { id: radix_mir::MirTempId(0), ty: element, span: DEF_SPAN() });
    // The caller declares a local at id 1 of its own: the callee's coordinate id.
    let entry = function(
        0,
        ENTRY,
        vec![param(0, tensor)],
        element,
        vec![local(1, element)],
        vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![call_stmt(
                Some(MirPlace::local(MirLocalId(1))),
                MirFunctionId(1),
                vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
            )],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(1))))),
                span: test_span(),
            },
            span: test_span(),
        }],
        Some(MirKernelShaderStage::Compute),
    );

    let mut functions = vec![entry, helper];
    assert!(compose_fns(&mut functions).is_empty(), "kernel helper must compose");
    let composed = &functions[0];
    let local_ty = |id: MirLocalId| {
        composed
            .params
            .iter()
            .find(|param| param.local == id)
            .map(|param| param.ty)
            .or_else(|| {
                composed
                    .locals
                    .iter()
                    .find(|local| local.id == id)
                    .map(|local| local.ty)
            })
    };
    let filum = composed
        .blocks
        .iter()
        .flat_map(|block| &block.statements)
        .find_map(|stmt| match &stmt.kind {
            MirStatementKind::RuntimeCall { call, .. } => match &call.intrinsic {
                MirIntrinsic::FilumReduce(filum) => Some(filum),
                _ => None,
            },
            _ => None,
        })
        .expect("the composed caller carries the filum declaration");

    let [coordinate] = &filum.coordinates[..] else {
        panic!("one coordinate binder: {filum:#?}");
    };
    assert_eq!(
        local_ty(coordinate.local),
        Some(index_ty),
        "coordinate names the callee's index local: {composed:#?}"
    );
    assert_eq!(
        local_ty(filum.term_binder),
        Some(element),
        "element binder names the callee's element local"
    );
    assert_eq!(
        local_ty(filum.term_result),
        Some(element),
        "term result names the callee's result local"
    );
    assert_ne!(filum.term_binder, filum.term_result);
    assert_ne!(coordinate.local, MirLocalId(1), "the caller's own local keeps its id");

    let [read_binder, write_result] = &filum.term_body[..] else {
        panic!("two term-body statements: {filum:#?}");
    };
    let MirStatementKind::Assign { place: temp_place, value: first } = &read_binder.kind else {
        panic!("term body starts with the temp assign");
    };
    let MirStatementKind::Assign { place: result_place, value: second } = &write_result.kind else {
        panic!("term body ends with the result assign");
    };
    let MirPlaceBase::Temp(temp) = temp_place.base else {
        panic!("term body routes through a temp");
    };
    assert!(
        composed.temps.iter().any(|candidate| candidate.id == temp),
        "the term-body temp is declared: {composed:#?}"
    );
    assert!(
        matches!(&first.kind, MirValueKind::Operand(MirOperand::Place(place)) if place.base == MirPlaceBase::Local(filum.term_binder)),
        "the term body reads the element binder: {filum:#?}"
    );
    assert_eq!(
        result_place.base,
        MirPlaceBase::Local(filum.term_result),
        "the term body writes the term result"
    );
    assert!(
        matches!(&second.kind, MirValueKind::Operand(MirOperand::Temp(read)) if *read == temp),
        "the term result reads the same temp: {filum:#?}"
    );
    assert_ne!(first.id, second.id, "value ids stay distinct");
}
