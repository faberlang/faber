//! Device safety classification: MIR shapes accepted or rejected for GPU backends.
//!
//! Tests: classify_device_safe and device_roles_from_hir for scalar, tensor, atomic,
//! control-flow, and annotation-based kernel shapes across GPU targets.

use super::test_support::{atomic_i32_type, f32_type, indexed_local_place, rank1_tensor_type};
use super::{
    MirDeviceContext, MirDeviceRole, classify_device_safe, classify_device_safe_for_target_entry, device_roles_from_hir,
};
use crate::driver::{self, AnalyzeRequest, Config, Session};
use crate::hir::{DefId, HirFunction};
use crate::mir::*;
use crate::semantic::{IndexExpr, NumericWidth, Primitive, Type, TypeTable};
use crate::test_support::test_span;
use handroll::FxHashMap;

fn atomic_u32_type(types: &mut TypeTable) -> MirType {
    let element = types.sized_numeric(Primitive::Numerus, NumericWidth::U32);
    MirType::semantic(types.atomic(element))
}

fn text_type(types: &TypeTable) -> MirType {
    MirType::semantic(types.primitive(Primitive::Textus))
}

fn kernel_context(program: &MirProgram) -> MirDeviceContext {
    let mut roles = FxHashMap::default();
    roles.insert(DefId(1), MirDeviceRole::Kernel);
    let mut context = MirDeviceContext::from_hir_roles(roles);
    context.attach_program(program);
    context
}

fn kernel_program(
    types: &mut TypeTable,
    return_ty: MirType,
    statements: Vec<MirStatement>,
    terminator: MirTerminatorKind,
    error_ty: Option<MirType>,
) -> MirProgram {
    let param_ty = f32_type(types);
    MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: Some(DefId(1)),
            name: None,
            params: vec![MirParam {
                local: MirLocalId(0),
                name: None,
                ty: param_ty,
                mode: MirParamMode::Owned,
                span: test_span(),
            }],
            locals: Vec::new(),
            temps: Vec::new(),
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements,
                terminator: MirTerminator { kind: terminator, span: test_span() },
                span: test_span(),
            }],
            return_ty,
            error_ty,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: test_span(),
        }],
    }
}

/// Like [`kernel_program`] but accepts extra local variables alongside the
/// single f32 parameter. Use when the function must carry additional typed
/// slots (e.g. text or aggregate locals) that `classify_device_safe` checks
/// but which would fail MIR validation if placed in the return/param position.
fn kernel_program_with_locals(
    types: &mut TypeTable,
    return_ty: MirType,
    statements: Vec<MirStatement>,
    terminator: MirTerminatorKind,
    error_ty: Option<MirType>,
    locals: Vec<MirLocal>,
) -> MirProgram {
    let mut program = kernel_program(types, return_ty, statements, terminator, error_ty);
    program.functions[0].locals = locals;
    program
}

#[path = "device_test/goto_builders.rs"]
mod goto_builders;
use goto_builders::*;

fn classify(program: &MirProgram, types: &TypeTable) -> radix_types::Lowerability {
    let mut validation = MirValidationContext::new(types);
    attach_device_tensor_layouts(&mut validation);
    let context = kernel_context(program);
    let validated = ValidatedMir::new(program.clone(), validation).expect("valid test MIR");
    classify_device_safe(&validated, &context)
}

fn classify_host_owned(program: &MirProgram, types: &TypeTable) -> radix_types::Lowerability {
    let validation = MirValidationContext::new(types);
    let context = kernel_context(program);
    let validated = ValidatedMir::new(program.clone(), validation).expect("valid test MIR");
    classify_device_safe(&validated, &context)
}

fn assert_device_validation_rejects(program: &MirProgram, types: &TypeTable, issue: &str) {
    let mut validation = MirValidationContext::new(types);
    attach_device_tensor_layouts(&mut validation);
    let Err(errors) = ValidatedMir::new(program.clone(), validation) else {
        panic!("expected MIR validation rejection");
    };
    assert!(
        errors.iter().any(|error| error.issue == issue),
        "expected validation issue {issue}, got {errors:?}"
    );
}

fn attach_device_tensor_layouts(validation: &mut MirValidationContext<'_>) {
    let mut device_layouts = Vec::new();
    for index in 0..validation.types.type_count() {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "TypeId is u32 by design; type_count is bounded by u32::MAX"
        )]
        let ty = crate::semantic::TypeId(index as u32);
        let Ok(layout) = validation.layouts.layout_for_type(MirType::semantic(ty)) else {
            continue;
        };
        let MirLayoutKind::DeviceView(view) = layout.kind else {
            continue;
        };
        device_layouts.push((ty, view.element, view.rank));
    }
    for (ty, element, rank) in device_layouts {
        let mut handle = MirDeviceViewLayout::device_handle_row_major(MirScalarLayout::F32, rank);
        handle.element = element;
        validation
            .layouts
            .set_type_layout(ty, MirLayoutKind::DeviceView(handle));
    }
}

fn assert_rejected_goto_policy(result: radix_types::Lowerability, message: &str) {
    let radix_types::Lowerability::Rejected(gaps) = result else {
        panic!("{message}");
    };
    assert!(
        gaps.iter()
            .any(|gap| gap.slug() == "target-policy-rejection" && gap.shape() == "device multi-block control flow")
    );
    assert!(
        gaps.iter()
            .any(|gap| gap.slug() == "target-policy-rejection" && gap.shape() == "device goto terminator")
    );
}

fn classify_source(source: &str) -> radix_types::Lowerability {
    let session = Session::new(Config::default().with_latin_locale());
    let analysis =
        driver::analyze_source(&session, "device.fab", source, AnalyzeRequest::default()).expect("analyze source");
    let roles = device_roles_from_hir(&analysis.hir);
    let lowered = crate::mir::lower_analyzed_unit(analysis.application_view(), MirLoweringOptions::default())
        .expect("lower source to MIR");
    let mut validation = lowered.validated.validation().clone();
    attach_device_tensor_layouts(&mut validation);
    let mut context = MirDeviceContext::from_hir_roles(roles);
    context.attach_program(lowered.validated.program());
    let validated = ValidatedMir::new(lowered.validated.program().clone(), validation).expect("valid test MIR");
    classify_device_safe(&validated, &context)
}

#[test]
fn device_roles_from_hir_includes_genus_method_nucleum_annotation() {
    let source = r"
genus Worker {
    @ nucleum
    functio run(f32 x) → f32 {
        redde x
    }
}
";
    let session = Session::new(Config::default().with_latin_locale());
    let analysis =
        driver::analyze_source(&session, "device.fab", source, AnalyzeRequest::default()).expect("analyze source");

    let roles = device_roles_from_hir(&analysis.hir);
    let method_def = analysis
        .hir
        .items
        .iter()
        .find_map(|item| match &item.kind {
            crate::hir::HirItemKind::Struct(strukt) => strukt.methods.first().map(|method| method.def_id),
            _ => None,
        })
        .expect("method def id");

    assert_eq!(
        roles.get(&method_def),
        Some(&MirDeviceRole::Kernel),
        "genus method @ nucleum must map to kernel role by method def_id"
    );
}

#[test]
fn device_safe_accepts_scalar_f32_kernel() {
    let mut types = TypeTable::new();
    let f32_ty = f32_type(&mut types);
    let program = kernel_program(
        &mut types,
        f32_ty,
        Vec::new(),
        MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(0))))),
        None,
    );

    assert_eq!(classify(&program, &types), radix_types::Lowerability::Capable);
}

#[test]
fn device_safe_rejects_atomic_value_types_explicitly() {
    let mut types = TypeTable::new();
    let f32_ty = f32_type(&mut types);
    let atomic_ty = atomic_i32_type(&mut types);
    let program = kernel_program_with_locals(
        &mut types,
        f32_ty,
        Vec::new(),
        MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(0))))),
        None,
        vec![MirLocal { id: MirLocalId(1), name: None, ty: atomic_ty, mutable: false, span: test_span() }],
    );

    let radix_types::Lowerability::Rejected(gaps) = classify(&program, &types) else {
        panic!("atomic device value type must reject");
    };
    assert_eq!(gaps[0].slug(), "target-policy-rejection");
    assert_eq!(gaps[0].shape(), "kernel atomic value type i32");
}

#[test]
fn device_safe_rejects_atomic_u32_value_type() {
    let mut types = TypeTable::new();
    let f32_ty = f32_type(&mut types);
    let atomic_ty = atomic_u32_type(&mut types);
    let program = kernel_program_with_locals(
        &mut types,
        f32_ty,
        Vec::new(),
        MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(0))))),
        None,
        vec![MirLocal { id: MirLocalId(1), name: None, ty: atomic_ty, mutable: false, span: test_span() }],
    );

    let radix_types::Lowerability::Rejected(gaps) = classify(&program, &types) else {
        panic!("atomic u32 device value type must reject");
    };
    assert_eq!(gaps[0].slug(), "target-policy-rejection");
    assert_eq!(gaps[0].shape(), "kernel atomic value type u32");
}

#[test]
fn device_safe_rejects_atomic_runtime_calls_explicitly() {
    let mut types = TypeTable::new();
    let f32_ty = f32_type(&mut types);
    let atomic_ty = atomic_i32_type(&mut types);
    let i32_ty = MirType::semantic(types.sized_numeric(Primitive::Numerus, NumericWidth::I32));
    let program = kernel_program_with_locals(
        &mut types,
        f32_ty,
        vec![MirStatement {
            kind: MirStatementKind::RuntimeCall {
                destination: Some(MirPlace::local(MirLocalId(2))),
                call: MirRuntimeCall {
                    intrinsic: MirIntrinsic::Atomic(MirAtomicOp::Load),
                    args: vec![MirOperand::Place(MirPlace::local(MirLocalId(1)))],
                    return_ty: i32_ty,
                },
            },
            span: test_span(),
        }],
        MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(0))))),
        None,
        vec![
            MirLocal { id: MirLocalId(1), name: None, ty: atomic_ty, mutable: false, span: test_span() },
            MirLocal { id: MirLocalId(2), name: None, ty: i32_ty, mutable: true, span: test_span() },
        ],
    );

    let radix_types::Lowerability::Rejected(gaps) = classify(&program, &types) else {
        panic!("atomic runtime call must reject");
    };
    assert!(
        gaps.iter()
            .any(|gap| gap.slug() == "target-policy-rejection" && gap.shape() == "kernel atomic runtime call load")
    );
}

#[test]
fn device_safe_rejects_runtime_calls_with_policy_gap() {
    let mut types = TypeTable::new();
    let f32_ty = f32_type(&mut types);
    let vacuum_ty = MirType::semantic(types.primitive(Primitive::Vacuum));
    let program = kernel_program(
        &mut types,
        f32_ty,
        vec![MirStatement {
            kind: MirStatementKind::RuntimeCall {
                destination: None,
                call: MirRuntimeCall {
                    intrinsic: MirIntrinsic::Diagnostic(MirDiagnosticKind::Scribe),
                    args: vec![MirOperand::Constant(MirConstant::String(crate::lexer::Symbol(0)))],
                    return_ty: vacuum_ty,
                },
            },
            span: test_span(),
        }],
        MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(0))))),
        None,
    );

    let radix_types::Lowerability::Rejected(gaps) = classify(&program, &types) else {
        panic!("runtime call must reject");
    };
    assert_eq!(gaps[0].slug(), "target-policy-rejection");
    assert_eq!(gaps[0].shape(), "kernel runtime call");
}

#[test]
fn device_safe_rejects_tensor_slice_view_without_materialization() {
    let result = classify_source(
        r"@ nucleum
functio sectum(tf32[4] input) → vacuum {
    sit tf32[1] view ← input.sectio(0, 1)
}
",
    );

    let radix_types::Lowerability::Rejected(gaps) = result else {
        panic!("tensor slice view operation must reject for device targets");
    };
    assert!(gaps.iter().any(|gap| {
        gap.slug() == "target-policy-rejection"
            && gap.shape() == "kernel tensor view operation requires explicit materialize"
    }));
}

#[test]
fn device_safe_accepts_tensor_relu_runtime_call() {
    // D-W6-B1 U2: `activatio_relu` (registry.rs:476-481) lowers via
    // collection_method_op to `MirIntrinsic::Collection(MirCollectionOp::
    // TensorRelu)` — a device-safe elementwise op (max(0.0, x) per element),
    // mirroring the TensorAdd classifier arm. Pre-U2 the wildcard recorded
    // "kernel runtime call" (device.rs wildcard arm); the rung-2 ReLU kernel
    // now stages past device IR staging. (The source-path equivalent lives in
    // wgsl_text_test.rs `wgsl_text_tensor_relu_chain_stages_through_emit_chain_descriptor`.)
    let mut types = TypeTable::new();
    let f32_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let len = types.intern_index(IndexExpr::Literal(4));
    let tensor_ty = MirType::semantic(types.intern(Type::Tensor(f32_ty, len)));
    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: Some(DefId(1)),
            name: None,
            params: vec![MirParam {
                local: MirLocalId(0),
                name: None,
                ty: tensor_ty,
                mode: MirParamMode::Owned,
                span: test_span(),
            }],
            locals: vec![MirLocal { id: MirLocalId(1), name: None, ty: tensor_ty, mutable: true, span: test_span() }],
            temps: Vec::new(),
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::RuntimeCall {
                        destination: Some(MirPlace::local(MirLocalId(1))),
                        call: MirRuntimeCall {
                            intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorRelu),
                            args: vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
                            return_ty: tensor_ty,
                        },
                    },
                    span: test_span(),
                }],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(0))))),
                    span: test_span(),
                },
                span: test_span(),
            }],
            return_ty: tensor_ty,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: test_span(),
        }],
    };

    assert_eq!(classify(&program, &types), radix_types::Lowerability::Capable);
}

#[test]
fn device_safe_accepts_tensor_transpose_runtime_call() {
    // S5-U2: `TensorTranspose` is a recipe op — the plan pass admits it
    // (`CollectionKernelPlan::Transpose`, kernel_plan.rs), the ABI resolves
    // its dispatch contract (workgroup 64, dispatch M·N), and the Metal
    // emitter lowers the column-major remap body. The device-safe gate must
    // admit the statement the same way it admits the other recipe ops
    // (Sum/Mean, MatMul); pre-admission the wildcard recorded "kernel
    // runtime call" and rejected every transpose-bearing kernel at the
    // target-entry probe.
    let mut types = TypeTable::new();
    let f32_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let dim = types.intern_index(IndexExpr::Literal(4));
    let shape = types.intern_index(IndexExpr::Tuple(vec![dim, dim]));
    let tensor_ty = MirType::semantic(types.intern(Type::Tensor(f32_ty, shape)));
    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: Some(DefId(1)),
            name: None,
            params: vec![MirParam {
                local: MirLocalId(0),
                name: None,
                ty: tensor_ty,
                mode: MirParamMode::Owned,
                span: test_span(),
            }],
            locals: vec![MirLocal { id: MirLocalId(1), name: None, ty: tensor_ty, mutable: true, span: test_span() }],
            temps: Vec::new(),
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::RuntimeCall {
                        destination: Some(MirPlace::local(MirLocalId(1))),
                        call: MirRuntimeCall {
                            intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorTranspose),
                            args: vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
                            return_ty: tensor_ty,
                        },
                    },
                    span: test_span(),
                }],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(0))))),
                    span: test_span(),
                },
                span: test_span(),
            }],
            return_ty: tensor_ty,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: test_span(),
        }],
    };

    // The target-entry probe path (the Metal emitter's `emit_metal_text_
    // probe_with_reflection_and_device_roles`) classifies with
    // host-owned tensor entry allowed — the same conditions under which
    // the recipe-op and elementwise kernels stage past device IR staging.
    let context = kernel_context(&program);
    let validated = ValidatedMir::new(program, MirValidationContext::new(&types)).expect("valid test MIR");
    assert_eq!(
        classify_device_safe_for_target_entry(&validated, &context),
        radix_types::Lowerability::Capable
    );
}

#[test]
fn device_safe_rejects_product_linked_device_call() {
    let mut types = TypeTable::new();
    let f32_ty = f32_type(&mut types);
    let program = kernel_program_with_locals(
        &mut types,
        f32_ty,
        vec![MirStatement {
            kind: MirStatementKind::Call {
                destination: Some(MirPlace::local(MirLocalId(1))),
                callee: MirCallee::definition(DefId(99)),
                args: vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
            },
            span: test_span(),
        }],
        MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(0))))),
        None,
        vec![MirLocal { id: MirLocalId(1), name: None, ty: f32_ty, mutable: true, span: test_span() }],
    );
    let mut validation = MirValidationContext::new(&types);
    validation.external_function_identities.insert(
        DefId(99),
        MirExternalFunctionIdentity::new("norma", vec!["math".to_owned()], "remote_sum"),
    );
    validation.functions.insert(
        DefId(99),
        MirFunctionSignature {
            params: vec![MirParamSignature { ty: f32_ty, optional: false }],
            return_ty: f32_ty,
            error_ty: None,
        },
    );

    let context = kernel_context(&program);
    let validated = ValidatedMir::new(program, validation).expect("valid test MIR");
    let radix_types::Lowerability::Rejected(gaps) = classify_device_safe(&validated, &context) else {
        panic!("product-linked device call must reject");
    };
    assert!(
        gaps.iter()
            .any(|gap| gap.slug() == "target-policy-rejection" && gap.shape() == "device function call")
    );
}

#[test]
fn device_safe_rejects_failable_control_with_abi_gap() {
    let mut types = TypeTable::new();
    let f32_ty = f32_type(&mut types);
    let text_ty = text_type(&types);
    let vacuum_ty = MirType::semantic(types.primitive(Primitive::Vacuum));
    let program = MirProgram {
        functions: vec![
            MirFunction {
                id: MirFunctionId(0),
                source: Some(DefId(1)),
                name: None,
                params: vec![MirParam {
                    local: MirLocalId(0),
                    name: None,
                    ty: f32_ty,
                    mode: MirParamMode::Owned,
                    span: test_span(),
                }],
                locals: vec![MirLocal { id: MirLocalId(1), name: None, ty: text_ty, mutable: true, span: test_span() }],
                temps: Vec::new(),
                blocks: vec![
                    MirBlock {
                        id: MirBlockId(0),
                        statements: Vec::new(),
                        terminator: MirTerminator {
                            kind: MirTerminatorKind::TryCall {
                                destination: None,
                                callee: MirCallee::Function(MirFunctionId(1)),
                                args: Vec::new(),
                                ok_block: MirBlockId(1),
                                error_place: MirPlace::local(MirLocalId(1)),
                                error_block: MirBlockId(2),
                            },
                            span: test_span(),
                        },
                        span: test_span(),
                    },
                    MirBlock {
                        id: MirBlockId(1),
                        statements: Vec::new(),
                        terminator: MirTerminator {
                            kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(0))))),
                            span: test_span(),
                        },
                        span: test_span(),
                    },
                    MirBlock {
                        id: MirBlockId(2),
                        statements: Vec::new(),
                        terminator: MirTerminator {
                            kind: MirTerminatorKind::ReturnError(MirOperand::Place(MirPlace::local(MirLocalId(1)))),
                            span: test_span(),
                        },
                        span: test_span(),
                    },
                ],
                return_ty: f32_ty,
                error_ty: Some(text_ty),
                is_async: false,
                is_generator: false,
                shader_stage: None,
                span: test_span(),
            },
            MirFunction {
                id: MirFunctionId(1),
                source: None,
                name: None,
                params: Vec::new(),
                locals: Vec::new(),
                temps: Vec::new(),
                blocks: vec![MirBlock {
                    id: MirBlockId(0),
                    statements: Vec::new(),
                    terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: test_span() },
                    span: test_span(),
                }],
                return_ty: vacuum_ty,
                error_ty: Some(text_ty),
                is_async: false,
                is_generator: false,
                shader_stage: None,
                span: test_span(),
            },
        ],
    };

    let radix_types::Lowerability::Rejected(gaps) = classify(&program, &types) else {
        panic!("try_call must reject");
    };
    assert!(
        gaps.iter()
            .any(|gap| gap.slug() == "missing-abi-fact" && gap.shape() == "failable device function")
    );
    assert!(
        gaps.iter()
            .any(|gap| gap.slug() == "missing-abi-fact" && gap.shape() == "try_call")
    );
}

#[test]
fn gpu_control_device_safe_rejects_indexed_write_goto_with_extra_target_statement() {
    let mut types = TypeTable::new();
    let program = indexed_write_goto_program(&mut types, 0, false, true);

    assert_rejected_goto_policy(
        classify(&program, &types),
        "indexed write goto with extra target statement must reject",
    );
}

#[test]
fn gpu_control_device_safe_rejects_indexed_write_goto_chain_with_intermediate_statement() {
    let mut types = TypeTable::new();
    let program = indexed_write_goto_program(&mut types, 1, true, false);

    assert_rejected_goto_policy(
        classify(&program, &types),
        "indexed write goto chain with intermediate statement must reject",
    );
}

#[test]
fn gpu_control_device_safe_rejects_indexed_write_goto_chain_with_extra_target_statement() {
    let mut types = TypeTable::new();
    let program = indexed_write_goto_program(&mut types, 1, false, true);

    assert_rejected_goto_policy(
        classify(&program, &types),
        "indexed write goto chain with extra target statement must reject",
    );
}

#[test]
fn gpu_control_device_safe_rejects_scalar_return_goto_with_target_statement() {
    let mut types = TypeTable::new();
    let program = scalar_return_goto_program(&mut types, 0, false, true);

    assert_rejected_goto_policy(
        classify(&program, &types),
        "target statements must keep scalar return goto rejected",
    );
}

#[test]
fn gpu_control_device_safe_rejects_scalar_return_goto_chain_with_intermediate_statement() {
    let mut types = TypeTable::new();
    let program = scalar_return_goto_program(&mut types, 1, true, false);

    assert_rejected_goto_policy(
        classify(&program, &types),
        "scalar return goto chain with intermediate statement must reject",
    );
}

#[test]
fn gpu_control_device_safe_rejects_scalar_return_goto_chain_with_target_statement() {
    let mut types = TypeTable::new();
    let program = scalar_return_goto_program(&mut types, 1, false, true);

    assert_rejected_goto_policy(
        classify(&program, &types),
        "scalar return goto chain with target statement must reject",
    );
}

#[test]
fn gpu_control_device_safe_rejects_scalar_return_goto_cycle() {
    let mut types = TypeTable::new();
    let program = scalar_return_goto_cycle_program(&mut types);

    assert_device_validation_rejects(
        &program,
        &types,
        "mir_validation_block_bb0_does_not_reach_a_terminator_return_returnerror_unreachable_on_every_path",
    );
}

#[test]
fn gpu_control_device_safe_rejects_side_effecting_nested_indexed_loop_branch() {
    let source = r"
@ nucleum
functio fill(tf32[4] input, varia tf32[4] output, u32 id) → vacuum {
    varia u32 count ← 0
    varia u32 side ← 0
    dum count ≺ 2 {
        si count ≻ 0 {
            side ↑
            count ↑
        }
        output[id] ← input[id]
        count ↑
    }
}
";

    assert!(matches!(classify_source(source), radix_types::Lowerability::Rejected(_)));
}

#[test]
fn gpu_control_device_safe_rejects_side_effecting_nested_indexed_loop_control() {
    let source = r"
@ nucleum
functio fill(tf32[4] input, varia tf32[4] output, u32 id) → vacuum {
    varia u32 count ← 0
    varia u32 side ← 0
    dum count ≺ 3 {
        si count ≻ 0 {
            si count ≻ 1 {
                rumpe
            }
            side ↑
            count ↑
        }
        output[id] ← input[id]
        count ↑
    }
}
";

    assert!(matches!(classify_source(source), radix_types::Lowerability::Rejected(_)));
}

#[test]
fn gpu_control_device_safe_rejects_side_effecting_loop_entry_goto() {
    let mut types = TypeTable::new();
    let program = indexed_loop_entry_goto_program(&mut types, true, false);
    let radix_types::Lowerability::Rejected(gaps) = classify(&program, &types) else {
        panic!("expected side-effecting transparent loop-entry goto to reject");
    };

    assert!(
        gaps.iter()
            .any(|gap| gap.shape() == "device multi-block control flow")
    );
}

#[test]
fn gpu_control_device_safe_rejects_cyclic_loop_entry_goto() {
    let mut types = TypeTable::new();
    let program = scalar_loop_entry_goto_program(&mut types, false, true);

    assert_device_validation_rejects(
        &program,
        &types,
        "mir_validation_block_bb0_does_not_reach_a_terminator_return_returnerror_unreachable_on_every_path",
    );
}

#[test]
fn gpu_control_device_safe_rejects_side_effecting_loop_control_entry_goto() {
    let mut types = TypeTable::new();
    let program = indexed_loop_control_entry_goto_program(&mut types, true, true, false);
    let radix_types::Lowerability::Rejected(gaps) = classify(&program, &types) else {
        panic!("expected side-effecting transparent loop-control entry goto to reject");
    };

    assert!(
        gaps.iter()
            .any(|gap| gap.shape() == "device multi-block control flow")
    );
}

#[test]
fn gpu_control_device_safe_rejects_cyclic_loop_control_entry_goto() {
    let mut types = TypeTable::new();
    let program = scalar_loop_control_entry_goto_program(&mut types, false, false, true);

    assert_device_validation_rejects(
        &program,
        &types,
        "mir_validation_block_bb0_does_not_reach_a_terminator_return_returnerror_unreachable_on_every_path",
    );
}

#[test]
fn gpu_control_device_safe_rejects_side_effecting_branch_arm_goto() {
    let mut types = TypeTable::new();
    let program = scalar_return_branch_arm_goto_program(&mut types, true);

    let radix_types::Lowerability::Rejected(gaps) = classify(&program, &types) else {
        panic!("side-effecting branch arm goto must reject");
    };
    assert!(
        gaps.iter()
            .any(|gap| gap.slug() == "target-policy-rejection" && gap.shape() == "device multi-block control flow")
    );
    assert!(
        gaps.iter()
            .any(|gap| gap.slug() == "target-policy-rejection" && gap.shape() == "device branch terminator")
    );
}

#[test]
fn gpu_control_device_safe_rejects_side_effecting_value_join_branch_arm_goto() {
    let mut types = TypeTable::new();
    let program = scalar_value_join_branch_arm_goto_program(&mut types, true);

    let radix_types::Lowerability::Rejected(gaps) = classify(&program, &types) else {
        panic!("side-effecting value-join branch arm goto must reject");
    };
    assert!(
        gaps.iter()
            .any(|gap| gap.slug() == "target-policy-rejection" && gap.shape() == "device multi-block control flow")
    );
    assert!(
        gaps.iter()
            .any(|gap| gap.slug() == "target-policy-rejection" && gap.shape() == "device branch terminator")
    );
}

#[test]
fn gpu_control_device_safe_rejects_side_effecting_switch_arm_goto() {
    let mut types = TypeTable::new();
    let program = scalar_return_switch_arm_goto_program(&mut types, true);

    let radix_types::Lowerability::Rejected(gaps) = classify(&program, &types) else {
        panic!("side-effecting switch-arm goto must reject");
    };
    assert!(
        gaps.iter()
            .any(|gap| gap.slug() == "target-policy-rejection" && gap.shape() == "device multi-block control flow")
    );
    assert!(
        gaps.iter()
            .any(|gap| gap.slug() == "target-policy-rejection" && gap.shape() == "device switch terminator")
    );
}

#[test]
fn gpu_control_device_safe_rejects_side_effecting_indexed_switch_arm_goto() {
    let mut types = TypeTable::new();
    let program = indexed_switch_arm_goto_program(&mut types, true);

    let radix_types::Lowerability::Rejected(gaps) = classify(&program, &types) else {
        panic!("side-effecting indexed switch-arm goto must reject");
    };
    assert!(
        gaps.iter()
            .any(|gap| gap.slug() == "target-policy-rejection" && gap.shape() == "device multi-block control flow")
    );
    assert!(
        gaps.iter()
            .any(|gap| gap.slug() == "target-policy-rejection" && gap.shape() == "device switch terminator")
    );
}

#[test]
fn gpu_control_device_safe_rejects_side_effecting_indexed_branch_arm_goto() {
    let mut types = TypeTable::new();
    let program = indexed_branch_arm_goto_program(&mut types, true);

    let radix_types::Lowerability::Rejected(gaps) = classify(&program, &types) else {
        panic!("side-effecting indexed branch-arm goto must reject");
    };
    assert!(
        gaps.iter()
            .any(|gap| gap.slug() == "target-policy-rejection" && gap.shape() == "device multi-block control flow")
    );
    assert!(
        gaps.iter()
            .any(|gap| gap.slug() == "target-policy-rejection" && gap.shape() == "device branch terminator")
    );
}

#[test]
fn gpu_control_device_safe_rejects_side_effecting_indexed_value_join_branch_arm_goto() {
    let mut types = TypeTable::new();
    let program = indexed_value_join_branch_arm_goto_program(&mut types, true);

    let radix_types::Lowerability::Rejected(gaps) = classify(&program, &types) else {
        panic!("side-effecting indexed value-join branch-arm goto must reject");
    };
    assert!(
        gaps.iter()
            .any(|gap| gap.slug() == "target-policy-rejection" && gap.shape() == "device multi-block control flow")
    );
    assert!(
        gaps.iter()
            .any(|gap| gap.slug() == "target-policy-rejection" && gap.shape() == "device branch terminator")
    );
}

#[test]
fn gpu_control_device_safe_rejects_transparent_branch_arm_goto_with_stray_block() {
    let mut types = TypeTable::new();
    let mut program = scalar_return_branch_arm_goto_program(&mut types, false);
    program.functions[0].blocks.push(MirBlock {
        id: MirBlockId(4),
        statements: Vec::new(),
        terminator: MirTerminator {
            kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(0))))),
            span: test_span(),
        },
        span: test_span(),
    });

    let radix_types::Lowerability::Rejected(gaps) = classify(&program, &types) else {
        panic!("stray branch-arm goto block must reject");
    };
    assert!(
        gaps.iter()
            .any(|gap| gap.slug() == "target-policy-rejection" && gap.shape() == "device multi-block control flow")
    );
    assert!(
        gaps.iter()
            .any(|gap| gap.slug() == "target-policy-rejection" && gap.shape() == "device branch terminator")
    );
}

#[test]
fn gpu_control_device_safe_rejects_transparent_value_join_branch_arm_goto_with_stray_block() {
    let mut types = TypeTable::new();
    let mut program = scalar_value_join_branch_arm_goto_program(&mut types, false);
    program.functions[0].blocks.push(MirBlock {
        id: MirBlockId(6),
        statements: Vec::new(),
        terminator: MirTerminator {
            kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(0))))),
            span: test_span(),
        },
        span: test_span(),
    });

    let radix_types::Lowerability::Rejected(gaps) = classify(&program, &types) else {
        panic!("stray value-join branch-arm goto block must reject");
    };
    assert!(
        gaps.iter()
            .any(|gap| gap.slug() == "target-policy-rejection" && gap.shape() == "device multi-block control flow")
    );
    assert!(
        gaps.iter()
            .any(|gap| gap.slug() == "target-policy-rejection" && gap.shape() == "device branch terminator")
    );
}

#[test]
fn gpu_control_device_safe_rejects_transparent_value_join_branch_arm_goto_cycle() {
    let mut types = TypeTable::new();
    let mut program = scalar_value_join_branch_arm_goto_program(&mut types, false);
    program.functions[0].blocks[1].terminator =
        MirTerminator { kind: MirTerminatorKind::Goto(MirBlockId(2)), span: test_span() };
    program.functions[0].blocks[2].terminator =
        MirTerminator { kind: MirTerminatorKind::Goto(MirBlockId(1)), span: test_span() };

    assert_device_validation_rejects(
        &program,
        &types,
        "mir_validation_block_bb0_does_not_reach_a_terminator_return_returnerror_unreachable_on_every_path",
    );
}

#[test]
fn gpu_control_device_safe_rejects_transparent_indexed_value_join_branch_arm_goto_with_stray_block() {
    let mut types = TypeTable::new();
    let mut program = indexed_value_join_branch_arm_goto_program(&mut types, false);
    program.functions[0].blocks.push(MirBlock {
        id: MirBlockId(6),
        statements: Vec::new(),
        terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: test_span() },
        span: test_span(),
    });

    let radix_types::Lowerability::Rejected(gaps) = classify(&program, &types) else {
        panic!("stray indexed value-join branch-arm goto block must reject");
    };
    assert!(
        gaps.iter()
            .any(|gap| gap.slug() == "target-policy-rejection" && gap.shape() == "device multi-block control flow")
    );
    assert!(
        gaps.iter()
            .any(|gap| gap.slug() == "target-policy-rejection" && gap.shape() == "device branch terminator")
    );
}

#[test]
fn gpu_control_device_safe_rejects_transparent_indexed_value_join_branch_arm_goto_cycle() {
    let mut types = TypeTable::new();
    let mut program = indexed_value_join_branch_arm_goto_program(&mut types, false);
    program.functions[0].blocks[1].terminator =
        MirTerminator { kind: MirTerminatorKind::Goto(MirBlockId(2)), span: test_span() };
    program.functions[0].blocks[2].terminator =
        MirTerminator { kind: MirTerminatorKind::Goto(MirBlockId(1)), span: test_span() };

    assert_device_validation_rejects(
        &program,
        &types,
        "mir_validation_block_bb0_does_not_reach_a_terminator_return_returnerror_unreachable_on_every_path",
    );
}

#[test]
fn gpu_control_device_safe_rejects_transparent_indexed_branch_arm_goto_with_stray_block() {
    let mut types = TypeTable::new();
    let mut program = indexed_branch_arm_goto_program(&mut types, false);
    program.functions[0].blocks.push(MirBlock {
        id: MirBlockId(5),
        statements: Vec::new(),
        terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: test_span() },
        span: test_span(),
    });

    let radix_types::Lowerability::Rejected(gaps) = classify(&program, &types) else {
        panic!("stray indexed branch-arm goto block must reject");
    };
    assert!(
        gaps.iter()
            .any(|gap| gap.slug() == "target-policy-rejection" && gap.shape() == "device multi-block control flow")
    );
    assert!(
        gaps.iter()
            .any(|gap| gap.slug() == "target-policy-rejection" && gap.shape() == "device branch terminator")
    );
}

#[test]
fn gpu_control_device_safe_rejects_transparent_indexed_branch_arm_goto_cycle() {
    let mut types = TypeTable::new();
    let mut program = indexed_branch_arm_goto_program(&mut types, false);
    program.functions[0].blocks[1].terminator =
        MirTerminator { kind: MirTerminatorKind::Goto(MirBlockId(1)), span: test_span() };

    assert_device_validation_rejects(
        &program,
        &types,
        "mir_validation_block_bb1_does_not_reach_a_terminator_return_returnerror_unreachable_on_every_path",
    );
}

#[test]
fn gpu_control_device_safe_rejects_transparent_switch_arm_goto_with_stray_block() {
    let mut types = TypeTable::new();
    let mut program = scalar_return_switch_arm_goto_program(&mut types, false);
    program.functions[0].blocks.push(MirBlock {
        id: MirBlockId(4),
        statements: Vec::new(),
        terminator: MirTerminator {
            kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(0))))),
            span: test_span(),
        },
        span: test_span(),
    });

    let radix_types::Lowerability::Rejected(gaps) = classify(&program, &types) else {
        panic!("stray switch-arm goto block must reject");
    };
    assert!(
        gaps.iter()
            .any(|gap| gap.slug() == "target-policy-rejection" && gap.shape() == "device multi-block control flow")
    );
    assert!(
        gaps.iter()
            .any(|gap| gap.slug() == "target-policy-rejection" && gap.shape() == "device switch terminator")
    );
}

#[test]
fn device_safe_rejects_text_host_value_type() {
    let mut types = TypeTable::new();
    let text_ty = text_type(&types);
    let f32_ty = f32_type(&mut types);
    let program = kernel_program_with_locals(
        &mut types,
        f32_ty,
        Vec::new(),
        MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(0))))),
        None,
        vec![MirLocal { id: MirLocalId(1), name: None, ty: text_ty, mutable: false, span: test_span() }],
    );

    let radix_types::Lowerability::Rejected(gaps) = classify(&program, &types) else {
        panic!("text host value type must reject");
    };
    assert!(
        gaps.iter()
            .any(|gap| gap.shape().starts_with("device host value type"))
    );
}

#[test]
fn device_safe_rejects_array_aggregate_construct() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let array = types.array(numerus);
    let array_ty = MirType::semantic(array);
    let program = kernel_program_with_locals(
        &mut types,
        array_ty,
        vec![MirStatement {
            kind: MirStatementKind::Construct {
                destination: MirPlace::local(MirLocalId(1)),
                aggregate: MirAggregate {
                    kind: MirAggregateKind::Array,
                    ty: array_ty,
                    fields: MirAggregateFields::Ordered(Vec::new()),
                },
            },
            span: test_span(),
        }],
        MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(1))))),
        None,
        vec![MirLocal { id: MirLocalId(1), name: None, ty: array_ty, mutable: false, span: test_span() }],
    );

    let radix_types::Lowerability::Rejected(gaps) = classify(&program, &types) else {
        panic!("array aggregate construct must reject");
    };
    assert!(
        gaps.iter()
            .any(|gap| gap.shape() == "kernel aggregate construct")
    );
}

#[test]
fn device_safe_rejects_array_return_type() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let array = types.array(numerus);
    let array_ty = MirType::semantic(array);
    let program = kernel_program_with_locals(
        &mut types,
        array_ty,
        Vec::new(),
        MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(1))))),
        None,
        vec![MirLocal { id: MirLocalId(1), name: None, ty: array_ty, mutable: false, span: test_span() }],
    );

    let radix_types::Lowerability::Rejected(gaps) = classify(&program, &types) else {
        panic!("array return type must reject");
    };
    assert!(
        gaps.iter()
            .any(|gap| gap.shape().starts_with("device aggregate value type"))
    );
}

#[test]
fn gpu_math_rejects_f32_array_as_vector_surrogate() {
    let mut types = TypeTable::new();
    let f32_ty = f32_type(&mut types);
    let vector_surrogate_ty = MirType::semantic(types.array(f32_ty.semantic_id()));
    let program = kernel_program_with_locals(
        &mut types,
        vector_surrogate_ty,
        vec![MirStatement {
            kind: MirStatementKind::Construct {
                destination: MirPlace::local(MirLocalId(1)),
                aggregate: MirAggregate {
                    kind: MirAggregateKind::Array,
                    ty: vector_surrogate_ty,
                    fields: MirAggregateFields::Ordered(Vec::new()),
                },
            },
            span: test_span(),
        }],
        MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(1))))),
        None,
        vec![MirLocal { id: MirLocalId(1), name: None, ty: vector_surrogate_ty, mutable: false, span: test_span() }],
    );

    let radix_types::Lowerability::Rejected(gaps) = classify(&program, &types) else {
        panic!("f32 array vector surrogate must reject");
    };
    assert!(gaps.iter().any(|gap| {
        gap.slug() == "target-policy-rejection"
            && gap.shape() == "device vector surrogate requires explicit vector fact"
    }));
}

fn tensor_view_return_program(tensor_ty: MirType) -> MirProgram {
    MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: Some(DefId(1)),
            name: None,
            params: vec![MirParam {
                local: MirLocalId(0),
                name: None,
                ty: tensor_ty,
                mode: MirParamMode::Owned,
                span: test_span(),
            }],
            locals: Vec::new(),
            temps: Vec::new(),
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: Vec::new(),
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(0))))),
                    span: test_span(),
                },
                span: test_span(),
            }],
            return_ty: tensor_ty,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: test_span(),
        }],
    }
}

#[test]
fn device_safe_accepts_rank1_f32_tensor_view_kernel() {
    let mut types = TypeTable::new();
    let f32_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let len = types.intern_index(IndexExpr::Literal(4));
    let tensor_ty = MirType::semantic(types.intern(Type::Tensor(f32_ty, len)));
    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: Some(DefId(1)),
            name: None,
            params: vec![MirParam {
                local: MirLocalId(0),
                name: None,
                ty: tensor_ty,
                mode: MirParamMode::Owned,
                span: test_span(),
            }],
            locals: Vec::new(),
            temps: Vec::new(),
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: Vec::new(),
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(0))))),
                    span: test_span(),
                },
                span: test_span(),
            }],
            return_ty: tensor_ty,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: test_span(),
        }],
    };

    assert_eq!(classify(&program, &types), radix_types::Lowerability::Capable);
}

#[test]
fn device_safe_rejects_host_owned_tensor_without_device_handle_layout() {
    let mut types = TypeTable::new();
    let f32_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let len = types.intern_index(IndexExpr::Literal(4));
    let tensor_ty = MirType::semantic(types.intern(Type::Tensor(f32_ty, len)));
    let program = tensor_view_return_program(tensor_ty);

    let radix_types::Lowerability::Rejected(gaps) = classify_host_owned(&program, &types) else {
        panic!("host-owned tensor must not become a device kernel resource");
    };
    assert!(gaps.iter().any(|gap| {
        gap.slug() == "target-policy-rejection" && gap.shape() == "kernel tensor storage/locus requires device handle"
    }));
}

#[test]
fn device_safe_rejects_i16_device_view_element() {
    let mut types = TypeTable::new();
    let tensor_ty = rank1_tensor_type(&mut types, Primitive::Numerus, NumericWidth::I16);
    let program = tensor_view_return_program(tensor_ty);
    let radix_types::Lowerability::Rejected(gaps) = classify(&program, &types) else {
        panic!("i16 device view element must reject");
    };
    assert!(
        gaps.iter()
            .any(|gap| gap.slug() == "missing-layout-fact" && gap.shape() == "kernel device view element i16"),
        "expected i16 layout gap, got {gaps:?}"
    );
}

#[test]
fn device_safe_admits_i64_device_view_element() {
    // Kernel-wide types ruling 2026-10-28: 64-bit integers are kernel values.
    let mut types = TypeTable::new();
    let tensor_ty = rank1_tensor_type(&mut types, Primitive::Numerus, NumericWidth::I64);
    let program = tensor_view_return_program(tensor_ty);
    if let radix_types::Lowerability::Rejected(gaps) = classify(&program, &types) {
        assert!(
            !gaps
                .iter()
                .any(|gap| gap.shape() == "kernel device view element i64"),
            "i64 view element must not be a layout gap, got {gaps:?}"
        );
    }
}

#[test]
fn device_safe_admits_u64_device_view_element() {
    // Kernel-wide types ruling 2026-10-28: 64-bit integers are kernel values.
    let mut types = TypeTable::new();
    let tensor_ty = rank1_tensor_type(&mut types, Primitive::Numerus, NumericWidth::U64);
    let program = tensor_view_return_program(tensor_ty);
    if let radix_types::Lowerability::Rejected(gaps) = classify(&program, &types) {
        assert!(
            !gaps
                .iter()
                .any(|gap| gap.shape() == "kernel device view element u64"),
            "u64 view element must not be a layout gap, got {gaps:?}"
        );
    }
}

#[test]
fn device_safe_rejects_f64_device_view_element() {
    let mut types = TypeTable::new();
    let tensor_ty = rank1_tensor_type(&mut types, Primitive::Fractus, NumericWidth::F64);
    let program = tensor_view_return_program(tensor_ty);
    let radix_types::Lowerability::Rejected(gaps) = classify(&program, &types) else {
        panic!("f64 device view element must reject");
    };
    assert!(
        gaps.iter()
            .any(|gap| gap.slug() == "missing-layout-fact" && gap.shape() == "kernel device view element f64"),
        "expected f64 layout gap, got {gaps:?}"
    );
}

#[test]
fn device_safe_reports_rank2_tensor_device_view_layout_gap() {
    let mut types = TypeTable::new();
    let f32_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let rows = types.intern_index(IndexExpr::Literal(2));
    let cols = types.intern_index(IndexExpr::Literal(2));
    let shape = types.intern_index(IndexExpr::Tuple(vec![rows, cols]));
    let tensor_ty = MirType::semantic(types.intern(Type::Tensor(f32_ty, shape)));
    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: Some(DefId(1)),
            name: None,
            params: vec![MirParam {
                local: MirLocalId(0),
                name: None,
                ty: tensor_ty,
                mode: MirParamMode::Owned,
                span: test_span(),
            }],
            locals: Vec::new(),
            temps: Vec::new(),
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: Vec::new(),
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(0))))),
                    span: test_span(),
                },
                span: test_span(),
            }],
            return_ty: tensor_ty,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: test_span(),
        }],
    };

    let radix_types::Lowerability::Rejected(gaps) = classify(&program, &types) else {
        panic!("rank-2 tensor device view must reject until view math exists");
    };
    assert!(
        gaps.iter()
            .any(|gap| gap.slug() == "missing-layout-fact" && gap.shape() == "kernel device view rank 2")
    );
}

fn role_func(is_kernel: bool, nucleum_fragment: bool, is_vertex: bool, is_fragment: bool) -> HirFunction {
    use crate::lexer::Symbol;
    HirFunction {
        name: Symbol(0),
        type_params: Vec::new(),
        params: Vec::new(),
        cli_args: None,
        ret_ty: None,
        err_ty: None,
        body: None,
        posture: radix_syntax::CallablePosture::SyncFinite,
        is_async: false,
        is_generator: false,
        is_kernel,
        nucleum_fragment,
        is_vertex,
        is_fragment,
        test: None,
    }
}

#[test]
fn device_role_accepts_kernel_only() {
    // CTO a7c616ed: kernel-only annotation yields MirDeviceRole::Kernel.
    assert_eq!(
        crate::mir::device::device_role_for_hir_function(&role_func(true, false, false, false)),
        Some(MirDeviceRole::Kernel)
    );
}

#[test]
fn device_role_rejects_vertex_only() {
    // Vertex-only is a graphics stage (MirKernelShaderStage), not a
    // MirDeviceRole — returns None.
    assert_eq!(
        crate::mir::device::device_role_for_hir_function(&role_func(false, false, true, false)),
        None
    );
}

#[test]
fn device_role_rejects_fragment_only() {
    // Fragment-only is a graphics stage, not a MirDeviceRole.
    assert_eq!(
        crate::mir::device::device_role_for_hir_function(&role_func(false, false, false, true)),
        None
    );
}

#[test]
fn device_role_rejects_conflicting_kernel_vertex() {
    // Conflicting compute + graphics: fail-closed, no device role.
    assert_eq!(
        crate::mir::device::device_role_for_hir_function(&role_func(true, false, true, false)),
        None
    );
}

#[test]
fn device_role_rejects_conflicting_kernel_fragment() {
    assert_eq!(
        crate::mir::device::device_role_for_hir_function(&role_func(true, false, false, true)),
        None
    );
}

#[test]
fn device_role_rejects_conflicting_vertex_fragment() {
    assert_eq!(
        crate::mir::device::device_role_for_hir_function(&role_func(false, false, true, true)),
        None
    );
}

#[test]
fn device_role_accepts_nucleum_fragment_only() {
    assert_eq!(
        crate::mir::device::device_role_for_hir_function(&role_func(false, true, false, false)),
        Some(MirDeviceRole::Fragment)
    );
    assert_eq!(
        role_func(false, true, false, false).nucleum_role(),
        Some(crate::hir::NucleumRole::Fragment)
    );
}

#[test]
fn device_role_rejects_conflicting_entry_and_fragment() {
    assert_eq!(
        crate::mir::device::device_role_for_hir_function(&role_func(true, true, false, false)),
        None
    );
    assert_eq!(role_func(true, true, false, false).nucleum_role(), None);
}

#[test]
fn device_role_rejects_nucleum_fragment_plus_vertex() {
    assert_eq!(
        crate::mir::device::device_role_for_hir_function(&role_func(false, true, true, false)),
        None
    );
}

#[test]
fn device_role_rejects_nucleum_fragment_plus_graphics_fragment() {
    assert_eq!(
        crate::mir::device::device_role_for_hir_function(&role_func(false, true, false, true)),
        None
    );
}

#[test]
fn device_role_rejects_nucleum_entry_plus_graphics_fragment() {
    assert_eq!(
        crate::mir::device::device_role_for_hir_function(&role_func(true, false, false, true)),
        None
    );
}

fn first_function_item(analysis: &crate::driver::AnalyzedModule) -> (&crate::hir::DefId, &HirFunction) {
    analysis
        .hir
        .items
        .iter()
        .find_map(|item| match &item.kind {
            crate::hir::HirItemKind::Function(func) => Some((&item.def_id, func.as_ref())),
            _ => None,
        })
        .expect("expected a function item")
}

#[test]
fn nucleum_annotation_lowers_to_entry_role() {
    let source = r"
@ nucleum
functio k(f32 x) → f32 {
    redde x
}
";
    let session = Session::new(Config::default().with_latin_locale());
    let analysis =
        driver::analyze_source(&session, "device.fab", source, AnalyzeRequest::default()).expect("analyze source");
    let (def_id, func) = first_function_item(&analysis);
    assert_eq!(func.nucleum_role(), Some(crate::hir::NucleumRole::Entry));
    assert!(func.is_kernel);
    assert!(!func.nucleum_fragment);
    let roles = device_roles_from_hir(&analysis.hir);
    assert_eq!(roles.get(def_id), Some(&MirDeviceRole::Kernel));
}

#[test]
fn nucleum_fragment_annotation_lowers_to_fragment_role() {
    let source = r"
@ nucleum fragment
functio helper(f32 x) → f32 {
    redde x
}
";
    let session = Session::new(Config::default().with_latin_locale());
    let analysis =
        driver::analyze_source(&session, "device.fab", source, AnalyzeRequest::default()).expect("analyze source");
    let (def_id, func) = first_function_item(&analysis);
    assert_eq!(func.nucleum_role(), Some(crate::hir::NucleumRole::Fragment));
    assert!(!func.is_kernel);
    assert!(func.nucleum_fragment);
    let roles = device_roles_from_hir(&analysis.hir);
    assert_eq!(
        roles.get(def_id),
        Some(&MirDeviceRole::Fragment),
        "fragment is a composition role, not Kernel"
    );
}

#[test]
fn nucleum_fragment_is_not_a_dispatch_or_reflection_entry() {
    let source = r"
@ nucleum fragment
functio helper(f32 x) → f32 {
    redde x
}

@ nucleum
functio k(f32 x) → f32 {
    redde x
}
";
    let session = Session::new(Config::default().with_latin_locale());
    let analysis =
        driver::analyze_source(&session, "device.fab", source, AnalyzeRequest::default()).expect("analyze source");
    let roles = device_roles_from_hir(&analysis.hir);
    let mut kernel_entries = 0;
    let mut fragment_entries = 0;
    for item in &analysis.hir.items {
        let crate::hir::HirItemKind::Function(_) = &item.kind else {
            continue;
        };
        match roles.get(&item.def_id) {
            Some(MirDeviceRole::Kernel) => kernel_entries += 1,
            Some(MirDeviceRole::Fragment) => fragment_entries += 1,
            other => panic!("unexpected role for {}: {other:?}", item.def_id.0),
        }
    }
    assert_eq!(kernel_entries, 1, "only @ nucleum Entry receives Kernel/dispatch identity");
    assert_eq!(fragment_entries, 1);
}

#[test]
fn nucleum_fragment_call_composes_into_entry() {
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
    let roles = device_roles_from_hir(&analysis.hir);
    let lowered = crate::mir::lower_analyzed_unit(analysis.application_view(), MirLoweringOptions::default())
        .expect("legal fragment call must compose");
    assert!(
        crate::mir::fragment_composition::surviving_fragment_calls_with_metadata(
            &lowered.validated.program().functions,
            &[],
            &roles,
            &crate::mir::MirDeviceFunctionMetadataMap::default(),
        )
        .is_empty(),
        "no fragment call may survive composition"
    );
}
