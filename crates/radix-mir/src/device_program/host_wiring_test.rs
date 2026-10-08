//! NJ1 proofs: the host call-site collector (`collect_host_wiring`).
//!
//! The fixtures are hand-built MIR (the same shape `host_partition_test.rs`
//! and `constructor_test.rs` use): `radix-mir` has no source frontend, so a
//! real-source lowering check (the Temp-versus-Place forms) cannot live here.

use super::*;
use crate::{
    MirBlock, MirBlockId, MirLocal, MirParam, MirParamMode, MirProjection, MirTemp, MirTerminator,
    MirTerminatorKind, MirType,
};
use radix_lexer::{Interner, Span};
use radix_types::{NumericWidth, Primitive, TypeTable};

const COLLIGE: MirFunctionId = MirFunctionId(1);
const RECOLLIGE: MirFunctionId = MirFunctionId(2);
const HOST: MirFunctionId = MirFunctionId(3);
const HOST_B: MirFunctionId = MirFunctionId(4);

fn f32_ty() -> MirType {
    let mut types = TypeTable::new();
    MirType::semantic(types.sized_numeric(Primitive::Fractus, NumericWidth::F32))
}

/// A compute kernel whose parameters carry `names`; parameter `i` is local
/// `id * 10 + i`.
fn kernel(id: MirFunctionId, names: &[&str], interner: &mut Interner) -> MirFunction {
    let ty = f32_ty();
    let params: Vec<MirParam> = names
        .iter()
        .enumerate()
        .map(|(index, name)| MirParam {
            local: MirLocalId(id.0 * 10 + u32::try_from(index).unwrap()),
            name: Some(interner.intern(name)),
            ty,
            mode: MirParamMode::Owned,
            span: Span::default(),
        })
        .collect();
    let locals = params
        .iter()
        .map(|param| MirLocal {
            id: param.local,
            name: param.name,
            ty,
            mutable: true,
            span: Span::default(),
        })
        .collect();
    function(
        id,
        params,
        locals,
        Vec::new(),
        Some(crate::MirKernelShaderStage::Compute),
    )
}

fn function(
    id: MirFunctionId,
    params: Vec<MirParam>,
    locals: Vec<MirLocal>,
    statements: Vec<MirStatement>,
    shader_stage: Option<crate::MirKernelShaderStage>,
) -> MirFunction {
    MirFunction {
        id,
        source: None,
        name: None,
        params,
        locals,
        temps: vec![MirTemp {
            id: MirTempId(0),
            ty: f32_ty(),
            span: Span::default(),
        }],
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements,
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(None),
                span: Span::default(),
            },
            span: Span::default(),
        }],
        return_ty: f32_ty(),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage,
        span: Span::default(),
    }
}

fn host(id: MirFunctionId, statements: Vec<MirStatement>) -> MirFunction {
    function(id, Vec::new(), Vec::new(), statements, None)
}

fn local(id: u32) -> MirPlace {
    MirPlace::local(MirLocalId(id))
}

fn arg(id: u32) -> MirOperand {
    MirOperand::Place(local(id))
}

fn call(
    kernel: MirFunctionId,
    destination: Option<MirPlace>,
    args: Vec<MirOperand>,
) -> MirStatement {
    MirStatement {
        kind: MirStatementKind::Call {
            destination,
            callee: MirCallee::Function(kernel),
            args,
        },
        span: Span::default(),
    }
}

fn value(function: MirFunctionId, local: u32) -> HostValue {
    HostValue {
        function,
        base: HostBase::Local(MirLocalId(local)),
    }
}

fn selected() -> BTreeSet<MirFunctionId> {
    BTreeSet::from([COLLIGE, RECOLLIGE])
}

/// The chain program: `collige(a, medius)` then `recollige(medius, result)`.
/// Locals: a = 1, medius = 2, result = 3.
fn chain_program(interner: &mut Interner, collige: &[&str], recollige: &[&str]) -> MirProgram {
    MirProgram {
        functions: vec![
            kernel(COLLIGE, collige, interner),
            kernel(RECOLLIGE, recollige, interner),
            host(
                HOST,
                vec![
                    call(COLLIGE, None, vec![arg(1), arg(2)]),
                    call(RECOLLIGE, None, vec![arg(2), arg(3)]),
                ],
            ),
        ],
    }
}

#[test]
fn host_wiring_chain_value_is_one_set_for_output_and_next_input() {
    let mut interner = Interner::new();
    let program = chain_program(&mut interner, &["src", "medius"], &["medius", "result"]);
    let wiring = collect_host_wiring(&program, &selected());

    let produced = Slot::param(COLLIGE, 1);
    let consumed = Slot::param(RECOLLIGE, 0);
    let sites = wiring.sites(produced).expect("the output slot has sites");
    assert_eq!(sites.values, BTreeSet::from([value(HOST, 2)]));
    assert!(!sites.unknown);
    assert_eq!(wiring.sites(consumed).unwrap().values, sites.values);
    assert!(wiring.same_host_values(produced, consumed));
    // Different host values never join.
    assert!(!wiring.same_host_values(Slot::param(COLLIGE, 0), consumed));
    assert!(!wiring.same_host_values(produced, Slot::param(RECOLLIGE, 1)));
}

#[test]
fn host_wiring_resolves_definition_callees_to_the_selected_kernel() {
    // Real lowering calls a named kernel by its source `DefId`
    // (`MirCallee::Definition`), not by `MirCallee::Function`.
    let mut interner = Interner::new();
    let mut program = chain_program(&mut interner, &["src", "medius"], &["in", "result"]);
    program.functions[0].source = Some(radix_hir::DefId(7));
    program.functions[1].source = Some(radix_hir::DefId(8));
    for statement in &mut program.functions[2].blocks[0].statements {
        let MirStatementKind::Call { callee, .. } = &mut statement.kind else {
            unreachable!("the host holds only calls");
        };
        let MirCallee::Function(id) = callee else {
            unreachable!("the fixture calls by function id");
        };
        *callee = MirCallee::definition(radix_hir::DefId(if *id == COLLIGE { 7 } else { 8 }));
    }
    let wiring = collect_host_wiring(&program, &selected());
    assert!(!wiring.is_opaque(COLLIGE));
    assert!(wiring.same_host_values(Slot::param(COLLIGE, 1), Slot::param(RECOLLIGE, 0)));
}

#[test]
fn host_wiring_equal_names_with_different_host_values_do_not_join() {
    // Both kernels name their parameter `medius`; the host passes different
    // locals, so the names must not matter.
    let mut interner = Interner::new();
    let program = MirProgram {
        functions: vec![
            kernel(COLLIGE, &["src", "medius"], &mut interner),
            kernel(RECOLLIGE, &["medius", "result"], &mut interner),
            host(
                HOST,
                vec![
                    call(COLLIGE, None, vec![arg(1), arg(2)]),
                    call(RECOLLIGE, None, vec![arg(5), arg(3)]),
                ],
            ),
        ],
    };
    let wiring = collect_host_wiring(&program, &selected());
    assert!(!wiring.same_host_values(Slot::param(COLLIGE, 1), Slot::param(RECOLLIGE, 0)));
}

#[test]
fn host_wiring_several_call_sites_accumulate_and_partial_overlap_is_no_join() {
    // `collige` is launched twice (outputs local 2, then local 4); `recollige`
    // consumes only local 2.
    let mut interner = Interner::new();
    let program = MirProgram {
        functions: vec![
            kernel(COLLIGE, &["src", "dst"], &mut interner),
            kernel(RECOLLIGE, &["dst", "result"], &mut interner),
            host(
                HOST,
                vec![
                    call(COLLIGE, None, vec![arg(1), arg(2)]),
                    call(RECOLLIGE, None, vec![arg(2), arg(3)]),
                ],
            ),
            host(HOST_B, vec![call(COLLIGE, None, vec![arg(1), arg(4)])]),
        ],
    };
    let wiring = collect_host_wiring(&program, &selected());

    let produced = Slot::param(COLLIGE, 1);
    let consumed = Slot::param(RECOLLIGE, 0);
    assert_eq!(
        wiring.sites(produced).unwrap().values,
        BTreeSet::from([value(HOST, 2), value(HOST_B, 4)])
    );
    assert_eq!(
        wiring.sites(consumed).unwrap().values,
        BTreeSet::from([value(HOST, 2)])
    );
    assert!(!wiring.same_host_values(produced, consumed));
    // A slot is equal to itself when it is known and non-empty.
    assert!(wiring.same_host_values(produced, produced));
    // Same local id in a different host function is a different value.
    assert_ne!(value(HOST, 2), value(HOST_B, 2));
}

#[test]
fn host_wiring_return_form_call_records_the_destination() {
    // result = collige(a); recollige(result).
    let mut interner = Interner::new();
    let program = MirProgram {
        functions: vec![
            kernel(COLLIGE, &["src"], &mut interner),
            kernel(RECOLLIGE, &["medius"], &mut interner),
            host(
                HOST,
                vec![
                    call(COLLIGE, Some(local(2)), vec![arg(1)]),
                    call(RECOLLIGE, None, vec![arg(2)]),
                ],
            ),
        ],
    };
    let wiring = collect_host_wiring(&program, &selected());

    let result = Slot::result(COLLIGE);
    assert_eq!(
        wiring.sites(result).unwrap().values,
        BTreeSet::from([value(HOST, 2)])
    );
    assert!(wiring.same_host_values(result, Slot::param(RECOLLIGE, 0)));
    // The unconsumed result of `recollige` was discarded: unknown.
    assert!(wiring.sites(Slot::result(RECOLLIGE)).unwrap().unknown);
}

#[test]
fn host_wiring_temps_are_host_values() {
    // The place form and the operand form of one temp name the same value.
    let mut interner = Interner::new();
    let program = MirProgram {
        functions: vec![
            kernel(COLLIGE, &["src"], &mut interner),
            kernel(RECOLLIGE, &["medius"], &mut interner),
            host(
                HOST,
                vec![
                    call(COLLIGE, Some(MirPlace::temp(MirTempId(0))), vec![arg(1)]),
                    call(RECOLLIGE, None, vec![MirOperand::Temp(MirTempId(0))]),
                ],
            ),
        ],
    };
    let wiring = collect_host_wiring(&program, &selected());
    assert!(wiring.same_host_values(Slot::result(COLLIGE), Slot::param(RECOLLIGE, 0)));
    assert_eq!(
        wiring.sites(Slot::param(RECOLLIGE, 0)).unwrap().values,
        BTreeSet::from([HostValue {
            function: HOST,
            base: HostBase::Temp(MirTempId(0)),
        }])
    );
}

#[test]
fn host_wiring_projection_and_constant_arguments_are_unknown() {
    let mut interner = Interner::new();
    let field = interner.intern("field");
    let mut projected = local(1);
    projected.projections.push(MirProjection::Field(field));
    let program = MirProgram {
        functions: vec![
            kernel(COLLIGE, &["a", "b", "c"], &mut interner),
            kernel(RECOLLIGE, &["a", "b", "c"], &mut interner),
            host(
                HOST,
                vec![
                    call(
                        COLLIGE,
                        None,
                        vec![
                            MirOperand::Place(projected.clone()),
                            MirOperand::Constant(MirConstant::Int(7)),
                            arg(2),
                        ],
                    ),
                    call(
                        RECOLLIGE,
                        None,
                        vec![
                            MirOperand::Place(projected),
                            MirOperand::Constant(MirConstant::Int(7)),
                            arg(2),
                        ],
                    ),
                ],
            ),
        ],
    };
    let wiring = collect_host_wiring(&program, &selected());

    for position in 0..2 {
        let slot = Slot::param(COLLIGE, position);
        assert!(wiring.sites(slot).unwrap().unknown);
        assert!(wiring.sites(slot).unwrap().values.is_empty());
        // Even identical unknown sites never join.
        assert!(!wiring.same_host_values(slot, Slot::param(RECOLLIGE, position)));
    }
    // A plain local alongside them still joins.
    assert!(wiring.same_host_values(Slot::param(COLLIGE, 2), Slot::param(RECOLLIGE, 2)));
}

#[test]
fn host_wiring_one_unknown_call_site_poisons_the_slot() {
    let mut interner = Interner::new();
    let program = MirProgram {
        functions: vec![
            kernel(COLLIGE, &["src"], &mut interner),
            kernel(RECOLLIGE, &["src"], &mut interner),
            host(
                HOST,
                vec![
                    call(COLLIGE, None, vec![arg(1)]),
                    call(
                        COLLIGE,
                        None,
                        vec![MirOperand::Constant(MirConstant::Int(0))],
                    ),
                    call(RECOLLIGE, None, vec![arg(1)]),
                ],
            ),
        ],
    };
    let wiring = collect_host_wiring(&program, &selected());
    assert!(!wiring.same_host_values(Slot::param(COLLIGE, 0), Slot::param(RECOLLIGE, 0)));
}

#[test]
fn host_wiring_indirect_reach_makes_the_kernel_opaque() {
    let mut interner = Interner::new();
    let program = MirProgram {
        functions: vec![
            kernel(COLLIGE, &["src"], &mut interner),
            kernel(RECOLLIGE, &["src"], &mut interner),
            host(
                HOST,
                vec![
                    call(COLLIGE, None, vec![arg(1)]),
                    // `recollige` reached through a function value, plus a
                    // direct call that alone would have joined.
                    MirStatement {
                        kind: MirStatementKind::Call {
                            destination: None,
                            callee: MirCallee::Value(MirOperand::Constant(MirConstant::Function(
                                RECOLLIGE,
                            ))),
                            args: vec![arg(1)],
                        },
                        span: Span::default(),
                    },
                ],
            ),
        ],
    };
    let wiring = collect_host_wiring(&program, &selected());
    // A constant-function callee is a direct call (the constructor treats it
    // as one), so it records; passing a kernel as a VALUE is the opaque case.
    assert!(wiring.same_host_values(Slot::param(COLLIGE, 0), Slot::param(RECOLLIGE, 0)));
    assert!(!wiring.is_opaque(RECOLLIGE));

    let opaque_program = MirProgram {
        functions: vec![
            kernel(COLLIGE, &["src"], &mut interner),
            kernel(RECOLLIGE, &["src"], &mut interner),
            host(
                HOST,
                vec![
                    call(COLLIGE, None, vec![arg(1)]),
                    call(RECOLLIGE, None, vec![arg(1)]),
                    // `recollige` escapes as an argument value.
                    call(
                        COLLIGE,
                        None,
                        vec![MirOperand::Constant(MirConstant::Function(RECOLLIGE))],
                    ),
                ],
            ),
        ],
    };
    let wiring = collect_host_wiring(&opaque_program, &selected());
    assert!(wiring.is_opaque(RECOLLIGE));
    assert!(!wiring.is_opaque(COLLIGE));
    assert!(!wiring.same_host_values(Slot::param(COLLIGE, 0), Slot::param(RECOLLIGE, 0)));
}

#[test]
fn host_wiring_failable_call_makes_the_kernel_opaque() {
    let mut interner = Interner::new();
    let mut try_host = host(HOST, vec![call(COLLIGE, None, vec![arg(1)])]);
    try_host.blocks[0].terminator.kind = MirTerminatorKind::TryCall {
        destination: None,
        callee: MirCallee::Function(RECOLLIGE),
        args: vec![arg(1)],
        ok_block: MirBlockId(0),
        error_place: local(9),
        error_block: MirBlockId(0),
    };
    let program = MirProgram {
        functions: vec![
            kernel(COLLIGE, &["src"], &mut interner),
            kernel(RECOLLIGE, &["src"], &mut interner),
            try_host,
        ],
    };
    let wiring = collect_host_wiring(&program, &selected());
    assert!(wiring.is_opaque(RECOLLIGE));
    assert!(!wiring.same_host_values(Slot::param(COLLIGE, 0), Slot::param(RECOLLIGE, 0)));
}

#[test]
fn host_wiring_ignores_device_callers_and_unselected_kernels() {
    let mut interner = Interner::new();
    let device_caller = function(
        MirFunctionId(9),
        Vec::new(),
        Vec::new(),
        vec![call(COLLIGE, None, vec![arg(77)])],
        Some(crate::MirKernelShaderStage::Compute),
    );
    let program = MirProgram {
        functions: vec![
            kernel(COLLIGE, &["src"], &mut interner),
            kernel(RECOLLIGE, &["src"], &mut interner),
            device_caller,
            host(
                HOST,
                vec![
                    call(COLLIGE, None, vec![arg(1)]),
                    call(MirFunctionId(50), None, vec![arg(1)]),
                ],
            ),
        ],
    };
    let wiring = collect_host_wiring(&program, &selected());
    assert_eq!(
        wiring.sites(Slot::param(COLLIGE, 0)).unwrap().values,
        BTreeSet::from([value(HOST, 1)])
    );
    assert!(wiring.sites(Slot::param(MirFunctionId(50), 0)).is_none());
}

#[test]
fn host_wiring_is_invariant_under_parameter_renaming() {
    // The same program with every kernel parameter renamed (fresh symbols,
    // including a name collision in the second build) collects the identical
    // structure: names are not part of the key.
    let mut first_interner = Interner::new();
    let first = chain_program(
        &mut first_interner,
        &["src", "medius"],
        &["medius", "result"],
    );
    let mut second_interner = Interner::new();
    // Burn symbols so ids differ from the first interner.
    for filler in ["x0", "x1", "x2"] {
        second_interner.intern(filler);
    }
    let second = chain_program(
        &mut second_interner,
        &["alpha", "beta"],
        &["gamma", "alpha"],
    );

    assert_ne!(
        first.functions[0].params[1].name,
        second.functions[0].params[1].name
    );
    let a = collect_host_wiring(&first, &selected());
    let b = collect_host_wiring(&second, &selected());
    assert_eq!(a, b);
    assert!(b.same_host_values(Slot::param(COLLIGE, 1), Slot::param(RECOLLIGE, 0)));
}

#[test]
fn host_wiring_slot_maps_a_parameter_local_to_its_position() {
    let mut interner = Interner::new();
    let kernel = kernel(RECOLLIGE, &["medius", "result"], &mut interner);
    assert_eq!(
        Slot::of_param_local(&kernel, MirLocalId(21)),
        Some(Slot::param(RECOLLIGE, 1))
    );
    assert_eq!(Slot::of_param_local(&kernel, MirLocalId(99)), None);
}
