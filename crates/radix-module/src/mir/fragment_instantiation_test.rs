//! Coverage for nucleum-fragment instantiation checking and the call-legality matrix.

use super::{FragmentInstantiationError, ISSUE_CONTEXT, ISSUE_TYPE, check_with_metadata};
use crate::diagnostics::DiagnosticArg;
use crate::driver::{self, AnalyzeRequest, Config, Session};
use crate::hir::DefId;
use crate::lexer::Span;
use crate::mir::MirLoweringOptions;
use crate::mir::{
    MirBlock, MirBlockId, MirCallee, MirDeviceFunctionMetadataMap, MirDeviceRole, MirFunction, MirFunctionId,
    MirKernelShaderStage, MirLayoutTable, MirLocal, MirLocalId, MirMonomorphization, MirMonomorphizationKey,
    MirOperand, MirParam, MirParamMode, MirPlace, MirStatement, MirStatementKind, MirTerminator, MirTerminatorKind,
    MirType,
};
use crate::semantic::TypeTable;
use crate::test_support::{test_span, test_span_between};

/// Plain-wrapper stand-in for the deleted production `check` (merge-16):
/// the tests exercise instantiation checking without an explicit metadata carrier.
fn check(
    functions: &[MirFunction],
    monomorphizations: &[MirMonomorphization],
    layouts: &MirLayoutTable,
    types: &TypeTable,
    roles: &FxHashMap<DefId, MirDeviceRole>,
    summaries: Option<&super::FragmentEffectTable>,
) -> Vec<FragmentInstantiationError> {
    check_with_metadata(
        functions,
        monomorphizations,
        layouts,
        types,
        roles,
        summaries,
        &MirDeviceFunctionMetadataMap::default(),
    )
}
use rustc_hash::FxHashMap;

const ENTRY: DefId = DefId(1);
const FRAGMENT: DefId = DefId(2);
const HOST: DefId = DefId(3);
const GRAPHICS: DefId = DefId(4);
const ENTRY_INNER: DefId = DefId(5);
const FRAGMENT_INNER: DefId = DefId(6);

const CALL_SPAN: fn() -> Span = || test_span_between(40, 52);
const DEF_SPAN: fn() -> Span = || test_span_between(8, 20);

fn f32_ty(types: &mut TypeTable) -> MirType {
    crate::mir::test_support::f32_type(types)
}

fn text_ty(types: &TypeTable) -> MirType {
    MirType::semantic(types.primitive(crate::semantic::Primitive::Textus))
}

fn param(local: u32, ty: MirType, mode: MirParamMode) -> MirParam {
    MirParam { local: MirLocalId(local), name: None, ty, mode, span: test_span() }
}

fn function(
    id: u32,
    source: DefId,
    params: Vec<MirParam>,
    return_ty: MirType,
    callee: Option<MirCallee>,
    args: Vec<MirOperand>,
    dest: Option<MirPlace>,
    shader_stage: Option<MirKernelShaderStage>,
) -> MirFunction {
    let statements = if let Some(callee) = callee {
        vec![MirStatement { kind: MirStatementKind::Call { destination: dest, callee, args }, span: CALL_SPAN() }]
    } else {
        Vec::new()
    };
    MirFunction {
        id: MirFunctionId(id),
        source: Some(source),
        name: None,
        params,
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements,
            terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: test_span() },
            span: test_span(),
        }],
        return_ty,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage,
        span: if source == FRAGMENT || source == FRAGMENT_INNER || source == ENTRY_INNER {
            DEF_SPAN()
        } else {
            test_span()
        },
    }
}

fn roles() -> FxHashMap<DefId, MirDeviceRole> {
    let mut roles = FxHashMap::default();
    roles.insert(ENTRY, MirDeviceRole::Kernel);
    roles.insert(ENTRY_INNER, MirDeviceRole::Kernel);
    roles.insert(FRAGMENT, MirDeviceRole::Fragment);
    roles.insert(FRAGMENT_INNER, MirDeviceRole::Fragment);
    roles
}

fn expect_issue<'a>(
    errors: &'a [FragmentInstantiationError],
    issue: &str,
    reason: &str,
) -> &'a FragmentInstantiationError {
    errors
        .iter()
        .find(|err| err.issue == issue && err.reason == reason)
        .unwrap_or_else(|| panic!("expected {issue} reason={reason}, got {errors:?}"))
}

fn check_fns(functions: &[MirFunction], types: &TypeTable) -> Vec<FragmentInstantiationError> {
    check(functions, &[], &MirLayoutTable::from_types(types), types, &roles(), None)
}

#[test]
fn entry_to_fragment_matching_signature_is_legal() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let helper = function(
        1,
        FRAGMENT,
        vec![param(0, ty, MirParamMode::Owned)],
        ty,
        None,
        Vec::new(),
        None,
        None,
    );
    let entry = function(
        0,
        ENTRY,
        vec![param(0, ty, MirParamMode::Owned)],
        ty,
        Some(MirCallee::Function(MirFunctionId(1))),
        vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
        Some(MirPlace::local(MirLocalId(0))),
        Some(MirKernelShaderStage::Compute),
    );
    let errors = check_fns(&[entry, helper], &types);
    assert!(errors.is_empty(), "legal Entry→Fragment must pass: {errors:?}");
}

#[test]
fn fragment_to_fragment_matching_signature_is_legal() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let inner = function(
        1,
        FRAGMENT_INNER,
        vec![param(0, ty, MirParamMode::Owned)],
        ty,
        None,
        Vec::new(),
        None,
        None,
    );
    let outer = function(
        0,
        FRAGMENT,
        vec![param(0, ty, MirParamMode::Owned)],
        ty,
        Some(MirCallee::Function(MirFunctionId(1))),
        vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
        Some(MirPlace::local(MirLocalId(0))),
        None,
    );
    let errors = check_fns(&[outer, inner], &types);
    assert!(errors.is_empty(), "legal Fragment→Fragment must pass: {errors:?}");
}

#[test]
fn entry_to_entry_is_checked_and_legal() {
    // DFV2-4: device code may call any concrete device function; the former
    // `nested_dispatch` reject is now the ordinary typed instantiation check.
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let inner = function(
        1,
        ENTRY_INNER,
        vec![param(0, ty, MirParamMode::Owned)],
        ty,
        None,
        Vec::new(),
        None,
        Some(MirKernelShaderStage::Compute),
    );
    let outer = function(
        0,
        ENTRY,
        vec![param(0, ty, MirParamMode::Owned)],
        ty,
        Some(MirCallee::Function(MirFunctionId(1))),
        vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
        Some(MirPlace::local(MirLocalId(0))),
        Some(MirKernelShaderStage::Compute),
    );
    let errors = check_fns(&[outer, inner], &types);
    assert!(errors.is_empty(), "legal Entry→Entry must pass for composition: {errors:?}");
}

#[test]
fn entry_to_entry_type_mismatch_is_a_type_error() {
    // DFV2-4: the entry→entry row runs the typed check — a mismatched
    // argument reports the instantiation TYPE issue, not a legality reject.
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let text = text_ty(&types);
    let inner = function(
        1,
        ENTRY_INNER,
        vec![param(0, ty, MirParamMode::Owned)],
        ty,
        None,
        Vec::new(),
        None,
        Some(MirKernelShaderStage::Compute),
    );
    let outer = function(
        0,
        ENTRY,
        vec![param(0, text, MirParamMode::Owned)],
        text,
        Some(MirCallee::Function(MirFunctionId(1))),
        vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
        Some(MirPlace::local(MirLocalId(0))),
        Some(MirKernelShaderStage::Compute),
    );
    let errors = check_fns(&[outer, inner], &types);
    expect_issue(&errors, ISSUE_TYPE, "argument");
}

#[test]
fn fragment_to_entry_is_rejected() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let entry = function(
        1,
        ENTRY,
        vec![param(0, ty, MirParamMode::Owned)],
        ty,
        None,
        Vec::new(),
        None,
        Some(MirKernelShaderStage::Compute),
    );
    let fragment = function(
        0,
        FRAGMENT,
        vec![param(0, ty, MirParamMode::Owned)],
        ty,
        Some(MirCallee::Function(MirFunctionId(1))),
        vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
        Some(MirPlace::local(MirLocalId(0))),
        None,
    );
    expect_issue(&check_fns(&[fragment, entry], &types), ISSUE_CONTEXT, "entry_callee");
}

#[test]
fn nucleus_to_graphics_is_rejected() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let graphics = function(
        1,
        GRAPHICS,
        vec![param(0, ty, MirParamMode::Owned)],
        ty,
        None,
        Vec::new(),
        None,
        Some(MirKernelShaderStage::Fragment),
    );
    let entry = function(
        0,
        ENTRY,
        vec![param(0, ty, MirParamMode::Owned)],
        ty,
        Some(MirCallee::Function(MirFunctionId(1))),
        vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
        Some(MirPlace::local(MirLocalId(0))),
        Some(MirKernelShaderStage::Compute),
    );
    expect_issue(&check_fns(&[entry, graphics], &types), ISSUE_CONTEXT, "graphics_stage");
}

#[test]
fn host_to_fragment_is_rejected() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let fragment = function(
        1,
        FRAGMENT,
        vec![param(0, ty, MirParamMode::Owned)],
        ty,
        None,
        Vec::new(),
        None,
        None,
    );
    let host = function(
        0,
        HOST,
        vec![param(0, ty, MirParamMode::Owned)],
        ty,
        Some(MirCallee::Function(MirFunctionId(1))),
        vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
        Some(MirPlace::local(MirLocalId(0))),
        None,
    );
    let errors = check_fns(&[host, fragment], &types);
    let err = expect_issue(&errors, ISSUE_CONTEXT, "host_callable");
    assert!(
        err.args
            .iter()
            .any(|arg| arg.name == "caller" && arg.value == "host")
    );
}

#[test]
fn nucleus_to_ordinary_is_rejected() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let host = function(
        1,
        HOST,
        vec![param(0, ty, MirParamMode::Owned)],
        ty,
        None,
        Vec::new(),
        None,
        None,
    );
    let entry = function(
        0,
        ENTRY,
        vec![param(0, ty, MirParamMode::Owned)],
        ty,
        Some(MirCallee::Function(MirFunctionId(1))),
        vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
        Some(MirPlace::local(MirLocalId(0))),
        Some(MirKernelShaderStage::Compute),
    );
    expect_issue(&check_fns(&[entry, host], &types), ISSUE_CONTEXT, "host_callable");
}

#[test]
fn tensor_shape_mismatch_emits_type() {
    let mut types = TypeTable::new();
    let element = types.sized_numeric(crate::semantic::Primitive::Fractus, crate::semantic::NumericWidth::F32);
    let shape4 = types.intern_index(crate::semantic::IndexExpr::Literal(4));
    let shape8 = types.intern_index(crate::semantic::IndexExpr::Literal(8));
    let t4 = MirType::semantic(types.tensor_with_shape(element, shape4));
    let t8 = MirType::semantic(types.tensor_with_shape(element, shape8));
    let helper = function(
        1,
        FRAGMENT,
        vec![param(0, t4, MirParamMode::Owned)],
        t4,
        None,
        Vec::new(),
        None,
        None,
    );
    let entry = function(
        0,
        ENTRY,
        vec![param(0, t8, MirParamMode::Owned)],
        t4,
        Some(MirCallee::Function(MirFunctionId(1))),
        vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
        Some(MirPlace::local(MirLocalId(0))),
        Some(MirKernelShaderStage::Compute),
    );
    expect_issue(&check_fns(&[entry, helper], &types), ISSUE_TYPE, "shape");
}

#[test]
fn same_shape_with_distinct_index_ids_is_accepted() {
    let mut types = TypeTable::new();
    let element = types.sized_numeric(crate::semantic::Primitive::Fractus, crate::semantic::NumericWidth::F32);
    let shape = types.intern_index(crate::semantic::IndexExpr::Literal(4));
    let same_shape = types.intern_index(crate::semantic::IndexExpr::Literal(4));
    assert_ne!(shape, same_shape);
    let expected = MirType::semantic(types.tensor_with_shape(element, shape));
    let actual = MirType::semantic(types.tensor_with_shape(element, same_shape));
    let helper = function(
        1,
        FRAGMENT,
        vec![param(0, expected, MirParamMode::Owned)],
        expected,
        None,
        Vec::new(),
        None,
        None,
    );
    let entry = function(
        0,
        ENTRY,
        vec![param(0, actual, MirParamMode::Owned)],
        actual,
        Some(MirCallee::Function(MirFunctionId(1))),
        vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
        Some(MirPlace::local(MirLocalId(0))),
        Some(MirKernelShaderStage::Compute),
    );

    let errors = check_fns(&[entry, helper], &types);
    assert!(errors.is_empty(), "equal tensor shapes should instantiate: {errors:?}");
}

#[test]
fn argument_type_mismatch_emits_type() {
    let mut types = TypeTable::new();
    let f32 = f32_ty(&mut types);
    let u32 = crate::mir::test_support::u32_type(&mut types);
    let helper = function(
        1,
        FRAGMENT,
        vec![param(0, f32, MirParamMode::Owned)],
        f32,
        None,
        Vec::new(),
        None,
        None,
    );
    let entry = function(
        0,
        ENTRY,
        vec![param(0, u32, MirParamMode::Owned)],
        f32,
        Some(MirCallee::Function(MirFunctionId(1))),
        vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
        Some(MirPlace::local(MirLocalId(0))),
        Some(MirKernelShaderStage::Compute),
    );
    let errors = check_fns(&[entry, helper], &types);
    let err = expect_issue(&errors, ISSUE_TYPE, "argument");
    assert_eq!(err.span, CALL_SPAN());
    assert_eq!(err.definition_span, Some(DEF_SPAN()));
    assert!(err.args.iter().any(|arg| arg.name == "expected"));
    assert!(err.args.iter().any(|arg| arg.name == "actual"));
}

#[test]
fn argument_mismatch_is_scoped_to_caller_function() {
    let mut types = TypeTable::new();
    let f32 = f32_ty(&mut types);
    let u32 = crate::mir::test_support::u32_type(&mut types);
    let helper = function(
        1,
        FRAGMENT,
        vec![param(0, f32, MirParamMode::Owned)],
        f32,
        None,
        Vec::new(),
        None,
        None,
    );
    let entry = function(
        0,
        ENTRY,
        vec![param(0, u32, MirParamMode::Owned)],
        f32,
        Some(MirCallee::Function(MirFunctionId(1))),
        vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
        Some(MirPlace::local(MirLocalId(0))),
        Some(MirKernelShaderStage::Compute),
    );
    // Callee first: a program-wide local-id scan would see helper's f32 and miss the mismatch.
    expect_issue(&check_fns(&[helper, entry], &types), ISSUE_TYPE, "argument");
}

#[test]
fn shape_mismatch_survives_untyped_prior_operand() {
    let mut types = TypeTable::new();
    let f32 = f32_ty(&mut types);
    let element = types.sized_numeric(crate::semantic::Primitive::Fractus, crate::semantic::NumericWidth::F32);
    let shape4 = types.intern_index(crate::semantic::IndexExpr::Literal(4));
    let shape8 = types.intern_index(crate::semantic::IndexExpr::Literal(8));
    let t4 = MirType::semantic(types.tensor_with_shape(element, shape4));
    let t8 = MirType::semantic(types.tensor_with_shape(element, shape8));
    let helper = function(
        1,
        FRAGMENT,
        vec![param(0, f32, MirParamMode::Owned), param(1, t4, MirParamMode::Owned)],
        t4,
        None,
        Vec::new(),
        None,
        None,
    );
    let entry = function(
        0,
        ENTRY,
        vec![param(0, t8, MirParamMode::Owned)],
        t4,
        Some(MirCallee::Function(MirFunctionId(1))),
        vec![
            MirOperand::Constant(crate::mir::MirConstant::Float(1.0)),
            MirOperand::Place(MirPlace::local(MirLocalId(0))),
        ],
        Some(MirPlace::local(MirLocalId(0))),
        Some(MirKernelShaderStage::Compute),
    );
    expect_issue(&check_fns(&[entry, helper], &types), ISSUE_TYPE, "shape");
}

#[test]
fn unsolved_generic_bindings_emit_type() {
    let mut types = TypeTable::new();
    let param_ty = MirType::semantic(types.intern(crate::semantic::Type::Param(crate::lexer::Symbol(7))));
    let helper = function(
        1,
        FRAGMENT,
        vec![param(0, param_ty, MirParamMode::Owned)],
        param_ty,
        None,
        Vec::new(),
        None,
        None,
    );
    let entry = function(
        0,
        ENTRY,
        vec![param(0, param_ty, MirParamMode::Owned)],
        param_ty,
        Some(MirCallee::Function(MirFunctionId(1))),
        vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
        Some(MirPlace::local(MirLocalId(0))),
        Some(MirKernelShaderStage::Compute),
    );
    expect_issue(&check_fns(&[entry, helper], &types), ISSUE_TYPE, "bindings");
}

#[test]
fn argument_mode_mismatch_emits_type() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let helper = function(
        1,
        FRAGMENT,
        vec![param(0, ty, MirParamMode::MutRef)],
        ty,
        None,
        Vec::new(),
        None,
        None,
    );
    let mut entry = function(
        0,
        ENTRY,
        vec![param(0, ty, MirParamMode::Owned)],
        ty,
        Some(MirCallee::Function(MirFunctionId(1))),
        vec![MirOperand::Constant(crate::mir::MirConstant::Float(1.0))],
        Some(MirPlace::local(MirLocalId(0))),
        Some(MirKernelShaderStage::Compute),
    );
    entry.locals = vec![MirLocal { id: MirLocalId(1), name: None, ty, mutable: false, span: test_span() }];
    expect_issue(&check_fns(&[entry, helper], &types), ISSUE_TYPE, "mode");
}

#[test]
fn return_mismatch_emits_type() {
    let mut types = TypeTable::new();
    let f32 = f32_ty(&mut types);
    let u32 = crate::mir::test_support::u32_type(&mut types);
    let helper = function(
        1,
        FRAGMENT,
        vec![param(0, f32, MirParamMode::Owned)],
        f32,
        None,
        Vec::new(),
        None,
        None,
    );
    let mut entry = function(
        0,
        ENTRY,
        vec![param(0, f32, MirParamMode::Owned)],
        u32,
        Some(MirCallee::Function(MirFunctionId(1))),
        vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
        Some(MirPlace::local(MirLocalId(1))),
        Some(MirKernelShaderStage::Compute),
    );
    entry.locals = vec![MirLocal { id: MirLocalId(1), name: None, ty: u32, mutable: false, span: test_span() }];
    expect_issue(&check_fns(&[entry, helper], &types), ISSUE_TYPE, "return");
}

#[test]
fn host_type_specialization_is_target_illegal() {
    let types = TypeTable::new();
    let text = text_ty(&types);
    let helper = function(
        1,
        FRAGMENT,
        vec![param(0, text, MirParamMode::Owned)],
        text,
        None,
        Vec::new(),
        None,
        None,
    );
    let entry = function(
        0,
        ENTRY,
        vec![param(0, text, MirParamMode::Owned)],
        text,
        Some(MirCallee::Function(MirFunctionId(1))),
        vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
        Some(MirPlace::local(MirLocalId(0))),
        Some(MirKernelShaderStage::Compute),
    );
    expect_issue(&check_fns(&[entry, helper], &types), ISSUE_TYPE, "target");
}

#[test]
fn failable_fragment_call_does_not_fit_effect() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let helper = function(
        1,
        FRAGMENT,
        vec![param(0, ty, MirParamMode::Owned)],
        ty,
        None,
        Vec::new(),
        None,
        None,
    );
    let mut entry = function(
        0,
        ENTRY,
        vec![param(0, ty, MirParamMode::Owned)],
        ty,
        None,
        Vec::new(),
        None,
        Some(MirKernelShaderStage::Compute),
    );
    entry.blocks[0].terminator = MirTerminator {
        kind: MirTerminatorKind::TryCall {
            destination: Some(MirPlace::local(MirLocalId(0))),
            callee: MirCallee::Function(MirFunctionId(1)),
            args: vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
            ok_block: MirBlockId(0),
            error_place: MirPlace::local(MirLocalId(0)),
            error_block: MirBlockId(0),
        },
        span: CALL_SPAN(),
    };
    expect_issue(&check_fns(&[entry, helper], &types), ISSUE_TYPE, "effect");
}

#[test]
fn monomorphized_definition_callee_uses_specialized_instance() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let helper = function(
        2,
        FRAGMENT,
        vec![param(0, ty, MirParamMode::Owned)],
        ty,
        None,
        Vec::new(),
        None,
        None,
    );
    let entry = function(
        0,
        ENTRY,
        vec![param(0, ty, MirParamMode::Owned)],
        ty,
        Some(MirCallee::definition(FRAGMENT)),
        vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
        Some(MirPlace::local(MirLocalId(0))),
        Some(MirKernelShaderStage::Compute),
    );
    let monomorphizations = vec![MirMonomorphization {
        key: MirMonomorphizationKey { source: FRAGMENT, type_args: vec![ty], shape_args: Vec::new() },
        function: MirFunctionId(2),
    }];
    let errors = check(
        &[entry, helper],
        &monomorphizations,
        &MirLayoutTable::from_types(&types),
        &types,
        &roles(),
        None,
    );
    assert!(errors.is_empty(), "specialized definition call should pass: {errors:?}");
}

#[test]
fn lowering_composes_entry_to_entry() {
    // DFV2-4: a device caller may call a concrete local device entry; the
    // call composes instead of the v1 nested-dispatch reject.
    let source = r"
@ nucleum
functio inner(f32 x) → f32 {
    redde x
}

@ nucleum
functio outer(f32 x) → f32 {
    redde inner(x)
}
";
    let session = Session::new(Config::default());
    let mut analysis =
        driver::analyze_source(&session, "device.fab", source, AnalyzeRequest::default()).expect("analyze source");
    let roles = crate::mir::device_roles_from_hir(&analysis.hir);
    let lowered = crate::mir::lower_analyzed_unit(&mut analysis, MirLoweringOptions::default())
        .expect("Entry→Entry must compose");
    assert!(
        crate::mir::fragment_composition::surviving_fragment_calls_with_metadata(
            &lowered.validated.program().functions,
            &[],
            &roles,
            &crate::mir::MirDeviceFunctionMetadataMap::default(),
        )
        .is_empty(),
        "no device call may survive composition: {:#?}",
        lowered.validated.program().functions
    );
}

#[test]
fn legal_entry_to_fragment_reaches_composition() {
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
    let session = Session::new(Config::default());
    let mut analysis =
        driver::analyze_source(&session, "device.fab", source, AnalyzeRequest::default()).expect("analyze source");
    let roles = crate::mir::device_roles_from_hir(&analysis.hir);
    let lowered = crate::mir::lower_analyzed_unit(&mut analysis, MirLoweringOptions::default())
        .expect("legal Entry→Fragment must compose");
    assert!(
        crate::mir::fragment_composition::surviving_fragment_calls_with_metadata(
            &lowered.validated.program().functions,
            &[],
            &roles,
            &crate::mir::MirDeviceFunctionMetadataMap::default(),
        )
        .is_empty(),
        "staging-gate rejection narrowed to composition success"
    );
}

// ---------------------------------------------------------------------------
// KCM-05: synthetic kernel closures classify through the metadata carrier.
// ---------------------------------------------------------------------------

use crate::mir::{MirDeviceFunctionMetadata, MirFunctionOrigin, MirFunctionVisibility, MirSyntheticFunctionIdentity};

fn synthetic_closure(
    id: u32,
    ty: MirType,
    callee: Option<MirCallee>,
    args: Vec<MirOperand>,
    dest: Option<MirPlace>,
) -> MirFunction {
    let statements = if let Some(callee) = callee {
        vec![MirStatement { kind: MirStatementKind::Call { destination: dest, callee, args }, span: CALL_SPAN() }]
    } else {
        Vec::new()
    };
    MirFunction {
        id: MirFunctionId(id),
        source: None,
        name: None,
        params: vec![param(0, ty, MirParamMode::Owned)],
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements,
            terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: test_span() },
            span: DEF_SPAN(),
        }],
        return_ty: ty,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: Some(MirKernelShaderStage::Compute),
        span: DEF_SPAN(),
    }
}

fn synthetic_metadata(function_id: u32) -> MirDeviceFunctionMetadataMap {
    let mut metadata = MirDeviceFunctionMetadataMap::default();
    metadata.insert(
        MirFunctionId(function_id),
        MirDeviceFunctionMetadata {
            identity: MirSyntheticFunctionIdentity { parent: MirFunctionId(0), ordinal: 0 },
            role: MirDeviceRole::Kernel,
            shader_stage: MirKernelShaderStage::Compute,
            visibility: MirFunctionVisibility::Private,
            origin: MirFunctionOrigin::SyntheticKernelClosure,
        },
    );
    metadata
}

fn check_fns_with_metadata(
    functions: &[MirFunction],
    types: &TypeTable,
    metadata: &MirDeviceFunctionMetadataMap,
) -> Vec<FragmentInstantiationError> {
    check_with_metadata(
        functions,
        &[],
        &MirLayoutTable::from_types(types),
        types,
        &roles(),
        None,
        metadata,
    )
}

/// A kernel caller's direct call to a synthetic kernel closure is a legal
/// device call — it composes downstream.
#[test]
fn kernel_caller_to_synthetic_closure_is_a_legal_device_call() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let closure = synthetic_closure(2, ty, None, Vec::new(), None);
    let entry = function(
        0,
        ENTRY,
        vec![param(0, ty, MirParamMode::Owned)],
        ty,
        Some(MirCallee::Function(MirFunctionId(2))),
        vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
        Some(MirPlace::local(MirLocalId(0))),
        Some(MirKernelShaderStage::Compute),
    );
    let metadata = synthetic_metadata(2);
    let errors = check_fns_with_metadata(&[entry, closure], &types, &metadata);
    assert!(errors.is_empty(), "Entry→synthetic closure must pass: {errors:?}");
}

/// A host caller's direct call to a synthetic kernel closure is the launch
/// boundary, not a host-callable violation.
#[test]
fn host_call_to_synthetic_closure_is_a_legal_launch_boundary() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let closure = synthetic_closure(2, ty, None, Vec::new(), None);
    let host = function(
        1,
        HOST,
        vec![param(0, ty, MirParamMode::Owned)],
        ty,
        Some(MirCallee::Function(MirFunctionId(2))),
        vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
        Some(MirPlace::local(MirLocalId(0))),
        None,
    );
    let metadata = synthetic_metadata(2);
    let errors = check_fns_with_metadata(&[host, closure], &types, &metadata);
    assert!(errors.is_empty(), "host→synthetic closure must pass: {errors:?}");
}

/// A synthetic kernel closure calling a named kernel entry is device→device.
#[test]
fn synthetic_closure_to_kernel_entry_is_a_legal_device_call() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let entry = function(
        1,
        ENTRY,
        vec![param(0, ty, MirParamMode::Owned)],
        ty,
        None,
        Vec::new(),
        None,
        Some(MirKernelShaderStage::Compute),
    );
    let closure = synthetic_closure(
        2,
        ty,
        Some(MirCallee::Function(MirFunctionId(1))),
        vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
        Some(MirPlace::local(MirLocalId(0))),
    );
    let metadata = synthetic_metadata(2);
    let errors = check_fns_with_metadata(&[closure, entry], &types, &metadata);
    assert!(errors.is_empty(), "synthetic→Entry must pass: {errors:?}");
}

/// Without the carrier a source-less compute-stage function has no proven
/// device identity: a kernel caller's call rejects host-callable (fail-closed
/// selection — never an entry without explicit facts).
#[test]
fn kernel_caller_to_uncarried_compute_rejects_host_callable() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let uncarried = synthetic_closure(2, ty, None, Vec::new(), None);
    let entry = function(
        0,
        ENTRY,
        vec![param(0, ty, MirParamMode::Owned)],
        ty,
        Some(MirCallee::Function(MirFunctionId(2))),
        vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
        Some(MirPlace::local(MirLocalId(0))),
        Some(MirKernelShaderStage::Compute),
    );
    let errors = check_fns_with_metadata(&[entry, uncarried], &types, &MirDeviceFunctionMetadataMap::default());
    expect_issue(&errors, ISSUE_CONTEXT, "host_callable");
}
