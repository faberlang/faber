//! Tests for runner capability classification and runtime dispatch ledger.
//!
//! These tests prove that classify_runner_lowerability accepts or
//! rejects MIR programs based on structural capability (fixtures,
//! intrinsic support, terminator gaps) and that every runtime ABI
//! family has an explicit dispatch status in the runner ledger.

use super::{classify_runner_lowerability, runner_dispatch_status, RunnerDispatchStatus};
use crate::driver::{analyze_source, Config, Session};
use crate::mir::{
    lower_analyzed_unit_with_context, Lowerability, MirBlock, MirBlockId, MirFunction, MirFunctionId, MirLocal,
    MirLocalId, MirOperand, MirPlace, MirProgram, MirProjection, MirRuntimeAbiFamily, MirStatement, MirStatementKind,
    MirTemp, MirTempId, MirTerminator, MirTerminatorKind, MirType, MirValidationContext, MirValue, MirValueId,
    MirValueKind, ValidatedMir,
};
use crate::semantic::{IndexExpr, Primitive, TypeTable};
use crate::test_support::test_span;
use std::path::Path;

/// Analyze, lower, and structurally classify one corpus fixture.
fn classify_fixture(relative: &str) -> Lowerability {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus")
        .join(relative);
    let source = std::fs::read_to_string(&path).expect("read fixture source");
    let session = Session::new(Config::default().with_dev_stdlib());
    let mut analysis = analyze_source(&session, "test.fab", &source).expect("fixture analyzes cleanly");
    let lowered = lower_analyzed_unit_with_context(&mut analysis).expect("fixture lowers to MIR");
    classify_runner_lowerability(&lowered.validated)
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
fn classifies_capable_program() {
    let lowerability = classify_fixture("incipit/salve-munde.fab");
    assert_eq!(lowerability, Lowerability::Capable);
}

#[test]
fn classifies_cede_intrinsic_as_capable() {
    // WHY: the runtime `Cede` handler implements yield materialization for
    // source `cede`, so cede.fab is structurally capable.
    let lowerability = classify_fixture("cede/cede.fab");
    assert_eq!(lowerability, Lowerability::Capable);
}

#[test]
fn classifies_cursor_stream_intrinsic_as_capable() {
    // WHY: Stage 4 implemented `CursorStream` materialization, so cursor.fab
    // (which lowers both Cede and CursorStream) is structurally capable.
    let lowerability = classify_fixture("cursor/cursor.fab");
    assert_eq!(lowerability, Lowerability::Capable);
}

#[test]
fn direct_driver_rejects_async_tempus_tacebit_without_import_contract() {
    // WHY: async-tempus-dormiet.fab now uses the explicit await-discard surface
    // (`tacebit`) instead of legacy await-form `cede`. This direct-driver test
    // intentionally does not install provider file interfaces, so the imported
    // async provider has no promissum carrier and must fail before MIR.
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus")
        .join("ad/async-tempus-dormiet.fab");
    let source = std::fs::read_to_string(&path).expect("read fixture source");
    let session = Session::new(Config::default().with_dev_stdlib());
    let Err(diagnostics) = analyze_source(&session, "test.fab", &source) else {
        panic!("fixture must fail without import contract");
    };
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.issue() == Some("cede_requires_promissum")),
        "expected cede_requires_promissum, got {diagnostics:?}"
    );
}

#[test]
fn classifies_tensor_to_sparsa_with_shared_carrier() {
    let lowerability = classify_fixture("sparsa/conversio.fab");
    assert_eq!(lowerability, Lowerability::Capable);
}

#[test]
fn classifies_missing_entry_function() {
    // WHY: tag.fab is a reference exemplar without incipit; the structural
    // classifier must mirror run_entry's first rejection site so the
    // structural tier never approves a program the run tier rejects.
    let lowerability = classify_fixture("tag/tag.fab");
    assert_eq!(rejected_shape(&lowerability), "no entry function in MIR program");
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
                terminator: MirTerminator { kind: MirTerminatorKind::Unreachable, span: test_span() },
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
    let vector = MirType::semantic(types.intern(crate::semantic::Type::Vector(element, width)));
    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: Vec::new(),
            locals: vec![MirLocal { id: MirLocalId(0), name: None, ty: vector, mutable: false, span: test_span() }],
            temps: vec![MirTemp { id: MirTempId(0), ty: MirType::semantic(element), span: test_span() }],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::Assign {
                        place: MirPlace::temp(MirTempId(0)),
                        value: MirValue {
                            id: MirValueId(0),
                            kind: MirValueKind::Operand(MirOperand::Place(MirPlace {
                                base: crate::mir::MirPlaceBase::Local(MirLocalId(0)),
                                projections: vec![MirProjection::VectorLane(0)],
                            })),
                            ty: MirType::semantic(element),
                            span: test_span(),
                        },
                    },
                    span: test_span(),
                }],
                terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: test_span() },
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
fn futura_fixture_is_capable_after_cede_implementation() {
    // WHY: futura.fab now consumes async work through explicit await-bind
    // morphology and no longer depends on await-form `cede`.
    let lowerability = classify_fixture("futura/futura.fab");
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
    let expected: [(MirRuntimeAbiFamily, RunnerDispatchStatus); 15] = [
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
        (MirRuntimeAbiFamily::RuntimeCursorStream, Implement),
        (MirRuntimeAbiFamily::RuntimeSermo, Implement),
        // S5-U1: gradient handles dispatch in-process via runtime.rs
        // eval_gradient (opaque handle storage map); no longer deferred debt.
        (MirRuntimeAbiFamily::RuntimeGradient, Implement),
        // S5-U3: `est <type>` variant-tag checks dispatch in-process via
        // runtime.rs eval_type_check (tag carried by the Value variant).
        (MirRuntimeAbiFamily::RuntimeTypeCheck, Implement),
        (MirRuntimeAbiFamily::RuntimeGpuBuiltin, IntentionalReject),
    ];
    assert_eq!(expected.len(), 15, "ledger must cover all fifteen runtime families");
    for (family, want) in expected {
        assert_eq!(runner_dispatch_status(family), want, "ledger status for {family:?}");
    }
}
