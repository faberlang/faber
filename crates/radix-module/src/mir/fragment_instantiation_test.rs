//! Coverage for nucleum-fragment instantiation checking and the call-legality matrix.

use super::{check, FragmentInstantiationError, ISSUE_CONTEXT, ISSUE_TYPE};
use crate::diagnostics::{semantic_message, DiagnosticArg};
use crate::driver::{self, Config, Session};
use crate::hir::DefId;
use crate::lexer::Span;
use crate::mir::{
    MirBlock, MirBlockId, MirCallee, MirDeviceRole, MirFunction, MirFunctionId, MirKernelShaderStage, MirLayoutTable,
    MirLocal, MirLocalId, MirMonomorphization, MirMonomorphizationKey, MirOperand, MirParam, MirParamMode, MirPlace,
    MirStatement, MirStatementKind, MirTerminator, MirTerminatorKind, MirType,
};
use crate::semantic::{SemanticErrorKind, TypeTable};
use crate::test_support::{test_span, test_span_between};
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
fn entry_to_entry_is_nested_dispatch() {
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
    let err = expect_issue(&errors, ISSUE_CONTEXT, "nested_dispatch");
    assert_eq!(err.span, CALL_SPAN());
    assert_eq!(err.definition_span, Some(DEF_SPAN()));
    assert!(err
        .args
        .iter()
        .any(|arg| arg.name == "caller" && arg.value == "entry"));
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
    assert!(err
        .args
        .iter()
        .any(|arg| arg.name == "caller" && arg.value == "host"));
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
fn catalog_emits_type_and_call_site_context() {
    let type_args = vec![
        DiagnosticArg::new("issue", ISSUE_TYPE),
        DiagnosticArg::new("reason", "argument"),
        DiagnosticArg::new("expected", "f32"),
        DiagnosticArg::new("actual", "u32"),
    ];
    assert_eq!(
        semantic_message(SemanticErrorKind::InvalidOperandTypes, &type_args).as_deref(),
        Some("@ nucleum fragment argument has type u32, expected f32")
    );

    let context_args = vec![
        DiagnosticArg::new("issue", ISSUE_CONTEXT),
        DiagnosticArg::new("reason", "nested_dispatch"),
        DiagnosticArg::new("caller", "entry"),
        DiagnosticArg::new("site", "call"),
    ];
    assert_eq!(
        semantic_message(SemanticErrorKind::InvalidOperandTypes, &context_args).as_deref(),
        Some("@ nucleum cannot call another @ nucleum entry")
    );
}

#[test]
fn lowering_rejects_entry_to_entry_nested_dispatch() {
    let source = r"
@ nucleum
functio inner(fractus<f32> x) → fractus<f32> {
    redde x
}

@ nucleum
functio outer(fractus<f32> x) → fractus<f32> {
    redde inner(x)
}
";
    let session = Session::new(Config::default());
    let mut analysis = driver::analyze_source(&session, "device.fab", source).expect("analyze source");
    let errors = crate::mir::lower_analyzed_unit(&mut analysis).expect_err("Entry→Entry must be rejected");
    assert!(
        errors.iter().any(|err| err.issue == ISSUE_CONTEXT),
        "expected NUCLEUM_FRAGMENT_CONTEXT, got {errors:?}"
    );
}

#[test]
fn legal_entry_to_fragment_reaches_composition() {
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
    let mut analysis = driver::analyze_source(&session, "device.fab", source).expect("analyze source");
    let lowered = crate::mir::lower_analyzed_unit(&mut analysis).expect("legal Entry→Fragment must compose");
    let roles = crate::mir::device_roles_from_hir(&analysis.hir);
    assert!(
        crate::mir::fragment_composition::surviving_fragment_calls(&lowered.functions, &[], &roles).is_empty(),
        "staging-gate rejection narrowed to composition success"
    );
}
