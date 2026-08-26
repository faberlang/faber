//! Coverage for nucleum-fragment MIR composition.

use super::{
    compose, reject_surviving_fragment_calls, surviving_fragment_calls, ExpansionBudget, ISSUE_CYCLE, ISSUE_EXPANSION,
};
use crate::diagnostics::{semantic_message, DiagnosticArg};
use crate::driver::{self, AnalyzeRequest, Config, Session};
use crate::hir::DefId;
use crate::lexer::Span;
use crate::mir::{
    MirBinOp, MirBlock, MirBlockId, MirCallee, MirDeviceRole, MirFunction, MirFunctionId, MirKernelShaderStage,
    MirLocal, MirLocalId, MirOperand, MirParam, MirParamMode, MirPlace, MirStatement, MirStatementKind, MirTerminator,
    MirTerminatorKind, MirType, MirValue, MirValueId, MirValueKind,
};
use crate::semantic::{SemanticErrorKind, TypeTable};
use crate::test_support::{test_span, test_span_between};
use rustc_hash::{FxHashMap, FxHashSet};

const ENTRY: DefId = DefId(1);
const FRAGMENT: DefId = DefId(2);
const FRAGMENT_INNER: DefId = DefId(6);
const FRAGMENT_B: DefId = DefId(7);
const KERNEL_HELPER: DefId = DefId(8);
const KERNEL_INNER: DefId = DefId(9);
const HOST: DefId = DefId(10);

fn with_budget_override<R>(budget: ExpansionBudget, f: impl FnOnce() -> R) -> R {
    struct Reset(Option<ExpansionBudget>);
    impl Drop for Reset {
        fn drop(&mut self) {
            super::BUDGET_OVERRIDE.with(|slot| slot.set(self.0));
        }
    }
    let prev = super::BUDGET_OVERRIDE.with(|slot| slot.replace(Some(budget)));
    let _reset = Reset(prev);
    f()
}

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
    surviving_fragment_calls(functions, &[], &roles()).len()
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
    let errors = reject_surviving_fragment_calls(&functions, &[], &roles());
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
        .map(|arg| arg.value.as_str())
        .unwrap_or("");
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
        .map(|arg| arg.value.as_str())
        .unwrap_or("");
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
    assert!(err
        .args
        .iter()
        .any(|arg| arg.name == "limit" && arg.value == "1"));
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
fn catalog_emits_cycle_and_expansion() {
    let cycle_args = vec![
        DiagnosticArg::new("issue", ISSUE_CYCLE),
        DiagnosticArg::new("reason", "cycle"),
        DiagnosticArg::new("cycle", "ping → pong → ping"),
    ];
    assert_eq!(
        semantic_message(SemanticErrorKind::OperandTypesInvalid, &cycle_args).as_deref(),
        Some("nucleum fragment call cycle: ping → pong → ping")
    );
    let expansion_args = vec![
        DiagnosticArg::new("issue", ISSUE_EXPANSION),
        DiagnosticArg::new("reason", "depth"),
        DiagnosticArg::new("kind", "depth"),
        DiagnosticArg::new("observed", "17"),
        DiagnosticArg::new("limit", "16"),
        DiagnosticArg::new("chain", "k → a → b"),
    ];
    assert_eq!(
        semantic_message(SemanticErrorKind::OperandTypesInvalid, &expansion_args).as_deref(),
        Some("nucleum fragment expansion exceeds depth limit 16 (observed 17) along k → a → b")
    );
}

#[test]
fn legal_entry_to_fragment_composes_through_lowering() {
    let source = r"
@ nucleum fragment
functio helper(fractus<f32> x) → fractus<f32> {
    redde x
}

@ nucleum
functio k(fractus<f32> x) → fractus<f32> {
    redde helper(x)
}
";
    let session = Session::new(Config::default());
    let mut analysis =
        driver::analyze_source(&session, "device.fab", source, AnalyzeRequest::default()).expect("analyze source");
    let roles = crate::mir::device_roles_from_hir(&analysis.hir);
    let lowered = crate::mir::lower_analyzed_unit(&mut analysis, Default::default()).expect("composition must succeed");
    assert!(
        surviving_fragment_calls(&lowered.program.functions, &[], &roles).is_empty(),
        "no fragment call may survive lowering: {:#?}",
        lowered.program.functions
    );
}

#[test]
fn source_cycle_is_rejected_at_lower() {
    let source = r"
@ nucleum fragment
functio ping(fractus<f32> x) → fractus<f32> {
    redde pong(x)
}

@ nucleum fragment
functio pong(fractus<f32> x) → fractus<f32> {
    redde ping(x)
}

@ nucleum
functio k(fractus<f32> x) → fractus<f32> {
    redde ping(x)
}
";
    let session = Session::new(Config::default());
    let mut analysis =
        driver::analyze_source(&session, "device.fab", source, AnalyzeRequest::default()).expect("analyze source");
    let errors =
        crate::mir::lower_analyzed_unit(&mut analysis, Default::default()).expect_err("cycle must fail closed");
    assert!(
        errors.iter().any(|err| err.issue == ISSUE_CYCLE),
        "expected nucleum_fragment_cycle, got {errors:?}"
    );
}

#[test]
fn budget_override_rejects_source_expansion() {
    let source = r"
@ nucleum fragment
functio helper(fractus<f32> x) → fractus<f32> {
    redde x
}

@ nucleum
functio k(fractus<f32> x) → fractus<f32> {
    redde helper(x)
}
";
    let budget = ExpansionBudget { max_concrete_instances: 0, ..ExpansionBudget::DOCUMENTED_DEFAULTS };
    with_budget_override(budget, || {
        let session = Session::new(Config::default());
        let mut analysis =
            driver::analyze_source(&session, "device.fab", source, AnalyzeRequest::default()).expect("analyze source");
        let errors =
            crate::mir::lower_analyzed_unit(&mut analysis, Default::default()).expect_err("budget must fail closed");
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
        "the composed private helper must be pruned: {:#?}",
        functions
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
        "composed transitive helpers must be pruned: {:#?}",
        functions
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
        .map(|arg| arg.value.as_str())
        .unwrap_or("");
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
    assert!(err
        .args
        .iter()
        .any(|arg| arg.name == "limit" && arg.value == "0"));
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
        "the called entry must stay a program symbol: {:#?}",
        functions
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
functio helper(fractus<f32> x) → fractus<f32> {
    redde x * 2.0
}

@ nucleum
functio k(fractus<f32> x) → fractus<f32> {
    redde helper(x)
}
";
    let session = Session::new(Config::default());
    let mut analysis =
        driver::analyze_source(&session, "device.fab", source, AnalyzeRequest::default()).expect("analyze source");
    let roles = crate::mir::device_roles_from_hir(&analysis.hir);
    let lowered =
        crate::mir::lower_analyzed_unit(&mut analysis, Default::default()).expect("kernel helper must compose");
    assert!(
        surviving_fragment_calls(&lowered.program.functions, &[], &roles).is_empty(),
        "no device call may survive lowering: {:#?}",
        lowered.program.functions
    );
    let kernel_symbols = lowered
        .program
        .functions
        .iter()
        .filter(|function| {
            function
                .source
                .is_some_and(|source| roles.get(&source) == Some(&MirDeviceRole::Kernel))
        })
        .count();
    assert_eq!(
        kernel_symbols, 1,
        "only the uncalled entry kernel may remain; the composed helper is pruned: {:#?}",
        lowered.program.functions
    );
}
