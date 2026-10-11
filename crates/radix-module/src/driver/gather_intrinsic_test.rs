//! CCI-4 driver-level tests for the row-gather intrinsic (`tensor.gather(ids)`).
//!
//! Proves the CCI-1-frozen row-gather source contract end-to-end through the
//! full language frontend (lex → parse → semantic → HIR→AIR → AIR validation
//! → MIR re-lowering) and the CPU runner, per the CTO row-6 acceptance proof
//! (mail `4971fc90`) and
//! `docs/archived/common-case-intrinsics/common-case-intrinsics-contract.md`:
//! - a typed `[V,D]` + `tensor<u32,[T]>` gather call compiles and the runner
//!   produces `[T,D]` matching row selection for **repeated and out-of-order**
//!   ids (K7c: the ids are a `tensor<u32,[n]>`, never a `lista`);
//! - an **out-of-range** id is a hard runner error (no silent clamping), and
//!   the AIR lane's static literal-ids contract fails closed on a runtime
//!   `tensor<u32,[n]>` ids operand with the typed `invalid_gather_contract`
//!   diagnostic;
//! - `TensorGet`/`TensorSlice` calls still compile and run unchanged
//!   (additive proof — row gather is not a rename of the indexing ops);
//! - an **empty id tensor yields the empty `[0,D]` result** (the CTO-4 shape
//!   proof, carried by the MIR `TensorGather` op + the runner). The empty case
//!   is spellable at the source level as a `tensor<u32, [0]>`.

use crate::air::lower_program;
use crate::codegen::Target;
use crate::driver::{AnalyzeRequest, Config, ModuleCompileRequest, Session, analyze_source, compile};
use crate::mir::MirLoweringOptions;
use crate::mir::{
    BufferHost, MirCollectionOp, MirIntrinsic, MirRuntimeCall, MirStatementKind, RunSourceError, run_source,
};
use crate::semantic::{self, PassConfig};
use crate::test_support::test_session;
use radix_mir::device_program_plans::transformer_shape_signature;
use radix_mir::kernel_plan::CollectionKernelPlan;
use radix_types::Type;

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
            .with_dev_locale_root(),
    )
}

/// Extract the `TensorGather` runtime call from a lowered MIR program.
fn collection_runtime_call(mir: &crate::mir::MirProgram, op: MirCollectionOp) -> &MirRuntimeCall {
    mir.functions
        .iter()
        .flat_map(|function| &function.blocks)
        .flat_map(|block| &block.statements)
        .find_map(|statement| match &statement.kind {
            MirStatementKind::RuntimeCall { call, .. }
                if matches!(call.intrinsic, MirIntrinsic::Collection(actual) if actual == op) =>
            {
                Some(call)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("expected a {op:?} runtime call in the lowered MIR"))
}

/// CCI-4 / K7c: a typed `[V,D]` + `tensor<u32,[T]>` gather call runs through
/// the full frontend and the runner produces `[T,D]` matching row selection
/// for **repeated and out-of-order** ids (CTO row 6).
#[test]
fn gather_selects_rows_for_repeated_and_out_of_order_ids() {
    // Table [4,3] with distinct rows; ids [3, 0, 2, 0] are out-of-order and
    // repeat row 0. Expected rows: row3, row0, row2, row0.
    let source = r"
incipit {
    fixum lista<f32> flat ← [1.0, 2.0, 3.0, 10.0, 20.0, 30.0, 100.0, 200.0, 300.0, 1000.0, 2000.0, 3000.0]
    fixum tf32[4, 3] table ← flat ↦ tf32[4, 3]
    fixum lista<u32> rows ← [3, 0, 2, 0]
    fixum tensor<u32, [4]> ids ← rows ↦ tensor<u32, [4]>
    fixum tf32[4, 3] out ← table.gather(ids)
    fixum f32 v00 ← out.accipe([0, 0])
    fixum f32 e00 ← 1000.0
    adfirma v00 ≡ e00
    fixum f32 v01 ← out.accipe([0, 1])
    fixum f32 e01 ← 2000.0
    adfirma v01 ≡ e01
    fixum f32 v02 ← out.accipe([0, 2])
    fixum f32 e02 ← 3000.0
    adfirma v02 ≡ e02
    fixum f32 v10 ← out.accipe([1, 0])
    fixum f32 e10 ← 1.0
    adfirma v10 ≡ e10
    fixum f32 v11 ← out.accipe([1, 1])
    fixum f32 e11 ← 2.0
    adfirma v11 ≡ e11
    fixum f32 v12 ← out.accipe([1, 2])
    fixum f32 e12 ← 3.0
    adfirma v12 ≡ e12
    fixum f32 v20 ← out.accipe([2, 0])
    fixum f32 e20 ← 100.0
    adfirma v20 ≡ e20
    fixum f32 v21 ← out.accipe([2, 1])
    fixum f32 e21 ← 200.0
    adfirma v21 ≡ e21
    fixum f32 v22 ← out.accipe([2, 2])
    fixum f32 e22 ← 300.0
    adfirma v22 ≡ e22
    fixum f32 v30 ← out.accipe([3, 0])
    fixum f32 e30 ← 1.0
    adfirma v30 ≡ e30
    fixum f32 v31 ← out.accipe([3, 1])
    fixum f32 e31 ← 2.0
    adfirma v31 ≡ e31
    fixum f32 v32 ← out.accipe([3, 2])
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

/// K7c: an **out-of-range id is a hard runner error** (no silent clamping):
/// the ids are runtime data, so the row bound is checked where they are read.
#[test]
fn gather_out_of_range_id_is_a_hard_runner_error() {
    let source = r"
incipit {
    fixum lista<f32> flat ← [1.0, 2.0, 3.0, 10.0, 20.0, 30.0, 100.0, 200.0, 300.0, 1000.0, 2000.0, 3000.0]
    fixum tf32[4, 3] table ← flat ↦ tf32[4, 3]
    fixum lista<u32> rows ← [3, 0, 4]
    fixum tensor<u32, [3]> ids ← rows ↦ tensor<u32, [3]>
    fixum tf32[3, 3] out ← table.gather(ids)
    nota out.magnitudines()
}
";
    let mut host = BufferHost::default();
    match run_source(&runner_session(), "gather_oob.fab", source, &mut host) {
        Err(RunSourceError::Stepper(errors)) => {
            let text = format!("{errors:?}");
            assert!(
                text.contains("tensor gather id 4 is out of range for table rows 4"),
                "expected the out-of-range gather id error, got {text}"
            );
        }
        other => panic!("an out-of-range gather id must be a hard runner error, got {other:?}"),
    }
}

/// CCI-4 / K7c: the AIR lane proves gather ids statically, and a
/// `tensor<u32,[n]>` operand is runtime data, so the call fails closed with
/// the typed gather diagnostic (the MIR lane owns runtime ids).
#[test]
fn gather_runtime_tensor_ids_fail_closed_in_the_air_lane() {
    let errors = air_lower_errors(
        r#"
@ radix lane "air"
functio select(tf32[4, 3] table, tensor<u32, [4]> ids) → tf32[4, 3] {
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

/// CCI-4 / K7c: the typed `[V,D]`+`tensor<u32,[T]>` call compiles through the
/// full frontend and MIR lowering to the `TensorGather` runtime call with
/// the `[table, ids]` operand layout.
#[test]
fn gather_resolves_to_tensor_gather_through_the_full_frontend() {
    let source = r"
functio select(tf32[4, 3] table, tensor<u32, [4]> ids) → tf32[4, 3] {
    redde table.gather(ids)
}
incipit {
    nota 1
}
";
    let analysis =
        analyze_source(&runner_session(), "gather_mir.fab", source, AnalyzeRequest::default()).expect("frontend");
    let lowered =
        crate::mir::lower_analyzed_unit(analysis.application_view(), MirLoweringOptions::default()).expect("lowering");
    let call = collection_runtime_call(lowered.validated.program(), MirCollectionOp::TensorGather);
    assert_eq!(call.args.len(), 2, "MIR layout [table, ids]");
}

/// Tier 2: a dynamic out-of-bounds read on an exact-shape tensor is a hard
/// runner error, not a nullable result or a recoverable `⇥` path.
#[test]
fn exact_tensor_dynamic_read_oob_is_hard_error() {
    let source = r"
incipit {
    fixum lista<f32> flat ← [1.0, 2.0, 3.0]
    fixum tf32[3] input ← flat ↦ tf32[3]
    fixum numerus i ← 3
    fixum f32 bad ← input[i]
    nota bad
}
";
    let analysis = analyze_source(
        &runner_session(),
        "exact_tensor_dynamic_oob.fab",
        source,
        AnalyzeRequest::default(),
    )
    .expect("exact tensor OOB source should analyze");
    let lowered = crate::mir::lower_analyzed_unit(analysis.application_view(), MirLoweringOptions::default())
        .expect("exact tensor OOB source should lower");
    let tensor_get = lowered
        .validated
        .program()
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

/// K11-b: a dynamic-shape tensor `accipe` is the checked read; out of range it takes the `⊥` default.
#[test]
fn dynamic_tensor_read_oob_takes_the_call_default() {
    let source = r"
functio lege<magnitudo N>(tensor<f32, [N]> input, numerus i) → f32 {
    redde input.accipe([i]) ⊥ 99.0
}

incipit {
    fixum lista<f32> flat ← [1.0, 2.0, 3.0]
    fixum tf32[3] input ← flat ↦ tf32[3]
    fixum f32 missing ← lege(input, 3)
    nota missing
}
";
    let mut host = BufferHost::default();
    run_source(&runner_session(), "dynamic_tensor_oob.fab", source, &mut host)
        .expect("dynamic tensor OOB takes the call default");
    assert_eq!(host.stdout_lines, vec!["99.0".to_owned()]);
}

/// Tier 2 in-bounds dynamic reads stay handler-free and preserve the value.
#[test]
fn exact_tensor_dynamic_read_in_bounds_is_handler_free() {
    let source = r"
incipit {
    fixum lista<f32> flat ← [1.0, 2.0, 3.0]
    fixum tf32[3] input ← flat ↦ tf32[3]
    fixum numerus i ← 1
    fixum f32 value ← input.accipe([i]) ⊥ 99.0
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
    fixum tf32[3, 3] table ← flat ↦ tf32[3, 3]
    fixum f32 cell ← table.accipe([1, 2])
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

/// CCI-4 / K7c: an **empty id tensor yields the empty `[0,D]` result** — the
/// CTO-4 shape proof for `T = 0`, spelled at the source level as a
/// `tensor<u32, [0]>` and run through the full frontend and the runner.
#[test]
fn gather_empty_ids_tensor_yields_empty_result() {
    let source = r"
incipit {
    fixum lista<f32> flat ← [1.0, 2.0, 3.0, 10.0, 20.0, 30.0, 100.0, 200.0, 300.0, 1000.0, 2000.0, 3000.0]
    fixum tf32[4, 3] table ← flat ↦ tf32[4, 3]
    fixum lista<u32> rows ← []
    fixum tensor<u32, [0]> ids ← rows ↦ tensor<u32, [0]>
    fixum tf32[0, 3] out ← table.gather(ids)
    nota out.magnitudines()
}
";
    let mut host = BufferHost::default();
    match run_source(&runner_session(), "gather_empty_ids.fab", source, &mut host) {
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

/// The pinned typed `[V,D]` + `tensor<u32,[T]>` gather source (table `[4,3]`,
/// ids `[4]`) — the CCI-4 pinned shape the plan facts resolve from. It lowers
/// on the MIR lane: the ids are runtime data, which the AIR lane's static
/// literal-ids contract does not admit.
const GATHER_PINNED_SOURCE: &str = r"
functio select(tf32[4, 3] table, tensor<u32, [4]> ids) → tf32[4, 3] {
    redde table.gather(ids)
}
incipit {
    nota 1
}
";

/// CCI-5: the pinned `gather` source fixture lowers to MIR and the MIR
/// resolves through the GI3-1 seam to `CollectionKernelPlan::Gather` with
/// the typed facts (table rows V = 4, table cols D = 3, id count T = 4).
#[test]
fn gather_fixture_resolves_gather_recipe() {
    let analysis = analyze_source(
        &runner_session(),
        "gather_pinned.fab",
        GATHER_PINNED_SOURCE,
        AnalyzeRequest::default(),
    )
    .expect("frontend");
    let lowered =
        crate::mir::lower_analyzed_unit(analysis.application_view(), MirLoweringOptions::default()).expect("lowering");
    let validation = lowered.validated.validation();
    let function = lowered
        .validated
        .program()
        .functions
        .iter()
        .find(|function| {
            function
                .blocks
                .iter()
                .flat_map(|block| &block.statements)
                .any(|statement| {
                    matches!(
                        &statement.kind,
                        MirStatementKind::RuntimeCall { call, .. }
                            if call.intrinsic == MirIntrinsic::Collection(MirCollectionOp::TensorGather)
                    )
                })
        })
        .expect("the gather function");
    let signature = transformer_shape_signature(function, MirCollectionOp::TensorGather, validation)
        .expect("the transformer-shaped signature resolves for gather");
    let plan =
        CollectionKernelPlan::from_transformer_op(&signature, function, validation, MirCollectionOp::TensorGather)
            .expect("the gather fixture resolves to a kernel plan");
    let CollectionKernelPlan::Gather(gather) = &plan else {
        panic!("expected Gather recipe, got {plan:?}");
    };
    assert_eq!(gather.table_rows, 4, "table rows V come from the operand dims");
    assert_eq!(gather.table_cols, 3, "table cols D come from the operand dims");
    assert_eq!(gather.id_count, 4, "id count T comes from the return-type dims");
    plan.validate().expect("the resolved plan validates");
}
