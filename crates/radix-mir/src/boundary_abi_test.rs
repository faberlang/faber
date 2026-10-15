//! NGAB1-U2 boundary-ABI validation tests (radix-mir side).
//!
//! Proves the versioned call-ABI gate rejects each invalid cross-boundary
//! class with its typed diagnostic — wrong type, wrong shape, wrong lifetime,
//! mutation of a read-only resource, and observation of unlaunched work —
//! and accepts the minimal one-region fixture (one host call → one prepared
//! region containing one kernel). The faber negative fixtures prove the same
//! classes end-to-end through source lowering (rejection without a launch).

use super::*;
use crate::{
    MirBlock, MirBlockId, MirCallee, MirConstant, MirFunction, MirFunctionId, MirKernelShaderStage,
    MirLocal, MirLocalId, MirOperand, MirParam, MirParamMode, MirPlace, MirProgram, MirStatement,
    MirStatementKind, MirTerminator, MirTerminatorKind, MirType, ValidatedMir,
};
use radix_lexer::{Interner, Span};
use radix_types::{IndexExpr, NumericWidth, Primitive, TypeTable};

/// f32 (`fractus<f32>`) — the fixture's scalar surface.
fn f32(types: &mut TypeTable) -> MirType {
    MirType::semantic(types.sized_numeric(Primitive::Fractus, NumericWidth::F32))
}

/// f16 (`fractus<f16>`) — a host value that widens into f32 in ordinary MIR
/// but carries the wrong element type at the typed boundary.
fn f16(types: &mut TypeTable) -> MirType {
    MirType::semantic(types.sized_numeric(Primitive::Fractus, NumericWidth::F16))
}

/// `tensor<f32, [n]>` — a literal-shaped device view.
fn view(types: &mut TypeTable, element: MirType, count: u64) -> MirType {
    let shape = types.intern_index(IndexExpr::Literal(count));
    MirType::semantic(types.tensor_with_shape(element.semantic_id(), shape))
}

/// `tensor<f32, [rows, cols]>` — a rank-2 literal-shaped device view.
fn view2d(types: &mut TypeTable, element: MirType, rows: u64, cols: u64) -> MirType {
    let row = types.intern_index(IndexExpr::Literal(rows));
    let col = types.intern_index(IndexExpr::Literal(cols));
    let shape = types.intern_index(IndexExpr::Tuple(vec![row, col]));
    MirType::semantic(types.tensor_with_shape(element.semantic_id(), shape))
}

fn local(id: u32, ty: MirType) -> MirLocal {
    MirLocal {
        id: MirLocalId(id),
        name: None,
        ty,
        mutable: true,
        span: Span::default(),
    }
}

fn param(local_id: u32, ty: MirType) -> MirParam {
    MirParam {
        local: MirLocalId(local_id),
        name: None,
        ty,
        mode: MirParamMode::Owned,
        span: Span::default(),
    }
}

/// A call statement: `local = kernel_k(args...)` with the given destination
/// local (None for a no-destination call).
fn call_stmt(kernel_id: u32, destination: Option<u32>, args: Vec<MirOperand>) -> MirStatement {
    MirStatement {
        kind: MirStatementKind::Call {
            destination: destination.map(|id| MirPlace::local(MirLocalId(id))),
            callee: MirCallee::Function(MirFunctionId(kernel_id)),
            args,
        },
        span: Span::default(),
    }
}

/// One kernel function declaring `params` (the boundary gate reads the
/// declared params only).
fn kernel(id: u32, params: Vec<MirParam>, return_ty: MirType) -> MirFunction {
    MirFunction {
        id: MirFunctionId(id),
        source: None,
        name: None,
        params,
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: Vec::new(),
        return_ty,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: Some(MirKernelShaderStage::Compute),
        span: Span::default(),
    }
}

/// A host function whose single block contains exactly the given call
/// statements, with `params` as the declared parameters and `locals` as the
/// extra locals.
fn host_with_calls(
    id: u32,
    params: Vec<MirParam>,
    locals: Vec<MirLocal>,
    calls: Vec<MirStatement>,
    return_ty: MirType,
) -> MirFunction {
    MirFunction {
        id: MirFunctionId(id),
        source: None,
        name: None,
        params,
        locals,
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: calls,
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(None),
                span: Span::default(),
            },
            span: Span::default(),
        }],
        return_ty,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    }
}

/// Run the boundary-ABI gate over a hand-built program and return the result.
///
/// The kernel is f1 with one parameter; the host is f2 with one parameter,
/// one extra local f30, and the given call statements. `build` receives the
/// shared type table (so every `MirType` carries an id this table resolves)
/// and returns `(kernel_param_ty, host_param_ty, calls)`.
fn validate_case(
    build: impl FnOnce(&mut TypeTable) -> (MirType, MirType, Vec<MirStatement>),
) -> Result<(), BoundaryAbiError> {
    let mut types = TypeTable::default();
    let f32_ty = f32(&mut types);
    let (kernel_param_ty, host_param_ty, calls) = build(&mut types);
    let program = MirProgram {
        functions: vec![
            kernel(1, vec![param(10, kernel_param_ty)], f32_ty),
            host_with_calls(
                2,
                vec![param(20, host_param_ty)],
                vec![local(30, host_param_ty)],
                calls,
                f32_ty,
            ),
        ],
    };
    let context = MirValidationContext::new(&types);
    let calls = derive_boundary_calls(&program, &[MirFunctionId(1)]);
    validate_boundary_abi(&program, &calls, &context)
}

/// The happy path: one host call → one prepared region → one kernel, an f32
/// value crossing an exact f32 boundary into a plain local destination.
#[test]
fn boundary_abi_accepts_exact_scalar_call() {
    let mut types = TypeTable::default();
    let f32_ty = f32(&mut types);
    let mut program = MirProgram {
        functions: vec![kernel(1, vec![param(10, f32_ty)], f32_ty)],
    };
    program.functions.push(host_with_calls(
        2,
        vec![param(20, f32_ty)],
        vec![local(30, f32_ty)],
        vec![call_stmt(
            1,
            Some(30),
            vec![MirOperand::Place(MirPlace::local(MirLocalId(20)))],
        )],
        f32_ty,
    ));
    let context = MirValidationContext::new(&types);
    let calls = derive_boundary_calls(&program, &[MirFunctionId(1)]);

    // The amended granularity: one host call → one prepared region containing
    // one kernel (the minimal one-region case, not a separate ABI).
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].host, MirFunctionId(2));
    assert_eq!(calls[0].region.kernels, vec![MirFunctionId(1)]);
    assert!(calls[0].region.is_minimal());

    validate_boundary_abi(&program, &calls, &context).expect("the exact scalar call is valid");
}

/// Wrong type: an f16 value crosses a boundary that declares f32. General MIR
/// allows the widening; the typed boundary requires the exact carried element
/// type fact.
#[test]
fn boundary_abi_rejects_wrong_element_type() {
    let result = validate_case(|types| {
        (
            f32(types),
            f16(types),
            vec![call_stmt(
                1,
                Some(30),
                vec![MirOperand::Place(MirPlace::local(MirLocalId(20)))],
            )],
        )
    });
    match result {
        Err(BoundaryAbiError::Type { position, .. }) => assert_eq!(position, 0),
        other => panic!("expected a typed Type violation, got {other:?}"),
    }
}

/// Wrong shape: a scalar value (element count 1) crosses a boundary that
/// declares a 4-element device-view slot.
#[test]
fn boundary_abi_rejects_wrong_element_count() {
    let result = validate_case(|types| {
        let element = f32(types);
        (
            view(types, element, 4),
            f32(types),
            vec![call_stmt(
                1,
                Some(30),
                vec![MirOperand::Place(MirPlace::local(MirLocalId(20)))],
            )],
        )
    });
    match result {
        Err(BoundaryAbiError::Shape {
            expected_count: 4,
            found_count: Some(1),
            ..
        }) => {}
        other => panic!("expected a typed Shape violation, got {other:?}"),
    }
}

/// Wrong lifetime: the kernel's output is written into a host function
/// parameter — the region's output escapes into caller-owned storage. The
/// argument is a literal (constants skip the exact-type check by design), so
/// only the lifetime rule fires.
#[test]
fn boundary_abi_rejects_output_into_host_parameter() {
    let result = validate_case(|types| {
        (
            f32(types),
            f32(types),
            vec![call_stmt(
                1,
                Some(20),
                vec![MirOperand::Constant(MirConstant::Float(1.0))],
            )],
        )
    });
    match result {
        Err(BoundaryAbiError::Lifetime { parameter, .. }) => assert_eq!(parameter, MirLocalId(20)),
        other => panic!("expected a typed Lifetime violation, got {other:?}"),
    }
}

/// Mutation of a read-only resource: the call writes its output into the same
/// storage as its input argument — the device input slot is read-only.
#[test]
fn boundary_abi_rejects_in_place_mutation() {
    let result = validate_case(|types| {
        (
            f32(types),
            f32(types),
            vec![call_stmt(
                1,
                Some(30),
                vec![MirOperand::Place(MirPlace::local(MirLocalId(30)))],
            )],
        )
    });
    match result {
        Err(BoundaryAbiError::Mutation { local, .. }) => assert_eq!(local, MirLocalId(30)),
        other => panic!("expected a typed Mutation violation, got {other:?}"),
    }
}

/// Observation of unlaunched work: a second call passes the first call's
/// output (the kernel's region result) back in as an input — the program's
/// single launch for the kernel is both producer and consumer.
#[test]
fn boundary_abi_rejects_observing_own_output() {
    let result = validate_case(|types| {
        (
            f32(types),
            f32(types),
            vec![
                // call 1: local 30 = scale(20)
                call_stmt(
                    1,
                    Some(30),
                    vec![MirOperand::Place(MirPlace::local(MirLocalId(20)))],
                ),
                // call 2: scale(local 30) — local 30 is call 1's output.
                call_stmt(
                    1,
                    Some(31),
                    vec![MirOperand::Place(MirPlace::local(MirLocalId(30)))],
                ),
            ],
        )
    });
    match result {
        Err(BoundaryAbiError::Observation { local, .. }) => assert_eq!(local, MirLocalId(30)),
        other => panic!("expected a typed Observation violation, got {other:?}"),
    }
}

// ── DDCP3-U5 (gate 6): unsupported callsite shapes ─────────────────────────

/// Red: a boundary call through a `Definition` callee — a callee the direct
/// region-call ABI cannot lower to the prepared-region symbol. The lane
/// emits exactly ONE boundary form (the direct call to the region symbol
/// with bounded numeric fields); a callsite shape it cannot map to that form
/// is rejected before emission (never a module that calls a never-emitted
/// kernel body).
#[test]
fn boundary_abi_rejects_definition_callee_callsite_shape() {
    let mut types = TypeTable::default();
    let f32_ty = f32(&mut types);
    let program = MirProgram {
        functions: vec![
            MirFunction {
                source: Some(radix_hir::DefId(1)),
                ..kernel(1, vec![param(10, f32_ty)], f32_ty)
            },
            host_with_calls(
                2,
                vec![param(20, f32_ty)],
                vec![local(30, f32_ty)],
                vec![MirStatement {
                    kind: MirStatementKind::Call {
                        destination: Some(MirPlace::local(MirLocalId(30))),
                        callee: MirCallee::Definition {
                            source: radix_hir::DefId(1),
                            type_args: Vec::new(),
                        },
                        args: vec![MirOperand::Place(MirPlace::local(MirLocalId(20)))],
                    },
                    span: Span::default(),
                }],
                f32_ty,
            ),
        ],
    };
    let context = MirValidationContext::new(&types);
    let calls = derive_boundary_calls(&program, &[MirFunctionId(1)]);

    // The Definition callee resolves to the kernel, so the boundary derives
    // one call through it — and the gate rejects the unsupported shape.
    assert_eq!(calls.len(), 1);
    match validate_boundary_abi(&program, &calls, &context) {
        Err(BoundaryAbiError::UnlowerableCallsiteShape { kernel, .. }) => {
            assert_eq!(kernel, MirFunctionId(1));
        }
        other => panic!("expected a CallsiteShape violation, got {other:?}"),
    }
}

/// Green: a function-constant value callee (`call kernel(_0)` lowered as a
/// value) IS a direct-call shape — the direct region-call ABI lowers it, so
/// the callsite-shape check admits it.
#[test]
fn boundary_abi_accepts_function_constant_value_callee() {
    let mut types = TypeTable::default();
    let f32_ty = f32(&mut types);
    let mut program = MirProgram {
        functions: vec![kernel(1, vec![param(10, f32_ty)], f32_ty)],
    };
    program.functions.push(host_with_calls(
        2,
        vec![param(20, f32_ty)],
        vec![local(30, f32_ty)],
        vec![MirStatement {
            kind: MirStatementKind::Call {
                destination: Some(MirPlace::local(MirLocalId(30))),
                callee: MirCallee::Value(MirOperand::Constant(MirConstant::Function(
                    MirFunctionId(1),
                ))),
                args: vec![MirOperand::Place(MirPlace::local(MirLocalId(20)))],
            },
            span: Span::default(),
        }],
        f32_ty,
    ));
    let context = MirValidationContext::new(&types);
    let calls = derive_boundary_calls(&program, &[MirFunctionId(1)]);
    assert_eq!(calls.len(), 1);
    validate_boundary_abi(&program, &calls, &context)
        .expect("the function-constant callee is valid");
}

/// The versioned call-ABI surface: the boundary carries its own ratchet (the
/// faber wire seam asserts it rides the accepted wire version), and the
/// no-kernel partition still derives with the versioned surface.
#[test]
fn boundary_abi_version_is_carried() {
    assert_eq!(HOST_CALL_ABI_VERSION, 8);
    let types = TypeTable::default();
    let program = MirProgram::new();
    let validated = ValidatedMir::new(program, MirValidationContext::new(&types))
        .expect("empty program validates");
    let partition = crate::host_partition::derive_host_partition(&validated, &Interner::default())
        .expect("empty partition derives");
    assert!(!partition.has_device());
    assert_eq!(partition.call_abi_version, HOST_CALL_ABI_VERSION);
    assert!(partition.boundary_calls.is_empty());
}

// ── NGAB1-U4: batch compatible call shapes ────────────────────────────────
//
// After the first accepted call shape (the scalar), compatible shapes batch
// through the same ABI without new mechanisms. The gate's type/shape checks
// dispatch on the layout kind (`MirLayoutKind::Scalar | DeviceView`) — the
// extensible variant table, never per-shape special-casing (R3) — so the
// vector and 2D shapes resolve through the shared extent synthesis, and the
// prepared region is already the call's submission unit (NGAB0-R1), carrying
// the batch's kernels rather than one ABI row per kernel.

/// Green: a 4-element vector call shape crosses an exact 4-element boundary —
/// the shape gate resolves the view's literal extent through the layout
/// dispatch and accepts the carried count.
#[test]
fn boundary_abi_accepts_exact_vector_call() {
    let result = validate_case(|types| {
        let element = f32(types);
        (
            view(types, element, 4),
            view(types, element, 4),
            vec![call_stmt(
                1,
                Some(30),
                vec![MirOperand::Place(MirPlace::local(MirLocalId(20)))],
            )],
        )
    });
    result.expect("the exact 4-element vector call is valid");
}

/// Red: a 2-element vector crossing a 4-element vector boundary is a shape
/// violation — the count check is exact over the carried extents.
#[test]
fn boundary_abi_rejects_wrong_vector_shape() {
    let result = validate_case(|types| {
        let element = f32(types);
        (
            view(types, element, 4),
            view(types, element, 2),
            vec![call_stmt(
                1,
                Some(30),
                vec![MirOperand::Place(MirPlace::local(MirLocalId(20)))],
            )],
        )
    });
    match result {
        Err(BoundaryAbiError::Shape {
            expected_count: 4,
            found_count: Some(2),
            ..
        }) => {}
        other => panic!("expected a typed Shape violation, got {other:?}"),
    }
}

/// Green: a [2,3] 2D tensor call shape — the element-count check resolves the
/// rank-2 literal shape through the shared extent synthesis (`IndexExpr::Tuple`
/// → product) with no 2D-specific code in the boundary.
#[test]
fn boundary_abi_accepts_2d_tensor_call() {
    let result = validate_case(|types| {
        let element = f32(types);
        (
            view2d(types, element, 2, 3),
            view2d(types, element, 2, 3),
            vec![call_stmt(
                1,
                Some(30),
                vec![MirOperand::Place(MirPlace::local(MirLocalId(20)))],
            )],
        )
    });
    result.expect("the exact [2,3] 2D tensor call is valid");
}

/// Red: a [2,3] boundary (6 elements) crossed by a 4-element value is a shape
/// violation whose expected count is the resolved 2D extent.
#[test]
fn boundary_abi_rejects_2d_shape_mismatch() {
    let result = validate_case(|types| {
        let element = f32(types);
        (
            view2d(types, element, 2, 3),
            view(types, element, 4),
            vec![call_stmt(
                1,
                Some(30),
                vec![MirOperand::Place(MirPlace::local(MirLocalId(20)))],
            )],
        )
    });
    match result {
        Err(BoundaryAbiError::Shape {
            expected_count: 6,
            found_count: Some(4),
            ..
        }) => {}
        other => panic!("expected a typed Shape violation, got {other:?}"),
    }
}

/// Green: two-kernel composition batches through ONE prepared submission
/// region — one boundary call whose region carries the batch's kernels in
/// invocation order (NGAB0-R1: the region is the call's submission unit, not
/// the kernel; never per-kernel ABI rows). The composition's value-flow — the
/// first kernel's output feeding the second kernel's input — is legal: the
/// observation rule is per-kernel, and a cross-kernel feed observes no
/// unlaunched work.
#[test]
fn boundary_abi_accepts_two_kernel_composition_region() {
    let mut types = TypeTable::default();
    let f32_ty = f32(&mut types);
    let program = MirProgram {
        functions: vec![
            kernel(1, vec![param(10, f32_ty)], f32_ty),
            kernel(3, vec![param(12, f32_ty)], f32_ty),
            host_with_calls(
                2,
                vec![param(20, f32_ty)],
                vec![local(30, f32_ty), local(31, f32_ty)],
                vec![
                    // scale_kernel(x) → _30
                    call_stmt(
                        1,
                        Some(30),
                        vec![MirOperand::Place(MirPlace::local(MirLocalId(20)))],
                    ),
                    // offset_kernel(_30) → _31 — the composition's feed.
                    call_stmt(
                        3,
                        Some(31),
                        vec![MirOperand::Place(MirPlace::local(MirLocalId(30)))],
                    ),
                ],
                f32_ty,
            ),
        ],
    };
    let context = MirValidationContext::new(&types);
    let calls = derive_boundary_calls(&program, &[MirFunctionId(1), MirFunctionId(3)]);

    // One host call → one prepared region containing the batch's two kernels,
    // in invocation order — no per-kernel ABI rows.
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].host, MirFunctionId(2));
    assert_eq!(
        calls[0].region.kernels,
        vec![MirFunctionId(1), MirFunctionId(3)],
        "the batch's kernels ride one region, varia invocation order"
    );
    assert!(!calls[0].region.is_minimal());

    validate_boundary_abi(&program, &calls, &context).expect("the two-kernel composition is valid");
}

/// Red: a two-kernel host still enforces the boundary rules — a call inside
/// the composition that writes its output into its own read-only input
/// storage is rejected with the typed Mutation class (batched shapes get no
/// loosening).
#[test]
fn boundary_abi_rejects_mutation_inside_two_kernel_composition() {
    let mut types = TypeTable::default();
    let f32_ty = f32(&mut types);
    let program = MirProgram {
        functions: vec![
            kernel(1, vec![param(10, f32_ty)], f32_ty),
            kernel(3, vec![param(12, f32_ty)], f32_ty),
            host_with_calls(
                2,
                vec![param(20, f32_ty)],
                vec![local(30, f32_ty), local(31, f32_ty)],
                vec![
                    // scale_kernel(x) → _30
                    call_stmt(
                        1,
                        Some(30),
                        vec![MirOperand::Place(MirPlace::local(MirLocalId(20)))],
                    ),
                    // offset_kernel(_30) → _30 — in-place, mutates the input.
                    call_stmt(
                        3,
                        Some(30),
                        vec![MirOperand::Place(MirPlace::local(MirLocalId(30)))],
                    ),
                ],
                f32_ty,
            ),
        ],
    };
    let context = MirValidationContext::new(&types);
    let calls = derive_boundary_calls(&program, &[MirFunctionId(1), MirFunctionId(3)]);
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].region.kernels,
        vec![MirFunctionId(1), MirFunctionId(3)]
    );

    match validate_boundary_abi(&program, &calls, &context) {
        Err(BoundaryAbiError::Mutation { local, .. }) => assert_eq!(local, MirLocalId(30)),
        other => panic!("expected a typed Mutation violation, got {other:?}"),
    }
}
