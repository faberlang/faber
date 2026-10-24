//! Tests for runner capability classification and runtime dispatch ledger.
//!
//! These tests prove that `classify_runner_lowerability` accepts or
//! rejects MIR programs based on structural capability (fixtures,
//! intrinsic support, terminator gaps) and that every runtime ABI
//! family has an explicit dispatch status in the runner ledger.

use super::{RunnerDispatchStatus, classify_runner_lowerability, runner_dispatch_status};
use radix_lexer::Span;
use radix_mir::{
    Lowerability, MirBlock, MirBlockId, MirFilumCoordinate, MirFilumForm, MirFilumReduceOp,
    MirFunction, MirFunctionId, MirIntrinsic, MirLocal, MirLocalId, MirOperand, MirPlace,
    MirPlaceBase, MirProgram, MirProjection, MirReduceOp, MirRuntimeAbiFamily, MirRuntimeCall,
    MirStatement, MirStatementKind, MirTemp, MirTempId, MirTerminator, MirTerminatorKind, MirType,
    MirValidationContext, MirValue, MirValueId, MirValueKind, ValidatedMir,
};
use radix_types::CapabilityGap;
use radix_types::{IndexExpr, Primitive, Type, TypeTable};

fn test_span() -> Span {
    Span::default()
}

fn rejected_shape(lowerability: &Lowerability) -> &str {
    lowerability
        .first_gap()
        .expect("fixture is rejected")
        .shape()
}

fn ty(types: &TypeTable, primitive: Primitive) -> MirType {
    MirType::semantic(types.primitive(primitive))
}

fn empty_validation(types: &TypeTable) -> MirValidationContext<'_> {
    MirValidationContext::new(types)
}

#[test]
fn classifies_unreachable_terminator_gap() {
    let types = TypeTable::new();
    let vacuum = ty(&types, Primitive::Vacuum);
    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: Vec::new(),
            locals: Vec::new(),
            temps: Vec::new(),
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: Vec::new(),
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Unreachable,
                    span: test_span(),
                },
                span: test_span(),
            }],
            return_ty: vacuum,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: test_span(),
        }],
    };

    let validated = ValidatedMir::new(program.clone(), empty_validation(&types))
        .expect("unreachable terminator test fixture must validate");
    let lowerability = classify_runner_lowerability(&validated);

    assert_eq!(rejected_shape(&lowerability), "unreachable terminator");
}

#[test]
fn classifies_vector_lane_read_projection_as_capable() {
    // CPU matrix subset (d44fc01): vector construct/lane projection execute in
    // the runner so matrix.applica can product into register vectors.
    let mut types = TypeTable::new();
    let element = types.primitive(Primitive::Numerus);
    let width = types.intern_index(IndexExpr::Literal(3));
    let vector = MirType::semantic(types.intern(Type::Vector(element, width)));
    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: Vec::new(),
            locals: vec![MirLocal {
                id: MirLocalId(0),
                name: None,
                ty: vector,
                mutable: false,
                span: test_span(),
            }],
            temps: vec![MirTemp {
                id: MirTempId(0),
                ty: MirType::semantic(element),
                span: test_span(),
            }],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::Assign {
                        place: MirPlace::temp(MirTempId(0)),
                        value: MirValue {
                            id: MirValueId(0),
                            kind: MirValueKind::Operand(MirOperand::Place(MirPlace {
                                base: MirPlaceBase::Local(MirLocalId(0)),
                                projections: vec![MirProjection::VectorLane(0)],
                            })),
                            ty: MirType::semantic(element),
                            span: test_span(),
                        },
                    },
                    span: test_span(),
                }],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(None),
                    span: test_span(),
                },
                span: test_span(),
            }],
            return_ty: ty(&types, Primitive::Vacuum),
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: test_span(),
        }],
    };

    let validated = ValidatedMir::new(program.clone(), empty_validation(&types))
        .expect("unreachable terminator test fixture must validate");
    let lowerability = classify_runner_lowerability(&validated);

    assert_eq!(lowerability, Lowerability::Capable);
}

#[test]
fn every_runtime_family_has_a_runner_ledger_status() {
    // GUARD: runtime-boundary ledger runner column. Exhaustiveness is also
    // compile-enforced by `runner_dispatch_status`. This table pins every
    // MirRuntimeAbiFamily to its ledger status so a reclassification (or a new
    // family) is a deliberate change visible in this test and in
    // docs/factory/stepper-alignment/stepper-runtime-ledger.md.
    use RunnerDispatchStatus::*;
    let expected: [(MirRuntimeAbiFamily, RunnerDispatchStatus); 25] = [
        (MirRuntimeAbiFamily::RuntimeDiagnostic, Implement),
        (MirRuntimeAbiFamily::RuntimeAssert, Implement),
        (MirRuntimeAbiFamily::RuntimeFormat, Implement),
        (MirRuntimeAbiFamily::RuntimeConvert, Implement),
        (MirRuntimeAbiFamily::RuntimeCollection, Implement),
        (MirRuntimeAbiFamily::RuntimeAtomic, IntentionalReject),
        (MirRuntimeAbiFamily::RuntimePanic, Implement),
        (MirRuntimeAbiFamily::RuntimeReadLine, Implement),
        (MirRuntimeAbiFamily::RuntimeProvider, HostDelegate),
        (MirRuntimeAbiFamily::RuntimeCede, Implement),
        (MirRuntimeAbiFamily::RuntimeDeviceBlock, Implement),
        (MirRuntimeAbiFamily::RuntimeCursorStream, Implement),
        (MirRuntimeAbiFamily::RuntimeSermo, Implement),
        // S5-U1: gradient handles dispatch in-process via runtime.rs
        // eval_gradient (opaque handle storage map); no longer deferred debt.
        (MirRuntimeAbiFamily::RuntimeGradient, Implement),
        // S5-U3: `est <type>` variant-tag checks dispatch in-process via
        // runtime.rs eval_type_check (tag carried by the Value variant).
        (MirRuntimeAbiFamily::RuntimeTypeCheck, Implement),
        (MirRuntimeAbiFamily::RuntimeTensorShapeCheck, Implement),
        (MirRuntimeAbiFamily::RuntimeTensorShapeSize, Implement),
        (MirRuntimeAbiFamily::RuntimeTensorLimes, Implement),
        // SFR-5: the filum sum declaration is device-lane only; the host script
        // runner rejects it by target policy.
        (MirRuntimeAbiFamily::RuntimeFilumReduce, IntentionalReject),
        (MirRuntimeAbiFamily::RuntimeScan, Implement),
        (MirRuntimeAbiFamily::RuntimeDuplicate, Implement),
        // K4: elementwise operator lift over tensors; the runner is the
        // reference, other lanes reject it.
        (MirRuntimeAbiFamily::RuntimeTensorLift, Implement),
        // IEEE pow over fractus: the runner evaluates the host libm pow.
        (MirRuntimeAbiFamily::RuntimePow, Implement),
        // A deferred vector op is expanded before any executor runs, so no lane
        // dispatches this family.
        (MirRuntimeAbiFamily::RuntimeVectorOp, IntentionalReject),
        (MirRuntimeAbiFamily::RuntimeGpuBuiltin, IntentionalReject),
    ];
    assert_eq!(
        expected.len(),
        25,
        "ledger must cover all twenty-five runtime families"
    );
    for (family, want) in expected {
        assert_eq!(
            runner_dispatch_status(family),
            want,
            "ledger status for {family:?}"
        );
    }
}

/// A filum reduction declaration is device-lane only: the host script runner
/// rejects it by target policy (its ledger status is `IntentionalReject`), so
/// the structural verdict is a policy rejection, not a missing shape.
#[test]
fn filum_reduce_is_a_target_policy_rejection() {
    let types = TypeTable::new();
    let numerus = ty(&types, Primitive::Numerus);
    let fractus = ty(&types, Primitive::Fractus);
    let filum = MirFilumReduceOp {
        op: MirReduceOp::Sum,
        form: MirFilumForm::Declared,
        term_ty: numerus,
        group: None,
        filum_binder: None,
        term_binder: MirLocalId(1),
        term_binder_name: None,
        coordinates: vec![MirFilumCoordinate {
            local: MirLocalId(2),
            name: None,
        }],
        lane_width: 32,
        term_body: Vec::new(),
        term_captures: None,
        term_result: MirLocalId(3),
    };
    let local = |id: u32, ty: MirType| MirLocal {
        id: MirLocalId(id),
        name: None,
        ty,
        mutable: true,
        span: test_span(),
    };
    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: Vec::new(),
            locals: vec![
                local(0, fractus),
                local(1, numerus),
                local(2, numerus),
                local(3, numerus),
            ],
            temps: Vec::new(),
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::RuntimeCall {
                        destination: Some(MirPlace::local(MirLocalId(0))),
                        call: MirRuntimeCall {
                            intrinsic: MirIntrinsic::FilumReduce(filum),
                            args: vec![MirOperand::Place(MirPlace::local(MirLocalId(1)))],
                            return_ty: fractus,
                        },
                    },
                    span: test_span(),
                }],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(None),
                    span: test_span(),
                },
                span: test_span(),
            }],
            return_ty: ty(&types, Primitive::Vacuum),
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: test_span(),
        }],
    };

    let validated = ValidatedMir::new(program, empty_validation(&types))
        .expect("filum reduce classification fixture must validate");
    let Lowerability::Rejected(gaps) = classify_runner_lowerability(&validated) else {
        panic!("a filum reduction must be rejected in host script mode");
    };

    let [gap] = gaps.as_slice() else {
        panic!("expected exactly one gap, got {gaps:?}");
    };
    assert!(
        matches!(gap, CapabilityGap::TargetPolicyRejection { .. }),
        "filum reduce is a policy rejection, got {gap:?}"
    );
}
