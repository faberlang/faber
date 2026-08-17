//! Shared MIR test helpers for backend text-emit probe tests.
//!
//! Extracted from `wgsl_text_test.rs` and `metal_text_test.rs` to eliminate ~600
//! lines of byte-identical duplication. Backend-specific assertion helpers
//! (`assert_wgsl_storage_binding`, `assert_metal_buffer_binding`) stay in their
//! respective test modules.

use crate::hir::DefId;
use crate::lexer::Interner;
use crate::mir::{
    MirBlock, MirBlockId, MirConstant, MirDeviceRole, MirFunction, MirFunctionId, MirKernelBuiltin,
    MirKernelReflection, MirKernelResourceAccess, MirKernelResourceRole, MirKernelSignature, MirLocal, MirLocalId,
    MirOperand, MirParam, MirParamMode, MirPlace, MirProgram, MirScalarLayout, MirStatement, MirStatementKind,
    MirSwitchCase, MirTemp, MirTempId, MirTerminator, MirTerminatorKind, MirType, MirValidationContext, MirValue,
    MirValueId, MirValueKind,
};
use crate::semantic::{IndexExpr, NumericWidth, Primitive, TypeTable};
use crate::test_support::test_span;
use rustc_hash::FxHashMap;

// Canonical homes: `f32_type` and `indexed_local_place` live in `test_support` (mir).
pub use super::test_support::{f32_type, indexed_local_place};

// ---------------------------------------------------------------------------
// Shared assertion helpers
// ---------------------------------------------------------------------------

/// Assert that a kernel resource record matches expected metadata.
pub fn assert_kernel_resource(
    kernel: &MirKernelReflection,
    index: usize,
    binding: u32,
    role: MirKernelResourceRole,
    access: MirKernelResourceAccess,
    layout: MirScalarLayout,
    source_local: MirLocalId,
) {
    let resource = &kernel.resources[index];
    assert_eq!(resource.group, 0);
    assert_eq!(resource.binding, binding);
    assert_eq!(resource.role, role);
    assert_eq!(resource.access, access);
    assert_eq!(resource.element_layout, layout);
    assert_eq!(resource.source_local, Some(source_local));
}

/// Assert two input + one output f32 resources on a kernel.
pub fn assert_f32_input_pair_output_resources(kernel: &MirKernelReflection) {
    assert_eq!(kernel.resources.len(), 3);
    assert!(kernel.resources.iter().all(|resource| resource.group == 0));
    assert_kernel_resource(
        kernel,
        0,
        0,
        MirKernelResourceRole::Input,
        MirKernelResourceAccess::Read,
        MirScalarLayout::F32,
        MirLocalId(0),
    );
    assert_kernel_resource(
        kernel,
        1,
        1,
        MirKernelResourceRole::Input,
        MirKernelResourceAccess::Read,
        MirScalarLayout::F32,
        MirLocalId(1),
    );
    assert_kernel_resource(
        kernel,
        2,
        2,
        MirKernelResourceRole::Output,
        MirKernelResourceAccess::Write,
        MirScalarLayout::F32,
        MirLocalId(2),
    );
}

/// Assert that every substring in `expected` appears in `text` and that they
/// appear in order. On failure the full text is included in the panic message
/// so that the developer can see what was actually emitted.
pub fn assert_emission_contains_ordered(text: &str, expected: &[&str]) {
    let mut search_from = 0usize;
    for &needle in expected {
        match text[search_from..].find(needle) {
            Some(pos) => search_from += pos + needle.len(),
            None => panic!(
                "expected substring {needle:?} not found (or found out of order) in emitted text:\n---\n{text}\n---"
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// Local fixture constructors (not a public MIR builder API)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct IndexedKernelTypes {
    tensor_ty: MirType,
    f32_ty: MirType,
    u32_ty: MirType,
    vacuum_ty: MirType,
}

fn indexed_kernel_types(types: &mut TypeTable, element_count: u64) -> IndexedKernelTypes {
    let f32_type_id = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let len = types.intern_index(IndexExpr::Literal(element_count));
    IndexedKernelTypes {
        tensor_ty: MirType::semantic(types.tensor_with_shape(f32_type_id, len)),
        f32_ty: MirType::semantic(f32_type_id),
        u32_ty: MirType::semantic(types.sized_numeric(Primitive::Numerus, NumericWidth::U32)),
        vacuum_ty: MirType::semantic(types.primitive(Primitive::Vacuum)),
    }
}

fn terminator(kind: MirTerminatorKind) -> MirTerminator {
    MirTerminator { kind, span: test_span() }
}

fn mir_block(id: u32, statements: Vec<MirStatement>, kind: MirTerminatorKind) -> MirBlock {
    MirBlock { id: MirBlockId(id), statements, terminator: terminator(kind), span: test_span() }
}

fn goto_kind(target: u32) -> MirTerminatorKind {
    MirTerminatorKind::Goto(MirBlockId(target))
}

fn goto_block(id: u32, target: u32) -> MirBlock {
    mir_block(id, Vec::new(), goto_kind(target))
}

fn return_void(id: u32) -> MirBlock {
    mir_block(id, Vec::new(), MirTerminatorKind::Return(None))
}

fn return_local(id: u32, local: u32) -> MirBlock {
    mir_block(
        id,
        Vec::new(),
        MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(local))))),
    )
}

fn true_branch(then_block: u32, else_block: u32) -> MirTerminatorKind {
    MirTerminatorKind::Branch {
        condition: MirOperand::Constant(MirConstant::Bool(true)),
        then_block: MirBlockId(then_block),
        else_block: MirBlockId(else_block),
    }
}

fn false_branch(then_block: u32, else_block: u32) -> MirTerminatorKind {
    MirTerminatorKind::Branch {
        condition: MirOperand::Constant(MirConstant::Bool(false)),
        then_block: MirBlockId(then_block),
        else_block: MirBlockId(else_block),
    }
}

fn true_switch(case_target: u32, default: u32) -> MirTerminatorKind {
    MirTerminatorKind::Switch {
        value: MirOperand::Constant(MirConstant::Bool(true)),
        cases: vec![MirSwitchCase { value: MirConstant::Bool(true), target: MirBlockId(case_target) }],
        default: MirBlockId(default),
    }
}

fn assign(place: MirPlace, value: MirValue) -> MirStatement {
    MirStatement { kind: MirStatementKind::Assign { place, value }, span: test_span() }
}

fn place_value(id: u32, place: MirPlace, ty: MirType) -> MirValue {
    MirValue { id: MirValueId(id), kind: MirValueKind::Operand(MirOperand::Place(place)), ty, span: test_span() }
}

fn constant_value(id: u32, constant: MirConstant, ty: MirType) -> MirValue {
    MirValue { id: MirValueId(id), kind: MirValueKind::Operand(MirOperand::Constant(constant)), ty, span: test_span() }
}

fn owned_param(local: u32, ty: MirType) -> MirParam {
    MirParam { local: MirLocalId(local), name: None, ty, mode: MirParamMode::Owned, span: test_span() }
}

fn mut_local(id: u32, ty: MirType) -> MirLocal {
    MirLocal { id: MirLocalId(id), name: None, ty, mutable: true, span: test_span() }
}

fn scalar_param(f32_ty: MirType) -> Vec<MirParam> {
    vec![owned_param(0, f32_ty)]
}

fn indexed_write_params(tys: &IndexedKernelTypes) -> Vec<MirParam> {
    vec![
        owned_param(0, tys.tensor_ty),
        owned_param(1, tys.tensor_ty),
        owned_param(2, tys.u32_ty),
    ]
}

/// Indexed kernel convention: local 0 is the input tensor, local 1 the output, local 2 the index.
fn indexed_in() -> MirPlace {
    indexed_local_place(MirLocalId(0), MirLocalId(2))
}

fn indexed_out() -> MirPlace {
    indexed_local_place(MirLocalId(1), MirLocalId(2))
}

fn kernel_program(
    params: Vec<MirParam>,
    locals: Vec<MirLocal>,
    temps: Vec<MirTemp>,
    blocks: Vec<MirBlock>,
    return_ty: MirType,
) -> MirProgram {
    MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: Some(DefId(1)),
            name: None,
            params,
            locals,
            temps,
            blocks,
            return_ty,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: test_span(),
        }],
    }
}

fn block_id(id: usize) -> u32 {
    u32::try_from(id).expect("test goto-chain block id bounded by u32::MAX")
}

fn goto_chain_blocks(intermediate_count: usize, target: MirBlock) -> Vec<MirBlock> {
    let mut blocks = vec![goto_block(0, 1)];
    for index in 1..=intermediate_count {
        blocks.push(goto_block(block_id(index), block_id(index + 1)));
    }
    blocks.push(target);
    blocks
}

#[must_use]
pub fn kernel_roles() -> FxHashMap<DefId, MirDeviceRole> {
    FxHashMap::from_iter([(DefId(1), MirDeviceRole::Kernel)])
}

// ---------------------------------------------------------------------------
// Indexed write kernel builders
// ---------------------------------------------------------------------------

#[must_use]
pub fn indexed_write_goto_kernel_with_len(
    types: &mut TypeTable,
    intermediate_count: usize,
    element_count: u64,
) -> MirProgram {
    let tys = indexed_kernel_types(types, element_count);
    let target = mir_block(
        block_id(intermediate_count + 1),
        vec![assign(indexed_out(), place_value(0, indexed_in(), tys.f32_ty))],
        MirTerminatorKind::Return(None),
    );
    kernel_program(
        indexed_write_params(&tys),
        Vec::new(),
        Vec::new(),
        goto_chain_blocks(intermediate_count, target),
        tys.vacuum_ty,
    )
}

#[must_use]
pub fn indexed_write_goto_kernel(types: &mut TypeTable, intermediate_count: usize) -> MirProgram {
    indexed_write_goto_kernel_with_len(types, intermediate_count, 4)
}

// ---------------------------------------------------------------------------
// Scalar return kernel builders
// ---------------------------------------------------------------------------

#[must_use]
pub fn scalar_return_goto_kernel(types: &mut TypeTable, intermediate_count: usize) -> MirProgram {
    let f32_ty = f32_type(types);
    kernel_program(
        scalar_param(f32_ty),
        Vec::new(),
        Vec::new(),
        goto_chain_blocks(intermediate_count, return_local(block_id(intermediate_count + 1), 0)),
        f32_ty,
    )
}

// ---------------------------------------------------------------------------
// Branch/switch arm goto kernel builders
// ---------------------------------------------------------------------------

fn scalar_return_arm_goto_kernel(types: &mut TypeTable, dispatch: MirTerminatorKind) -> MirProgram {
    let f32_ty = f32_type(types);
    kernel_program(
        scalar_param(f32_ty),
        Vec::new(),
        vec![MirTemp { id: MirTempId(0), ty: f32_ty, span: test_span() }],
        vec![
            mir_block(0, Vec::new(), dispatch),
            goto_block(1, 3),
            mir_block(
                2,
                vec![assign(
                    MirPlace::temp(MirTempId(0)),
                    constant_value(0, MirConstant::Float(0.0), f32_ty),
                )],
                MirTerminatorKind::Return(Some(MirOperand::Temp(MirTempId(0)))),
            ),
            return_local(3, 0),
        ],
        f32_ty,
    )
}

#[must_use]
pub fn scalar_return_branch_arm_goto_kernel(types: &mut TypeTable) -> MirProgram {
    scalar_return_arm_goto_kernel(types, true_branch(1, 2))
}

#[must_use]
pub fn scalar_return_switch_arm_goto_kernel(types: &mut TypeTable) -> MirProgram {
    scalar_return_arm_goto_kernel(types, true_switch(1, 2))
}

#[must_use]
pub fn scalar_value_join_branch_arm_goto_kernel(types: &mut TypeTable) -> MirProgram {
    let f32_ty = f32_type(types);
    kernel_program(
        scalar_param(f32_ty),
        vec![mut_local(1, f32_ty)],
        Vec::new(),
        vec![
            mir_block(0, Vec::new(), true_branch(1, 2)),
            goto_block(1, 3),
            goto_block(2, 4),
            mir_block(
                3,
                vec![assign(
                    MirPlace::local(MirLocalId(1)),
                    place_value(0, MirPlace::local(MirLocalId(0)), f32_ty),
                )],
                goto_kind(5),
            ),
            mir_block(
                4,
                vec![assign(
                    MirPlace::local(MirLocalId(1)),
                    constant_value(1, MirConstant::Float(0.0), f32_ty),
                )],
                goto_kind(5),
            ),
            return_local(5, 1),
        ],
        f32_ty,
    )
}

// ---------------------------------------------------------------------------
// Indexed switch / branch arm builders
// ---------------------------------------------------------------------------

fn indexed_arm_goto_kernel(types: &mut TypeTable, dispatch: MirTerminatorKind) -> MirProgram {
    let tys = indexed_kernel_types(types, 4);
    kernel_program(
        indexed_write_params(&tys),
        Vec::new(),
        Vec::new(),
        vec![
            mir_block(0, Vec::new(), dispatch),
            goto_block(1, 3),
            mir_block(
                2,
                vec![assign(
                    indexed_out(),
                    constant_value(0, MirConstant::Float(0.0), tys.f32_ty),
                )],
                goto_kind(4),
            ),
            mir_block(
                3,
                vec![assign(indexed_out(), place_value(1, indexed_in(), tys.f32_ty))],
                goto_kind(4),
            ),
            return_void(4),
        ],
        tys.vacuum_ty,
    )
}

#[must_use]
pub fn indexed_switch_arm_goto_kernel(types: &mut TypeTable) -> MirProgram {
    indexed_arm_goto_kernel(types, true_switch(1, 2))
}

#[must_use]
pub fn indexed_branch_arm_goto_kernel(types: &mut TypeTable) -> MirProgram {
    indexed_arm_goto_kernel(types, true_branch(1, 2))
}

#[must_use]
pub fn indexed_value_join_branch_arm_goto_kernel(types: &mut TypeTable) -> MirProgram {
    let tys = indexed_kernel_types(types, 4);
    kernel_program(
        indexed_write_params(&tys),
        vec![mut_local(3, tys.f32_ty)],
        Vec::new(),
        vec![
            mir_block(0, Vec::new(), true_branch(1, 2)),
            goto_block(1, 3),
            goto_block(2, 4),
            mir_block(
                3,
                vec![assign(
                    MirPlace::local(MirLocalId(3)),
                    place_value(0, indexed_in(), tys.f32_ty),
                )],
                goto_kind(5),
            ),
            mir_block(
                4,
                vec![assign(
                    MirPlace::local(MirLocalId(3)),
                    constant_value(1, MirConstant::Float(0.0), tys.f32_ty),
                )],
                goto_kind(5),
            ),
            mir_block(
                5,
                vec![assign(
                    indexed_out(),
                    place_value(2, MirPlace::local(MirLocalId(3)), tys.f32_ty),
                )],
                MirTerminatorKind::Return(None),
            ),
        ],
        tys.vacuum_ty,
    )
}

// ---------------------------------------------------------------------------
// Loop entry goto kernel builders
// ---------------------------------------------------------------------------

#[must_use]
pub fn scalar_loop_entry_goto_kernel(types: &mut TypeTable) -> MirProgram {
    let f32_ty = f32_type(types);
    kernel_program(
        scalar_param(f32_ty),
        vec![mut_local(1, f32_ty)],
        Vec::new(),
        vec![
            mir_block(
                0,
                vec![assign(
                    MirPlace::local(MirLocalId(1)),
                    place_value(0, MirPlace::local(MirLocalId(0)), f32_ty),
                )],
                goto_kind(4),
            ),
            mir_block(1, Vec::new(), false_branch(2, 3)),
            mir_block(
                2,
                vec![assign(
                    MirPlace::local(MirLocalId(1)),
                    constant_value(1, MirConstant::Float(0.0), f32_ty),
                )],
                goto_kind(1),
            ),
            return_local(3, 1),
            goto_block(4, 1),
        ],
        f32_ty,
    )
}

#[must_use]
pub fn indexed_loop_entry_goto_kernel(types: &mut TypeTable) -> MirProgram {
    let tys = indexed_kernel_types(types, 4);
    kernel_program(
        indexed_write_params(&tys),
        vec![mut_local(3, tys.u32_ty)],
        Vec::new(),
        vec![
            mir_block(
                0,
                vec![assign(
                    MirPlace::local(MirLocalId(3)),
                    constant_value(0, MirConstant::Int(0), tys.u32_ty),
                )],
                goto_kind(4),
            ),
            mir_block(1, Vec::new(), false_branch(2, 3)),
            mir_block(
                2,
                vec![
                    assign(indexed_out(), place_value(1, indexed_in(), tys.f32_ty)),
                    assign(
                        MirPlace::local(MirLocalId(3)),
                        constant_value(2, MirConstant::Int(1), tys.u32_ty),
                    ),
                ],
                goto_kind(1),
            ),
            return_void(3),
            goto_block(4, 1),
        ],
        tys.vacuum_ty,
    )
}

#[must_use]
pub fn scalar_loop_control_entry_goto_kernel(types: &mut TypeTable, control_is_break: bool) -> MirProgram {
    let f32_ty = f32_type(types);
    let control_target = if control_is_break { 3 } else { 1 };
    kernel_program(
        scalar_param(f32_ty),
        vec![mut_local(1, f32_ty)],
        Vec::new(),
        vec![
            mir_block(
                0,
                vec![assign(
                    MirPlace::local(MirLocalId(1)),
                    place_value(0, MirPlace::local(MirLocalId(0)), f32_ty),
                )],
                goto_kind(6),
            ),
            mir_block(1, Vec::new(), false_branch(2, 3)),
            mir_block(2, Vec::new(), true_branch(4, 5)),
            return_local(3, 1),
            goto_block(4, control_target),
            mir_block(
                5,
                vec![assign(
                    MirPlace::local(MirLocalId(1)),
                    constant_value(1, MirConstant::Float(0.0), f32_ty),
                )],
                goto_kind(1),
            ),
            goto_block(6, 1),
        ],
        f32_ty,
    )
}

#[must_use]
pub fn indexed_loop_control_entry_goto_kernel(types: &mut TypeTable, control_is_break: bool) -> MirProgram {
    let tys = indexed_kernel_types(types, 4);
    let control_target = if control_is_break { 3 } else { 1 };
    kernel_program(
        indexed_write_params(&tys),
        vec![mut_local(3, tys.u32_ty)],
        Vec::new(),
        vec![
            mir_block(
                0,
                vec![assign(
                    MirPlace::local(MirLocalId(3)),
                    constant_value(0, MirConstant::Int(0), tys.u32_ty),
                )],
                goto_kind(6),
            ),
            mir_block(1, Vec::new(), false_branch(2, 3)),
            mir_block(2, Vec::new(), true_branch(4, 5)),
            return_void(3),
            goto_block(4, control_target),
            mir_block(
                5,
                vec![
                    assign(indexed_out(), place_value(1, indexed_in(), tys.f32_ty)),
                    assign(
                        MirPlace::local(MirLocalId(3)),
                        constant_value(2, MirConstant::Int(1), tys.u32_ty),
                    ),
                ],
                goto_kind(1),
            ),
            goto_block(6, 1),
        ],
        tys.vacuum_ty,
    )
}

// ---------------------------------------------------------------------------
// Signature builder
// ---------------------------------------------------------------------------

#[must_use]
pub fn indexed_signature_with_builtin_input(builtin: MirKernelBuiltin) -> MirKernelSignature {
    let mut types = TypeTable::new();
    let program = indexed_write_goto_kernel(&mut types, 0);
    let validation = MirValidationContext::new(&types);
    let mut signature = MirKernelSignature::storage_buffer_kernel_with_interner_for_target_entry(
        &program.functions[0],
        &validation,
        &Interner::new(),
    )
    .expect("indexed kernel signature should lower");
    signature.builtin_inputs[0].builtin = builtin;
    signature
}
