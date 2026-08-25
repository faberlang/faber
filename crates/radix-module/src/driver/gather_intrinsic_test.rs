//! CCI-4 driver-level tests for the row-gather intrinsic (`tensor.gather(ids)`).
//!
//! Proves the CCI-1-frozen row-gather source contract end-to-end through the
//! full language frontend (lex → parse → semantic → HIR→AIR → AIR validation
//! → MIR re-lowering) and the CPU runner, per the CTO row-6 acceptance proof
//! (mail `4971fc90`) and
//! `docs/archived/common-case-intrinsics/common-case-intrinsics-contract.md`:
//! - a typed `[V,D]` + `[T]` gather call compiles and the runner produces
//!   `[T,D]` matching row selection for **repeated and out-of-order** ids;
//! - an **out-of-range** / negative / non-literal id vector fails closed with
//!   the typed `invalid_gather_contract` diagnostic (no silent clamping);
//! - `TensorGet`/`TensorSlice` calls still compile and run unchanged
//!   (additive proof — row gather is not a rename of the indexing ops);
//! - an **empty id vector yields the empty `[0,D]` result** (the CTO-4 shape
//!   proof, carried at the contract-designated layer: the MIR `TensorGather`
//!   op + the runner). The empty case is spellable at the source level —
//!   `table.gather([])` — since the DEFER-064 `NumerusArray` policy admits an
//!   empty literal (its element type resolves to the slot's canonical
//!   `numerus` from the index position); see the CCI-1 empty-literal repair
//!. The lower-level MIR proof below drives the exact
//!   `TensorGather` op with `T = 0` as an independent runner-layer check.

use crate::air::{lower_program, AirExpr, AirTensorOp};
use crate::codegen::Target;
use crate::driver::{analyze_source, compile, AnalyzeRequest, Config, ModuleCompileRequest, Session};
use crate::mir::{
    run_entry, run_source, validate_program, BufferHost, MirAggregateFields, MirCollectionOp, MirIntrinsic, MirOperand,
    MirPlaceBase, MirRuntimeCall, MirStatementKind, MirTempId, MirValidationContext, RunSourceError, ValidatedMir,
};
use crate::semantic::{self, PassConfig};
use crate::test_support::test_session;
use radix_mir::device_program_plans::transformer_shape_signature;
use radix_mir::kernel_plan::CollectionKernelPlan;
use radix_types::{Type, TypeTable};

/// Full frontend to an AIR program plus the semantic type table the program's
/// `TypeId`s resolve against (mirrors `air/lower_test.rs`; the lane-annotated
/// path owns the typed `invalid_gather_contract` static checks).
fn lower_with_types(source: &str) -> (crate::air::AirProgram, TypeTable) {
    let lex = crate::lexer::lex(source);
    let mut parse = crate::parser::parse(lex);
    let program = parse.program.expect("parse");
    let config = PassConfig::for_target(Target::MirWasm);
    let result = semantic::analyze(&program, &config, &mut parse.interner);
    assert!(result.success(), "semantic analysis failed: {:?}", result.diagnostics);
    let semantic::SemanticResult { hir, types, radix_lanes, .. } = result;
    let air = lower_program(hir.as_ref().expect("hir"), &radix_lanes, &parse.interner, &types).expect("air lowering");
    (air, types)
}

/// The same frontend but expecting the HIR→AIR construction site to reject the
/// source with the typed gather contract diagnostic.
fn air_lower_errors(source: &str) -> Vec<crate::air::AirLowerError> {
    let lex = crate::lexer::lex(source);
    let mut parse = crate::parser::parse(lex);
    let program = parse.program.expect("parse");
    let config = PassConfig::for_target(Target::MirWasm);
    let result = semantic::analyze(&program, &config, &mut parse.interner);
    assert!(result.success(), "semantic analysis failed: {:?}", result.diagnostics);
    let semantic::SemanticResult { hir, types, radix_lanes, .. } = result;
    lower_program(hir.as_ref().expect("hir"), &radix_lanes, &parse.interner, &types)
        .expect_err("the typed gather contract must fail closed at HIR→AIR")
}

fn runner_session() -> Session {
    Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_dev_stdlib(),
    )
}

/// Extract the `TensorGather` runtime call from a lowered MIR program.
fn collection_runtime_call(mir: &crate::mir::MirProgram, op: MirCollectionOp) -> &MirRuntimeCall {
    let statement = mir.functions[0]
        .blocks
        .first()
        .and_then(|block| block.statements.last())
        .expect("collection runtime call");
    let MirStatementKind::RuntimeCall { call, .. } = &statement.kind else {
        panic!("expected tensor AIR binding to become MIR runtime call");
    };
    assert!(
        matches!(call.intrinsic, MirIntrinsic::Collection(actual) if actual == op),
        "expected {op:?}, got {:?}",
        call.intrinsic
    );
    call
}

/// Empty the id-vector aggregate of every `TensorGather` call in a MIR
/// program, returning how many constructs were emptied. The lowered ids array
/// is a `Construct` (array of int constants) referenced by the gather call's
/// trailing operand, so replacing its fields with the empty ordered list
/// drives the exact op the frontend produced with `T = 0`.
fn empty_tensor_gather_ids(program: &mut crate::mir::MirProgram) -> usize {
    let mut emptied = 0;
    for function in &mut program.functions {
        for block in &mut function.blocks {
            let ids_temps: Vec<MirTempId> = block
                .statements
                .iter()
                .filter_map(|statement| match &statement.kind {
                    MirStatementKind::RuntimeCall { call, .. }
                        if call.intrinsic == MirIntrinsic::Collection(MirCollectionOp::TensorGather) =>
                    {
                        match call.args.get(1) {
                            Some(MirOperand::Temp(id)) => Some(*id),
                            Some(MirOperand::Place(place)) => match place.base {
                                MirPlaceBase::Temp(id) => Some(id),
                                MirPlaceBase::Local(_) => None,
                            },
                            _ => None,
                        }
                    }
                    _ => None,
                })
                .collect();
            for statement in &mut block.statements {
                if let MirStatementKind::Construct { destination, aggregate } = &mut statement.kind {
                    if let MirPlaceBase::Temp(id) = destination.base {
                        if ids_temps.contains(&id) {
                            aggregate.fields = MirAggregateFields::Ordered(Vec::new());
                            emptied += 1;
                        }
                    }
                }
            }
        }
    }
    emptied
}

/// CCI-4: a typed `[V,D]` + `[T]` gather call (`T` = id-vector length) runs
/// through the full frontend and the runner produces `[T,D]` matching row
/// selection for **repeated and out-of-order** ids (CTO row 6).
#[test]
fn gather_selects_rows_for_repeated_and_out_of_order_ids() {
    // Table [4,3] with distinct rows; ids [3, 0, 2, 0] are out-of-order and
    // repeat row 0. Expected rows: row3, row0, row2, row0.
    let source = r"
incipit {
    fixum lista<f32> flat ← [1.0, 2.0, 3.0, 10.0, 20.0, 30.0, 100.0, 200.0, 300.0, 1000.0, 2000.0, 3000.0]
    fixum tf32[] seed ← vacua
    fixum tf32[4, 3] table ← seed.strue(flat, [4, 3])
    fixum tf32[4, 3] out ← table.gather([3, 0, 2, 0])
    fixum f32 v00 ← out.accipe([0, 0]) vel 0.0
    fixum f32 e00 ← 1000.0
    adfirma v00 ≡ e00
    fixum f32 v01 ← out.accipe([0, 1]) vel 0.0
    fixum f32 e01 ← 2000.0
    adfirma v01 ≡ e01
    fixum f32 v02 ← out.accipe([0, 2]) vel 0.0
    fixum f32 e02 ← 3000.0
    adfirma v02 ≡ e02
    fixum f32 v10 ← out.accipe([1, 0]) vel 0.0
    fixum f32 e10 ← 1.0
    adfirma v10 ≡ e10
    fixum f32 v11 ← out.accipe([1, 1]) vel 0.0
    fixum f32 e11 ← 2.0
    adfirma v11 ≡ e11
    fixum f32 v12 ← out.accipe([1, 2]) vel 0.0
    fixum f32 e12 ← 3.0
    adfirma v12 ≡ e12
    fixum f32 v20 ← out.accipe([2, 0]) vel 0.0
    fixum f32 e20 ← 100.0
    adfirma v20 ≡ e20
    fixum f32 v21 ← out.accipe([2, 1]) vel 0.0
    fixum f32 e21 ← 200.0
    adfirma v21 ≡ e21
    fixum f32 v22 ← out.accipe([2, 2]) vel 0.0
    fixum f32 e22 ← 300.0
    adfirma v22 ≡ e22
    fixum f32 v30 ← out.accipe([3, 0]) vel 0.0
    fixum f32 e30 ← 1.0
    adfirma v30 ≡ e30
    fixum f32 v31 ← out.accipe([3, 1]) vel 0.0
    fixum f32 e31 ← 2.0
    adfirma v31 ≡ e31
    fixum f32 v32 ← out.accipe([3, 2]) vel 0.0
    fixum f32 e32 ← 3.0
    adfirma v32 ≡ e32
    nota out.magnitudines()
}
";
    let mut host = BufferHost::default();
    match run_source(&runner_session(), "gather.fab", source, &mut host) {
        Ok(()) => {}
        Err(RunSourceError::Frontend(diagnostics)) => {
            panic!("frontend should analyze cleanly: {diagnostics:?}")
        }
        Err(RunSourceError::Mir(error)) => panic!("lowering should be clean: {error:?}"),
        Err(RunSourceError::Stepper(errors)) => {
            panic!("runner should run the row gather: {errors:?}")
        }
    }
    assert_eq!(
        host.stdout_lines,
        vec!["[4, 3]".to_owned()],
        "the gather result shape is [T, D] = [4, 3]"
    );
}

/// CCI-4: an **out-of-range id fails closed** with the typed gather
/// diagnostic at the HIR→AIR construction site (no silent clamping).
#[test]
fn gather_out_of_range_id_fails_closed_typed_diagnostic() {
    let errors = air_lower_errors(
        r#"
@ radix lane "air"
functio select(tf32[4, 3] table) → tf32[4, 3] {
    redde table.gather([3, 0, 4])
}
"#,
    );
    assert!(
        errors.iter().any(|error| {
            error.issue.starts_with("invalid_gather_contract") && error.issue.contains("out_of_range")
        }),
        "expected typed gather range diagnostic, got {errors:?}"
    );
}

/// CCI-4: a negative id fails closed with the typed gather diagnostic.
#[test]
fn gather_negative_id_fails_closed_typed_diagnostic() {
    let errors = air_lower_errors(
        r#"
@ radix lane "air"
functio select(tf32[4, 3] table) → tf32[4, 3] {
    redde table.gather([2, -1])
}
"#,
    );
    assert!(
        errors
            .iter()
            .any(|error| { error.issue.starts_with("invalid_gather_contract") && error.issue.contains("negative") }),
        "expected typed gather negativity diagnostic, got {errors:?}"
    );
}

/// CCI-4: a non-literal id vector fails closed — the static contract cannot
/// be proven, so the call is not admitted (v1 requires a literal ids vector).
#[test]
fn gather_non_literal_ids_fail_closed_typed_diagnostic() {
    let errors = air_lower_errors(
        r#"
@ radix lane "air"
functio select(tf32[4, 3] table, lista<numerus> ids) → tf32[4, 3] {
    redde table.gather(ids)
}
"#,
    );
    assert!(
        errors
            .iter()
            .any(|error| { error.issue.starts_with("invalid_gather_contract") && error.issue.contains("literal") }),
        "expected typed gather literal diagnostic, got {errors:?}"
    );
}

/// CCI-4: the typed `[V,D]`+`[T]` call compiles through the full frontend to
/// AIR `Gather` (canonical args `[x, ids]`) and on to the MIR `TensorGather`
/// runtime call (`[x, ids]` layout).
#[test]
fn gather_resolves_to_tensor_gather_through_the_full_frontend() {
    let (air, types) = lower_with_types(
        r#"
@ radix lane "air"
functio select(tf32[4, 3] table) → tf32[4, 3] {
    redde table.gather([3, 0, 2, 0])
}
"#,
    );

    let binding = air.functions[0]
        .body
        .bindings
        .last()
        .expect("gather binding");
    let AirExpr::TensorOp { op, args } = &binding.expr else {
        panic!("expected tensor op, got {:?}", binding.expr);
    };
    assert_eq!(*op, AirTensorOp::Gather);
    assert_eq!(args.len(), 2, "canonical AIR args [x, ids]");

    let mir = crate::air::to_mir_program(&air, &crate::air::AirToMirContext::new()).expect("AIR to MIR");
    validate_program(&mir, &MirValidationContext::new(&types)).expect("AIR MIR validates");
    let call = collection_runtime_call(&mir, MirCollectionOp::TensorGather);
    assert_eq!(call.args.len(), 2, "MIR layout [x, ids]");
}

/// Tier 2: a dynamic out-of-bounds read on an exact-shape tensor is a hard
/// runner error, not a nullable result or a recoverable `⇥` path.
#[test]
fn exact_tensor_dynamic_read_oob_is_hard_error() {
    let source = r"
incipit {
    fixum lista<f32> flat ← [1.0, 2.0, 3.0]
    fixum tf32[] seed ← vacua
    fixum tf32[3] input ← seed.strue(flat, [3])
    fixum numerus i ← 3
    fixum f32 bad ← input.accipe([i]) vel 99.0
    nota bad
}
";
    let mut analysis = analyze_source(
        &runner_session(),
        "exact_tensor_dynamic_oob.fab",
        source,
        AnalyzeRequest::default(),
    )
    .expect("exact tensor OOB source should analyze");
    let lowered = crate::mir::lower_analyzed_unit(&mut analysis, Default::default())
        .expect("exact tensor OOB source should lower");
    let tensor_get = lowered
        .program
        .functions
        .iter()
        .flat_map(|function| &function.blocks)
        .flat_map(|block| &block.statements)
        .find_map(|statement| match &statement.kind {
            MirStatementKind::RuntimeCall { call, .. }
                if call.intrinsic == MirIntrinsic::Collection(MirCollectionOp::TensorGet) =>
            {
                Some(call)
            }
            _ => None,
        })
        .expect("exact tensor read must lower to TensorGet");
    assert!(
        !matches!(
            lowered
                .validated
                .validation()
                .types
                .get(tensor_get.return_ty.semantic_id()),
            Type::Option(_)
        ),
        "exact declared shape must select the strict MIR read carrier"
    );

    let mut host = BufferHost::default();
    let result = run_source(&runner_session(), "exact_tensor_dynamic_oob.fab", source, &mut host);
    let Err(RunSourceError::Stepper(errors)) = result else {
        panic!("exact tensor dynamic OOB must abort the runner: {result:?}");
    };
    assert!(
        errors
            .iter()
            .any(|error| error.issue == "runner_tensor_accipe_invalid_index"),
        "expected the named hard bounds error, got {errors:?}"
    );
}

/// Tier 3: dynamic-shape tensors retain nullable lookup semantics.
#[test]
fn dynamic_tensor_read_oob_remains_nullable() {
    let source = r"
functio lege<magnitudo N>(tensor<f32, [N]> input, numerus i) → f32 {
    redde input.accipe([i]) vel 99.0
}

incipit {
    fixum lista<f32> flat ← [1.0, 2.0, 3.0]
    fixum tf32[] seed ← vacua
    fixum tf32[3] input ← seed.strue(flat, [3])
    fixum f32 missing ← lege(input, 3)
    nota missing
}
";
    let mut host = BufferHost::default();
    run_source(&runner_session(), "dynamic_tensor_oob.fab", source, &mut host)
        .expect("dynamic tensor OOB stays a nullable lookup");
    assert_eq!(host.stdout_lines, vec!["99.0".to_owned()]);
}

/// Tier 2 in-bounds dynamic reads stay handler-free and preserve the value.
#[test]
fn exact_tensor_dynamic_read_in_bounds_is_handler_free() {
    let source = r"
incipit {
    fixum lista<f32> flat ← [1.0, 2.0, 3.0]
    fixum tf32[] seed ← vacua
    fixum tf32[3] input ← seed.strue(flat, [3])
    fixum numerus i ← 1
    fixum f32 value ← input.accipe([i]) vel 99.0
    nota value
}
";
    let mut host = BufferHost::default();
    run_source(&runner_session(), "exact_tensor_dynamic_in_bounds.fab", source, &mut host)
        .expect("in-bounds exact read does not need a handler");
    assert_eq!(host.stdout_lines, vec!["2.0".to_owned()]);
}

/// Tier 3: unbounded lists keep nullable lookup semantics on dynamic reads.
#[test]
fn unbounded_lista_dynamic_read_oob_remains_nullable() {
    let source = r"
incipit {
    fixum lista<numerus> values ← [1, 2]
    fixum numerus i ← 2
    fixum numerus missing ← values.accipe(i) vel 99
    nota missing
}
";
    let mut host = BufferHost::default();
    run_source(&runner_session(), "unbounded_lista_dynamic_oob.fab", source, &mut host)
        .expect("unbounded lista OOB stays a nullable lookup");
    assert_eq!(host.stdout_lines, vec!["99".to_owned()]);
}

/// CCI-4: `TensorGet` (`accipe`) and `TensorSlice` (`sectio`) still compile
/// and run unchanged (additive proof — row gather is not a rename).
#[test]
fn tensor_get_and_tensor_slice_compile_and_run_unchanged() {
    let source = r"
incipit {
    fixum lista<f32> flat ← [1.0, 2.0, 3.0, 10.0, 20.0, 30.0, 100.0, 200.0, 300.0]
    fixum tf32[] seed ← vacua
    fixum tf32[3, 3] table ← seed.strue(flat, [3, 3])
    fixum f32 cell ← table.accipe([1, 2]) vel 0.0
    fixum tf32[1, 3] slice ← table.sectio(1, 2)
    nota cell
    nota slice.planata()
}
";

    // Runner: unchanged TensorGet/TensorSlice behavior.
    let mut host = BufferHost::default();
    match run_source(&runner_session(), "tensor_get_slice.fab", source, &mut host) {
        Ok(()) => {}
        Err(RunSourceError::Frontend(diagnostics)) => {
            panic!("frontend should analyze cleanly: {diagnostics:?}")
        }
        Err(RunSourceError::Mir(error)) => panic!("lowering should be clean: {error:?}"),
        Err(RunSourceError::Stepper(errors)) => {
            panic!("runner should run TensorGet/TensorSlice: {errors:?}")
        }
    }
    assert_eq!(
        host.stdout_lines,
        vec!["30.0".to_owned(), "[10.0, 20.0, 30.0]".to_owned()],
        "TensorGet cell and TensorSlice row must be unchanged"
    );

    // Driver compile to a MIR-backed target stays green.
    let result = compile(
        &test_session(Target::MirLlvm),
        "tensor_get_slice.fab",
        source,
        ModuleCompileRequest::default(),
    );
    assert!(
        result.success(),
        "TensorGet/TensorSlice should still compile: {:?}",
        result.diagnostics
    );
}

/// CCI-4: an **empty id vector yields the empty `[0,D]` result** — the CTO-4
/// shape proof for `T = 0` at the MIR/runner layer.
///
/// The source-level spelling (`table.gather([])`) is proved by the companion
/// source-level test; this lower-layer test drives the exact `TensorGather`
/// op with the id vector emptied independently, proving the runner owns the
/// empty `[T,D]` shape fact for `T = 0`.
#[test]
fn gather_empty_id_vector_yields_empty_result() {
    let source = r"
incipit {
    fixum lista<f32> flat ← [1.0, 2.0, 3.0, 10.0, 20.0, 30.0, 100.0, 200.0, 300.0, 1000.0, 2000.0, 3000.0]
    fixum tf32[] seed ← vacua
    fixum tf32[4, 3] table ← seed.strue(flat, [4, 3])
    fixum tf32[4, 3] out ← table.gather([3, 0, 2, 0])
    nota out.magnitudines()
}
";
    let mut analysis =
        analyze_source(&runner_session(), "gather_empty.fab", source, AnalyzeRequest::default()).expect("frontend");
    let lowered = crate::mir::lower_analyzed_unit(&mut analysis, Default::default()).expect("lowering");

    let mut empty_program = lowered.program.clone();
    let emptied = empty_tensor_gather_ids(&mut empty_program);
    assert_eq!(emptied, 1, "the gather id-vector construct must be found and emptied");

    let context = lowered.validated.validation().clone();
    let empty_validated =
        ValidatedMir::new(empty_program, context).expect("empty id vector still validates at the MIR gate");

    let mut host = BufferHost::default();
    run_entry(&empty_validated, &mut host).expect("runner runs the empty gather");
    assert_eq!(
        host.stdout_lines,
        vec!["[0, 3]".to_owned()],
        "empty ids must yield the empty [0, D] shape"
    );
}

/// CCI-4: the empty id vector is spellable at the source level —
/// `table.gather([])` runs through the full frontend (the DEFER-064
/// `NumerusArray` policy admits the empty literal with its element type
/// resolved to the slot's canonical `numerus`) and the runner produces the
/// empty `[0, D]` result — the CTO-4 shape proof for `T = 0` end-to-end.
#[test]
fn gather_empty_literal_source_level_yields_empty_result() {
    let source = r"
incipit {
    fixum lista<f32> flat ← [1.0, 2.0, 3.0, 10.0, 20.0, 30.0, 100.0, 200.0, 300.0, 1000.0, 2000.0, 3000.0]
    fixum tf32[] seed ← vacua
    fixum tf32[4, 3] table ← seed.strue(flat, [4, 3])
    fixum tf32[4, 3] out ← table.gather([])
    nota out.magnitudines()
}
";
    let mut host = BufferHost::default();
    match run_source(&runner_session(), "gather_empty_literal.fab", source, &mut host) {
        Ok(()) => {}
        Err(RunSourceError::Frontend(diagnostics)) => {
            panic!("frontend should analyze cleanly: {diagnostics:?}")
        }
        Err(RunSourceError::Mir(error)) => panic!("lowering should be clean: {error:?}"),
        Err(RunSourceError::Stepper(errors)) => {
            panic!("runner should run the empty row gather: {errors:?}")
        }
    }
    assert_eq!(
        host.stdout_lines,
        vec!["[0, 3]".to_owned()],
        "empty ids must yield the empty [0, D] shape"
    );
}

// ---------------------------------------------------------------------------
// CCI-5: recipe resolution — the source fixture resolves to the GI3-1 recipe
// ---------------------------------------------------------------------------

/// The pinned typed `[V,D]` + `[T]` gather source (table `[4,3]`, ids
/// `[3, 0, 2, 0]`) — the CCI-4 pinned shape the plan facts resolve from.
const GATHER_PINNED_SOURCE: &str = r#"
@ radix lane "air"
functio select(tf32[4, 3] table) → tf32[4, 3] {
    redde table.gather([3, 0, 2, 0])
}
"#;

/// CCI-5: the pinned `gather` source fixture lowers AIR→MIR and the MIR
/// resolves through the GI3-1 seam to `CollectionKernelPlan::Gather` with
/// the typed facts (table rows V = 4, table cols D = 3, id count T = 4).
#[test]
fn gather_fixture_resolves_gather_recipe() {
    let (air, types) = lower_with_types(GATHER_PINNED_SOURCE);
    crate::air::validate_program(&air).expect("AIR should validate");
    let mir = crate::air::to_mir_program(&air, &crate::air::AirToMirContext::new()).expect("AIR to MIR");
    let validation = MirValidationContext::new(&types);
    let function = &mir.functions[0];
    let signature = transformer_shape_signature(function, MirCollectionOp::TensorGather, &validation)
        .expect("the transformer-shaped signature resolves for gather");
    let plan =
        CollectionKernelPlan::from_transformer_op(&signature, function, &validation, MirCollectionOp::TensorGather)
            .expect("the gather fixture resolves to a kernel plan");
    let CollectionKernelPlan::Gather(gather) = &plan else {
        panic!("expected Gather recipe, got {plan:?}");
    };
    assert_eq!(gather.table_rows, 4, "table rows V come from the operand dims");
    assert_eq!(gather.table_cols, 3, "table cols D come from the operand dims");
    assert_eq!(gather.id_count, 4, "id count T comes from the return-type dims");
    plan.validate().expect("the resolved plan validates");
}
