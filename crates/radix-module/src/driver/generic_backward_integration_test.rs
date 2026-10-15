//! Driver admission of `magnitudo`-generic backward primals (shape-generics
//! P3 unit-3B).
//!
//! Proves the production driver bundle path admits a concrete instance of a
//! generic `@ radix backward` primal end-to-end through
//! `prepare_air_backward_bundle` / `apply_air_backward_bundle`:
//!
//! - an uninstantiated generic remains a source/FHIR template and does not
//!   enter executable AIR or MIR;
//! - a concrete call specializes the primal before AIR, and its companion is
//!   generated and installed without a symbolic-shape error;
//! - the Phase-3 OQ4 pin settles in favor of "works unchanged": a
//!   `magnitudo`-generic wrapper calling the generic companion typechecks via
//!   shared `IndexExpr::Param` symbols, so `companion_signature.rs` needs no
//!   shape-param list;
//! - `prepare_air_backward_bundle` pre-interns broadcast cotangent-reduction
//!   types (`ensure_broadcast_reduction_types`, wired next to the matmul
//!   transpose pattern), so a broadcast-using generic primal generates its
//!   companion without `reverse_ad_broadcast_reduction_missing_type`.
//!
//! These tests stop at the MIR boundary — exactly the production path up to
//! emit (`lower_mir_with_context_for_target`).

use crate::codegen::Target;
use crate::driver::{
    AnalyzeRequest, FreshMirFunctionIds, SpannedCodegenError, analyze_source, apply_air_backward_bundle,
    lower_mir_with_context_for_target, prepare_air_backward_bundle,
};
use crate::mir::MirLoweringOptions;
use crate::mir::lower_analyzed_unit;
use crate::semantic::IndexExpr;
use crate::test_support::test_session_la;

/// Unwrap a driver bundle result, panicking with the codegen error message
/// (`SpannedCodegenError` carries no `Debug` impl).
fn expect_codegen_ok<T>(result: Result<T, SpannedCodegenError>, what: &str) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("{what}: {}", error.message),
    }
}

/// The generic `simple_loss` primal used by the concrete companion tests:
/// `simple_loss<R,C>(x, w) = media(x.multiplica(w))`.
const GENERIC_SIMPLE_LOSS_SOURCE: &str = r#"
@ radix lane "air"
@ radix backward "loss_backward"
functio simple_loss<magnitudo R, magnitudo C>(
    tensor<f32, [R, C]> x, tensor<f32, [R, C]> w
) → f32 {
    sit tensor<f32, [R, C]> product ← x ⊙ w
    redde product.media()
}
"#;

/// Names of every lowered MIR function, resolved through the lowering
/// interner.
fn mir_function_names(program: &crate::mir::MirProgram, interner: &crate::lexer::Interner) -> Vec<String> {
    program
        .functions
        .iter()
        .filter_map(|f| f.name.map(|symbol| interner.resolve(symbol).to_owned()))
        .collect()
}

#[test]
fn fresh_mir_function_ids_follow_sparse_max_without_collisions() {
    use crate::mir::MirFunctionId;

    let mut ids = FreshMirFunctionIds::after([MirFunctionId(7), MirFunctionId(1), MirFunctionId(3)]);

    assert_eq!(ids.next().expect("id space not exhausted"), MirFunctionId(8));
    assert_eq!(ids.next().expect("id space not exhausted"), MirFunctionId(9));
}

#[test]
fn concrete_generic_backward_reduction_finalizes_companion_shape_size() {
    let source = format!(
        "{GENERIC_SIMPLE_LOSS_SOURCE}\n{}",
        r"
incipit {
    sit tensor<f32, [2, 8]> x ← vacua
    sit tensor<f32, [2, 8]> w ← vacua
    sit f32 loss ← simple_loss(x, w)
    nota loss
}
"
    );
    let session = test_session_la(Target::MirLlvm);
    let mut analysis = analyze_source(&session, "generic_backward_concrete.fab", &source, AnalyzeRequest::default())
        .expect("concrete generic backward reduction should analyze cleanly");

    let lowered = expect_codegen_ok(
        lower_mir_with_context_for_target(&mut analysis, false),
        "concrete generic backward reduction should finalize after bundle application",
    );
    assert!(
        lowered
            .validated
            .program()
            .functions
            .iter()
            .all(|function| {
                let mut function = function.clone();
                !crate::mir::generic::function_contains_shape_size(&mut function)
            }),
        "the final whole executable MIR program must contain no ShapeSize survivor"
    );

    let names = mir_function_names(lowered.validated.program(), &lowered.interner);
    assert!(
        names.iter().any(|name| name == "loss_backward"),
        "concrete call site must install the generated companion; got functions: {names:?}"
    );
}

#[test]
fn unresolved_generic_backward_template_is_not_executable() {
    let session = test_session_la(Target::MirLlvm);
    let mut analysis = analyze_source(
        &session,
        "generic_backward_unresolved.fab",
        GENERIC_SIMPLE_LOSS_SOURCE,
        AnalyzeRequest::default(),
    )
    .expect("symbolic generic backward primal should analyze cleanly");
    let bundle = expect_codegen_ok(
        prepare_air_backward_bundle(&mut analysis),
        "backward bundle preparation should succeed",
    );
    assert!(bundle.is_none(), "an uncalled generic AIR template has no executable bundle");

    let lowered = lower_analyzed_unit(analysis.application_view(), MirLoweringOptions::default())
        .expect("template lowering succeeds");
    assert!(
        lowered.validated.program().functions.is_empty(),
        "generic AIR templates do not enter executable MIR"
    );
    assert!(
        lowered.generic_templates.is_empty(),
        "source templates stay in HIR/FHIR, not MIR"
    );
}

// ── Generic AIR templates remain deferred until a concrete call ──

#[test]
fn generic_backward_primal_is_deferred_without_a_concrete_call() {
    let session = test_session_la(Target::MirLlvm);
    let mut analysis = analyze_source(
        &session,
        "generic_backward.fab",
        GENERIC_SIMPLE_LOSS_SOURCE,
        AnalyzeRequest::default(),
    )
    .expect("generic backward primal should analyze cleanly");

    let bundle = expect_codegen_ok(
        prepare_air_backward_bundle(&mut analysis),
        "backward bundle preparation should succeed",
    );
    assert!(bundle.is_none(), "the generic source declaration is not executable AIR");
}

#[test]
fn generic_backward_template_stays_out_of_executable_mir() {
    let session = test_session_la(Target::MirLlvm);
    let mut analysis = analyze_source(
        &session,
        "generic_backward_package_template.fab",
        GENERIC_SIMPLE_LOSS_SOURCE,
        AnalyzeRequest::default(),
    )
    .expect("symbolic generic backward primal should analyze cleanly");
    let bundle = expect_codegen_ok(
        prepare_air_backward_bundle(&mut analysis),
        "backward bundle preparation should succeed",
    );
    assert!(bundle.is_none(), "no concrete call means no generated companion");
    let lowered = lower_analyzed_unit(analysis.application_view(), MirLoweringOptions::default())
        .expect("generic primal should lower to MIR");
    assert!(
        lowered.validated.program().functions.is_empty(),
        "the generic primal is not executable MIR"
    );
    assert!(
        lowered.generic_templates.is_empty(),
        "source templates stay in HIR/FHIR, not MIR"
    );
}

// ── Test 2: generic wrapper calls the generic companion (OQ4 pin) ──

#[test]
fn generic_wrapper_call_to_generic_companion_typechecks_at_call_site() {
    // OQ4 settles as "works unchanged": `companion_signature.rs` registers the
    // companion `FuncSig` with no shape-param list, but the call-site
    // typechecker derives shape params from the signature's `IndexExpr::Param`
    // positions and binds them through the wrapper's own rigid symbols — no
    // concrete-overload fallback, no `FuncSig` shape-param list.
    let source = r#"
@ radix lane "air"
@ radix backward "loss_backward"
functio simple_loss<magnitudo R, magnitudo C>(
    tensor<f32, [R, C]> x, tensor<f32, [R, C]> w
) → f32 {
    sit tensor<f32, [R, C]> product ← x ⊙ w
    redde product.media()
}

functio nil() → vacuum {
    tacet
}

functio gradientes_simple_loss<magnitudo R, magnitudo C>(
    tensor<f32, [R, C]> x, tensor<f32, [R, C]> w, f32 upstream
) → tensor<f32, [R, C]> {
    sit [gx, gw] ← loss_backward(x, w, nil(), upstream)
    redde gx
}
"#;

    let session = test_session_la(Target::MirLlvm);
    let mut analysis = analyze_source(&session, "generic_wrapper.fab", source, AnalyzeRequest::default())
        .expect("generic wrapper calling the generic companion must typecheck (OQ4)");

    // The wrapper's companion call is part of semantic analysis. Its generic
    // primal remains deferred until a concrete caller requests an instance.
    let bundle = expect_codegen_ok(
        prepare_air_backward_bundle(&mut analysis),
        "backward bundle preparation should succeed",
    );
    assert!(bundle.is_none(), "the wrapper has no concrete primal call site");
}

// ── Test 3: broadcast-using generic primal avoids the missing-type error ──

#[test]
fn broadcast_using_generic_primal_avoids_missing_broadcast_reduction_type() {
    // A partial broadcast `[R,1] + [R,C]` reduces the cotangent with SumAxis
    // over axis 1; the intermediate reduced shape `[R]` is not interned by
    // the forward program, so `prepare_air_backward_bundle` must pre-intern
    // it (the wired `ensure_broadcast_reduction_types`). Without the wiring
    // the walk fails with `reverse_ad_broadcast_reduction_missing_type`.
    let source = r#"
@ radix lane "air"
@ radix backward "broad_loss_backward"
functio broad_loss<magnitudo R, magnitudo C>(
    tensor<f32, [R, C]> a, tensor<f32, [C]> b
) → f32 {
    redde a.addita_bias(b).media()
}

incipit {
    sit tensor<f32, [2, 8]> a ← vacua
    sit tensor<f32, [8]> b ← vacua
    sit f32 loss ← broad_loss(a, b)
    nota loss
}
"#;

    let session = test_session_la(Target::MirLlvm);
    let mut analysis = analyze_source(&session, "generic_broadcast.fab", source, AnalyzeRequest::default())
        .expect("generic broadcast primal should analyze cleanly");

    let bundle = expect_codegen_ok(
        prepare_air_backward_bundle(&mut analysis),
        "backward bundle preparation should succeed",
    )
    .expect("an AIR lane with backward annotations must produce a bundle");

    // Load-bearing pre-intern check: the broadcast cotangent-reduction
    // intermediate `[8]` must be interned before reverse AD walks the
    // concrete instance, exactly as `ensure_broadcast_reduction_types` does
    // in `prepare_air_backward_bundle`.
    let param_row_shape_interned = (0..analysis.types.index_count()).any(|i| {
        let id = crate::semantic::IndexId(u32::try_from(i).expect("index table fits u32"));
        matches!(
            analysis.types.get_index(id),
            IndexExpr::Tuple(dims)
                if dims.len() == 1
                    && matches!(analysis.types.get_index(dims[0]), IndexExpr::Literal(8))
        )
    });
    assert!(
        param_row_shape_interned,
        "the broadcast-reduction intermediate shape [8] must be pre-interned \
         by prepare_air_backward_bundle (ensure_broadcast_reduction_types wiring)"
    );

    let lowered = lower_analyzed_unit(analysis.application_view(), MirLoweringOptions::default())
        .expect("generic broadcast primal should lower to MIR");
    let lowered = expect_codegen_ok(
        apply_air_backward_bundle(lowered, bundle, &analysis, false),
        "broadcast-using generic primal companion must generate without \
         reverse_ad_broadcast_reduction_missing_type",
    );

    let names = mir_function_names(lowered.validated.program(), &lowered.interner);
    assert!(
        names.iter().any(|name| name == "broad_loss_backward"),
        "companion 'broad_loss_backward' must be installed in the lowered MIR; got functions: {names:?}"
    );
}

// ── unit-3E: unrolled full-prefix range loop reaches reverse_ad ──

#[test]
fn unrolled_full_prefix_range_loop_generates_companion_through_driver() {
    let source = r#"
@ radix lane "air"
@ radix backward "lane_sum_backward"
functio lane_sum(tensor<f32, [4]> x) → f32 {
    varia tensor<f32, [4]> acc ← copy x
    itera ab 0‥4 sit i {
        acc ← acc + x
    }
    redde acc.media()
}
"#;

    let session = test_session_la(Target::MirLlvm);
    let mut analysis = analyze_source(&session, "unroll_lane_sum.fab", source, AnalyzeRequest::default())
        .expect("full-prefix unrolled range loop must analyze (SEM059.itera_loop flipped)");

    let bundle = expect_codegen_ok(
        prepare_air_backward_bundle(&mut analysis),
        "unrolled AIR-lane primal must lower to AIR and prepare a backward bundle",
    )
    .expect("an AIR lane with backward annotations must produce a bundle");

    let lane_sum_symbol = analysis.interner.intern("lane_sum");
    let primal = bundle
        .air_program
        .functions
        .iter()
        .find(|f| f.name == lane_sum_symbol)
        .expect("primal lane_sum must be present in the AIR program");
    assert!(
        !primal
            .body
            .bindings
            .iter()
            .any(|binding| matches!(binding.expr, crate::air::AirExpr::If { .. })),
        "full-prefix unroll must reach reverse_ad as a guard-free straight line"
    );

    let lowered = lower_analyzed_unit(analysis.application_view(), MirLoweringOptions::default())
        .expect("unrolled primal should lower to MIR");
    let lowered = expect_codegen_ok(
        apply_air_backward_bundle(lowered, bundle, &analysis, false),
        "unrolled full-prefix body must generate a reverse_ad companion",
    );

    let names = mir_function_names(lowered.validated.program(), &lowered.interner);
    assert!(
        names.iter().any(|name| name == "lane_sum_backward"),
        "companion 'lane_sum_backward' must be installed in the lowered MIR; got functions: {names:?}"
    );
    assert!(
        names.iter().any(|name| name == "lane_sum"),
        "primal 'lane_sum' must remain in the lowered MIR; got functions: {names:?}"
    );
}

#[test]
fn generic_full_prefix_range_reaches_concrete_backward_companion() {
    let source = r#"
@ radix lane "air"
@ radix backward "generic_lane_sum_backward"
functio generic_lane_sum<magnitudo N>(tensor<f32, [N]> x) → f32 {
    varia tensor<f32, [N]> acc ← copy x
    itera ex x sit value {
        acc ← acc + x
    }
    redde acc.media()
}

incipit {
    sit tensor<f32, [4]> x ← vacua
    sit f32 y ← generic_lane_sum(x)
    nota y
}
"#;

    let session = test_session_la(Target::MirLlvm);
    let mut analysis = analyze_source(&session, "generic_unroll_lane_sum.fab", source, AnalyzeRequest::default())
        .expect("generic full-prefix range must analyze after concrete specialization");
    let lowered = expect_codegen_ok(
        lower_mir_with_context_for_target(&mut analysis, false),
        "generic full-prefix range must reach the concrete backward companion",
    );
    let names = mir_function_names(lowered.validated.program(), &lowered.interner);
    assert!(
        names.iter().any(|name| name == "generic_lane_sum_backward"),
        "generic concrete companion must be installed in MIR; got functions: {names:?}"
    );
    assert!(
        lowered
            .validated
            .program()
            .functions
            .iter()
            .all(|function| {
                let mut function = function.clone();
                !crate::mir::generic::function_contains_shape_size(&mut function)
            }),
        "generic concrete primal and companion must have no ShapeSize survivor"
    );
}

#[test]
fn generic_air_instance_over_unroll_budget_fails_closed() {
    let source = r#"
@ radix lane "air"
functio generic_too_wide<magnitudo N>(tensor<f32, [N]> x) → f32 {
    itera ex x sit value { }
    redde 0.0
}

incipit {
    sit tensor<f32, [65]> x ← vacua
    sit f32 y ← generic_too_wide(x)
    nota y
}
"#;

    let result = analyze_source(
        &test_session_la(Target::MirLlvm),
        "generic_unroll_over_budget.fab",
        source,
        AnalyzeRequest::default(),
    );
    let Err(diagnostics) = result else {
        panic!("a concrete generic AIR instance above UNROLL_BUDGET must fail closed");
    };
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.issue() == Some("itera_loop")
                && diagnostic
                    .args
                    .iter()
                    .any(|arg| arg.name == "causa" && arg.value == "budget")
        }),
        "expected the specialized generic instance to report the unroll budget, got {diagnostics:?}"
    );
}

// ── unit-3F: masked len ≤ N unrolled body generates a companion ──

#[test]
fn generic_masked_route_reaches_concrete_backward_companion() {
    let source = r#"
@ radix lane "air"
@ radix backward "generic_masked_mean_backward"
functio generic_masked_mean<magnitudo N>(
    tensor<f32, [N]> x, tensor<f32, [N]> mask
) → f32 {
    varia tensor<f32, [N]> acc ← copy x
    itera ex x sit value {
        acc ← acc + x
    }
    redde (acc ⊙ mask).media()
}

incipit {
    sit tensor<f32, [4]> x ← vacua
    sit tensor<f32, [4]> mask ← vacua
    sit f32 y ← generic_masked_mean(x, mask)
    nota y
}
"#;

    let session = test_session_la(Target::MirLlvm);
    let mut analysis = analyze_source(&session, "generic_masked_mean.fab", source, AnalyzeRequest::default())
        .expect("generic caller-mask AIR instance must analyze cleanly");
    let bundle = expect_codegen_ok(
        prepare_air_backward_bundle(&mut analysis),
        "generic caller-mask AIR instance must prepare a backward bundle",
    )
    .expect("the concrete generic masked primal must produce a backward bundle");

    let primal_symbol = analysis.interner.intern("generic_masked_mean");
    let primal = bundle
        .air_program
        .functions
        .iter()
        .find(|function| function.name == primal_symbol)
        .expect("the concrete generic masked primal must enter AIR");
    assert!(
        !primal
            .body
            .bindings
            .iter()
            .any(|binding| matches!(binding.expr, crate::air::AirExpr::If { .. })),
        "generic caller-mask unroll must be guard-free before reverse AD"
    );
    assert!(
        primal.body.bindings.iter().any(|binding| {
            matches!(
                binding.expr,
                crate::air::AirExpr::TensorOp { op: crate::air::AirTensorOp::Mul, .. }
            )
        }),
        "the concrete generic instance must retain the caller-built mask multiply"
    );

    let lowered = lower_analyzed_unit(analysis.application_view(), MirLoweringOptions::default())
        .expect("generic masked primal should lower to MIR");
    let lowered = expect_codegen_ok(
        apply_air_backward_bundle(lowered, bundle, &analysis, false),
        "generic caller-mask body must generate a reverse-AD companion",
    );
    let names = mir_function_names(lowered.validated.program(), &lowered.interner);
    assert!(
        names
            .iter()
            .any(|name| name == "generic_masked_mean_backward"),
        "generic caller-mask companion must be installed in MIR; got functions: {names:?}"
    );
}

#[test]
fn unrolled_masked_route_generates_companion_through_driver() {
    let source = r#"
@ radix lane "air"
@ radix backward "masked_mean_backward"
functio masked_lane_mean(tensor<f32, [4]> x, tensor<f32, [4]> mask) → f32 {
    varia tensor<f32, [4]> acc ← copy x
    itera ab 0‥4 sit i {
        acc ← acc + x
    }
    redde (acc ⊙ mask).media()
}
"#;

    let session = test_session_la(Target::MirLlvm);
    let mut analysis = analyze_source(&session, "unroll_masked_mean.fab", source, AnalyzeRequest::default())
        .expect("mask-mode unrolled range loop must analyze");

    let bundle = expect_codegen_ok(
        prepare_air_backward_bundle(&mut analysis),
        "masked AIR-lane primal must lower to AIR and prepare a backward bundle",
    )
    .expect("an AIR lane with backward annotations must produce a bundle");

    let primal_symbol = analysis.interner.intern("masked_lane_mean");
    let primal = bundle
        .air_program
        .functions
        .iter()
        .find(|f| f.name == primal_symbol)
        .expect("primal masked_lane_mean must be present in the AIR program");
    assert!(
        !primal
            .body
            .bindings
            .iter()
            .any(|binding| matches!(binding.expr, crate::air::AirExpr::If { .. })),
        "mask-mode unroll must reach reverse_ad as a guard-free straight line"
    );

    let lowered = lower_analyzed_unit(analysis.application_view(), MirLoweringOptions::default())
        .expect("masked primal should lower to MIR");
    let lowered = expect_codegen_ok(
        apply_air_backward_bundle(lowered, bundle, &analysis, false),
        "masked unrolled body must generate a reverse_ad companion",
    );

    let names = mir_function_names(lowered.validated.program(), &lowered.interner);
    assert!(
        names.iter().any(|name| name == "masked_mean_backward"),
        "companion 'masked_mean_backward' must be installed in the lowered MIR; got functions: {names:?}"
    );
}

// ── companion call chaining: a later call reads the rewritten destination ──

/// One symbolic companion template `name(tensor<f32, [shape]>) → tensor<f32, [shape]>`.
fn chained_companion_template(id: u32, source: u32, ty: crate::mir::MirType) -> crate::mir::MirFunction {
    use crate::hir::DefId;
    use crate::mir::{
        MirBlock, MirBlockId, MirFunction, MirFunctionId, MirLocalId, MirParam, MirParamMode, MirTerminator,
        MirTerminatorKind,
    };
    use crate::test_support::test_span;

    MirFunction {
        id: MirFunctionId(id),
        source: Some(DefId(source)),
        name: None,
        params: vec![MirParam { local: MirLocalId(0), name: None, ty, mode: MirParamMode::Owned, span: test_span() }],
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: Vec::new(),
            terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: test_span() },
            span: test_span(),
        }],
        return_ty: ty,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: test_span(),
    }
}

#[test]
fn chained_companion_call_reads_the_rewritten_destination_type() {
    use crate::driver::rewrite_air_backward_calls;
    use crate::hir::DefId;
    use crate::lexer::Interner;
    use crate::mir::generic::canonical_index;
    use crate::mir::{
        MirBlock, MirBlockId, MirCallee, MirFunction, MirFunctionId, MirLocal, MirLocalId, MirMonomorphizationKey,
        MirOperand, MirParam, MirParamMode, MirPlace, MirPlaceBase, MirStatement, MirStatementKind, MirTemp, MirTempId,
        MirTerminator, MirTerminatorKind, MirType,
    };
    use crate::semantic::{NumericWidth, Primitive, TypeTable};
    use crate::test_support::test_span;

    // Two companion templates over distinct shape symbols: `first<N>` and
    // `second<M>`. The caller computes `t0 ← first(x)` and feeds the result to
    // `second(t0)`. The pre-rewrite type of `t0` is still the symbolic
    // `tensor<f32, [N]>`; only the rewrite of the first call makes it
    // `tensor<f32, [4]>`, which is what binds `M` for the second call.
    let mut interner = Interner::new();
    let n = interner.intern("N");
    let m = interner.intern("M");
    let mut types = TypeTable::new();
    let element = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let four = types.intern_index(IndexExpr::Literal(4));
    let shape_n = types.intern_index(IndexExpr::Param(n));
    let shape_m = types.intern_index(IndexExpr::Param(m));
    let concrete = MirType::semantic(types.tensor_with_shape(element, four));
    let symbolic_n = MirType::semantic(types.tensor_with_shape(element, shape_n));
    let symbolic_m = MirType::semantic(types.tensor_with_shape(element, shape_m));

    let first = DefId(10);
    let second = DefId(11);
    let call = |destination: MirPlace, source: DefId, argument: MirOperand| MirStatement {
        kind: MirStatementKind::Call {
            destination: Some(destination),
            callee: MirCallee::definition(source),
            args: vec![argument],
        },
        span: test_span(),
    };
    let caller = MirFunction {
        id: MirFunctionId(3),
        source: Some(DefId(12)),
        name: None,
        params: vec![MirParam {
            local: MirLocalId(0),
            name: None,
            ty: concrete,
            mode: MirParamMode::Owned,
            span: test_span(),
        }],
        // Lowering declares every param in `locals` as well as `params`.
        locals: vec![
            MirLocal { id: MirLocalId(0), name: None, ty: concrete, mutable: false, span: test_span() },
            MirLocal { id: MirLocalId(1), name: None, ty: symbolic_m, mutable: false, span: test_span() },
        ],
        temps: vec![MirTemp { id: MirTempId(0), ty: symbolic_n, span: test_span() }],
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![
                call(
                    MirPlace { base: MirPlaceBase::Temp(MirTempId(0)), projections: Vec::new() },
                    first,
                    MirOperand::Place(MirPlace::local(MirLocalId(0))),
                ),
                call(MirPlace::local(MirLocalId(1)), second, MirOperand::Temp(MirTempId(0))),
            ],
            terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: test_span() },
            span: test_span(),
        }],
        return_ty: concrete,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: test_span(),
    };
    let mut functions = vec![
        chained_companion_template(1, 10, symbolic_n),
        chained_companion_template(2, 11, symbolic_m),
        caller,
    ];

    let companion_sources = [first, second]
        .into_iter()
        .collect::<rustc_hash::FxHashSet<_>>();
    let concrete_shape = canonical_index(&types, four);
    let mut instances = rustc_hash::FxHashMap::default();
    let mut instance_return_types = rustc_hash::FxHashMap::default();
    for (source, id) in [(first, 21), (second, 22)] {
        let key = MirMonomorphizationKey { source, type_args: Vec::new(), shape_args: vec![concrete_shape] };
        instances.insert(key.clone(), MirFunctionId(id));
        instance_return_types.insert(key, concrete);
    }

    expect_codegen_ok(
        rewrite_air_backward_calls(&mut functions, &types, &companion_sources, &instances, &instance_return_types),
        "a companion call fed by an earlier companion call must see the rewritten destination type",
    );

    let callees = functions[2].blocks[0]
        .statements
        .iter()
        .map(|statement| match &statement.kind {
            MirStatementKind::Call { callee, .. } => callee.clone(),
            other => panic!("expected a call statement, got {other:?}"),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        callees,
        vec![
            MirCallee::Function(MirFunctionId(21)),
            MirCallee::Function(MirFunctionId(22))
        ]
    );
}
