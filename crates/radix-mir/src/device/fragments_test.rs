//! Survivor-invariant proofs at the `ValidatedMir` / `validate_program` seam.

use super::{
    FragmentSurvivorKind, NUCLEUM_FRAGMENT_SURVIVED, first_surviving_fragment_call,
    reject_nucleum_fragment_survivors, surviving_nucleum_fragments,
};
use crate::{
    MirBlock, MirBlockId, MirCallee, MirDeviceContext, MirDeviceRole, MirFunction, MirFunctionId,
    MirLocal, MirLocalId, MirOperand, MirParam, MirParamMode, MirPlace, MirProgram, MirStatement,
    MirStatementKind, MirTerminator, MirTerminatorKind, MirType, MirValidationContext,
    ValidatedMir, validate_program, validate_program_with_device_roles,
};
use handroll::FxHashMap;
use radix_hir::DefId;
use radix_lexer::Span;
use radix_types::{Primitive, TypeTable};

const ENTRY: DefId = DefId(1);
const FRAGMENT: DefId = DefId(2);

fn span() -> Span {
    Span::default()
}

fn f32_ty(types: &mut TypeTable) -> MirType {
    MirType::semantic(types.sized_numeric(Primitive::Fractus, radix_types::NumericWidth::F32))
}

fn param(local: u32, ty: MirType) -> MirParam {
    MirParam {
        local: MirLocalId(local),
        name: None,
        ty,
        mode: MirParamMode::Owned,
        span: span(),
    }
}

fn identity_function(id: u32, source: DefId, ty: MirType) -> MirFunction {
    MirFunction {
        id: MirFunctionId(id),
        source: Some(source),
        name: None,
        params: vec![param(0, ty)],
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: Vec::new(),
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                    MirLocalId(0),
                )))),
                span: span(),
            },
            span: span(),
        }],
        return_ty: ty,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: span(),
    }
}

fn kernel_calling_fragment(ty: MirType) -> MirFunction {
    MirFunction {
        id: MirFunctionId(0),
        source: Some(ENTRY),
        name: None,
        params: vec![param(0, ty)],
        locals: vec![MirLocal {
            id: MirLocalId(1),
            name: None,
            ty,
            mutable: false,
            span: span(),
        }],
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![MirStatement {
                kind: MirStatementKind::Call {
                    destination: Some(MirPlace::local(MirLocalId(1))),
                    callee: MirCallee::Function(MirFunctionId(1)),
                    args: vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
                },
                span: span(),
            }],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                    MirLocalId(1),
                )))),
                span: span(),
            },
            span: span(),
        }],
        return_ty: ty,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: span(),
    }
}

fn composed_kernel(ty: MirType) -> MirFunction {
    identity_function(0, ENTRY, ty)
}

fn roles() -> FxHashMap<DefId, MirDeviceRole> {
    let mut roles = FxHashMap::default();
    roles.insert(ENTRY, MirDeviceRole::Kernel);
    roles.insert(FRAGMENT, MirDeviceRole::Fragment);
    roles
}

fn kernel_only_roles() -> FxHashMap<DefId, MirDeviceRole> {
    let mut roles = FxHashMap::default();
    roles.insert(ENTRY, MirDeviceRole::Kernel);
    roles
}

fn uncomposed(ty: MirType) -> MirProgram {
    MirProgram {
        functions: vec![
            kernel_calling_fragment(ty),
            identity_function(1, FRAGMENT, ty),
        ],
    }
}

fn composed(ty: MirType) -> MirProgram {
    MirProgram {
        functions: vec![composed_kernel(ty)],
    }
}

fn leftover_symbol(ty: MirType) -> MirProgram {
    MirProgram {
        functions: vec![composed_kernel(ty), identity_function(1, FRAGMENT, ty)],
    }
}

fn device_from(program: &MirProgram, roles: &FxHashMap<DefId, MirDeviceRole>) -> MirDeviceContext {
    let mut device = MirDeviceContext::from_hir_roles(roles.clone());
    device.attach_program(program);
    device
}

#[test]
fn ordinary_kernel_without_roles_validates() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let program = composed(ty);
    let context = MirValidationContext::new(&types);
    ValidatedMir::new(program, context).expect("fragment-free kernel must validate");
}

#[test]
fn ordinary_kernel_with_kernel_role_has_no_survivors() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let program = composed(ty);
    let device = device_from(&program, &kernel_only_roles());
    assert!(
        surviving_nucleum_fragments(&program, &device).is_empty(),
        "guards must not false-positive on fragment-free kernels"
    );
    let context = MirValidationContext::new(&types);
    ValidatedMir::with_device_roles(program, context, &kernel_only_roles())
        .expect("kernel-only roles must not reject an ordinary kernel");
}

#[test]
fn validate_program_without_roles_does_not_reject_uncomposed() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let program = uncomposed(ty);
    let context = MirValidationContext::new(&types);
    validate_program(&program, &context)
        .expect("role-blind validate_program must not invent fragment survivors");
}

#[test]
fn uncomposed_program_reports_call_and_symbol() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let program = uncomposed(ty);
    let device = device_from(&program, &roles());
    let survivors = surviving_nucleum_fragments(&program, &device);
    assert!(
        survivors
            .iter()
            .any(|survivor| survivor.kind == FragmentSurvivorKind::Call),
        "bypassed composition must leave a fragment call, got {survivors:?}"
    );
    assert!(
        survivors
            .iter()
            .any(|survivor| survivor.kind == FragmentSurvivorKind::Symbol),
        "bypassed composition must leave a fragment symbol, got {survivors:?}"
    );
    assert!(first_surviving_fragment_call(&program, &device).is_some());
}

#[test]
fn mutation_bypassed_composition_fails_validation() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let program = uncomposed(ty);
    let context = MirValidationContext::new(&types);
    let errors = validate_program_with_device_roles(&program, &context, &roles())
        .expect_err("composition bypass must fail closed at validate_program");
    assert!(
        errors
            .iter()
            .any(|error| error.issue == NUCLEUM_FRAGMENT_SURVIVED),
        "expected {NUCLEUM_FRAGMENT_SURVIVED}, got {errors:?}"
    );
    let context = MirValidationContext::new(&types);
    let errors = ValidatedMir::with_device_roles(program, context, &roles())
        .expect_err("composition bypass must fail closed at ValidatedMir");
    assert!(
        errors
            .iter()
            .any(|error| error.issue == NUCLEUM_FRAGMENT_SURVIVED),
        "expected {NUCLEUM_FRAGMENT_SURVIVED}, got {errors:?}"
    );
}

#[test]
fn standalone_fragment_symbol_is_rejected() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let program = leftover_symbol(ty);
    let device = device_from(&program, &roles());
    assert!(
        surviving_nucleum_fragments(&program, &device)
            .iter()
            .any(|survivor| survivor.kind == FragmentSurvivorKind::Symbol),
        "standalone leftover must be a symbol survivor"
    );
    let errors = reject_nucleum_fragment_survivors(&program, &device);
    assert!(
        errors
            .iter()
            .any(|error| error.issue == NUCLEUM_FRAGMENT_SURVIVED),
        "standalone fragment symbol must be rejected, got {errors:?}"
    );
    let context = MirValidationContext::new(&types);
    ValidatedMir::with_device_roles(program, context, &roles())
        .expect_err("leftover fragment symbol must not enter validated MIR");
}

#[test]
fn composed_kernel_validates_with_roles() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let program = composed(ty);
    let context = MirValidationContext::new(&types);
    let validated = ValidatedMir::with_device_roles(program, context, &kernel_only_roles())
        .expect("composed entry with no leftover fragment must validate");
    let has_call = validated.program().functions.iter().any(|function| {
        function.blocks.iter().any(|block| {
            block
                .statements
                .iter()
                .any(|statement| matches!(statement.kind, MirStatementKind::Call { .. }))
        })
    });
    assert!(
        !has_call,
        "composed validated MIR must not retain a fragment call"
    );
}

#[test]
fn definition_callee_is_a_surviving_call() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let mut kernel = kernel_calling_fragment(ty);
    if let MirStatementKind::Call { callee, .. } = &mut kernel.blocks[0].statements[0].kind {
        *callee = MirCallee::definition(FRAGMENT);
    }
    let program = MirProgram {
        functions: vec![kernel, identity_function(1, FRAGMENT, ty)],
    };
    let device = device_from(&program, &roles());
    assert!(first_surviving_fragment_call(&program, &device).is_some());
}

/// DFV2-4: the survivor predicate covers every v2 device call — a kernel
/// helper callee is a survivor exactly like a fragment callee. Host callers
/// are the launch boundary and are never scanned.
#[test]
fn kernel_to_kernel_call_is_a_surviving_device_call() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let mut caller = kernel_calling_fragment(ty);
    caller.source = Some(ENTRY);
    let mut helper = identity_function(1, FRAGMENT, ty);
    helper.source = Some(DefId(3));
    let program = MirProgram {
        functions: vec![caller, helper],
    };
    let mut kernel_roles = roles();
    kernel_roles.insert(DefId(3), MirDeviceRole::Kernel);
    let device = device_from(&program, &kernel_roles);
    let survivor = first_surviving_fragment_call(&program, &device)
        .expect("an uncomposed kernel-helper call is a device-call survivor");
    assert_eq!(survivor.kind, FragmentSurvivorKind::Call);
}

/// DFV2-4: a host function's call to a public kernel entry is the launch
/// boundary — not a survivor.
#[test]
fn host_call_to_kernel_entry_is_not_a_survivor() {
    let mut types = TypeTable::new();
    let ty = f32_ty(&mut types);
    let entry = identity_function(1, ENTRY, ty);
    let mut host = kernel_calling_fragment(ty);
    host.source = None;
    host.shader_stage = None;
    let program = MirProgram {
        functions: vec![host, entry],
    };
    let device = device_from(&program, &kernel_only_roles());
    assert!(
        surviving_nucleum_fragments(&program, &device).is_empty(),
        "host→entry is the launch boundary, not a composition survivor"
    );
}
