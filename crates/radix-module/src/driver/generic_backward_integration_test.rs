//! Driver admission of `magnitudo`-generic backward primals (shape-generics
//! P3 unit-3B).
//!
//! Proves the production driver bundle path admits a generic `@ radix
//! backward` primal end-to-end through `prepare_air_backward_bundle` /
//! `apply_air_backward_bundle`:
//!
//! - the generic primal's tensor shapes arrive in AIR with `IndexExpr::Param`
//!   dims unchanged, and one symbolic companion is generated and installed
//!   into the lowered MIR without a symbolic-shape error;
//! - the Phase-3 OQ4 pin settles in favor of "works unchanged": a
//!   `magnitudo`-generic wrapper calling the generic companion typechecks via
//!   shared `IndexExpr::Param` symbols, so `companion_signature.rs` needs no
//!   shape-param list;
//! - `prepare_air_backward_bundle` pre-interns broadcast cotangent-reduction
//!   types (`ensure_broadcast_reduction_types`, wired next to the matmul
//!   transpose pattern), so a broadcast-using generic primal generates its
//!   companion without `reverse_ad_broadcast_reduction_missing_type`.
//!
//! The full target-emission of a symbolic companion (concrete substitution at
//! codegen) is Phase 4, so these tests stop at the MIR boundary — exactly the
//! production path up to emit (`lower_mir_with_context_for_target`).

use crate::codegen::Target;
use crate::driver::{
    analyze_source, apply_air_backward_bundle, lower_mir_with_context_for_target, prepare_air_backward_bundle,
    FreshMirFunctionIds, SpannedCodegenError,
    AnalyzeRequest};
use crate::mir::{lower_analyzed_unit, MirFunctionId};
use crate::semantic::{IndexExpr, Type};
use crate::test_support::test_session;

/// Unwrap a driver bundle result, panicking with the codegen error message
/// (`SpannedCodegenError` carries no `Debug` impl).
fn expect_codegen_ok<T>(result: Result<T, SpannedCodegenError>, what: &str) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("{what}: {}", error.message),
    }
}

/// The generic `simple_loss` primal every symbolic companion test starts
/// from: `simple_loss<R,C>(x, w) = media(x.multiplica(w))`.
const GENERIC_SIMPLE_LOSS_SOURCE: &str = r#"
@ radix lane "air"
@ radix backward "loss_backward"
functio simple_loss<magnitudo R, magnitudo C>(
    tensor<f32, [R, C]> x, tensor<f32, [R, C]> w
) → f32 {
    fixum tensor<f32, [R, C]> product ← x.multiplica(w)
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

    assert_eq!(ids.next(), MirFunctionId(8));
    assert_eq!(ids.next(), MirFunctionId(9));
}

#[test]
fn concrete_generic_backward_reduction_finalizes_companion_shape_size() {
    let source = format!(
        "{GENERIC_SIMPLE_LOSS_SOURCE}\n{}",
        r"
functio nil() → vacuum {
    tacet
}

functio gradientes_simple_loss<magnitudo R, magnitudo C>(
    tensor<f32, [R, C]> x, tensor<f32, [R, C]> w, f32 upstream
) → tensor<f32, [R, C]> {
    fixum [gx, gw] ← loss_backward(x, w, nil(), upstream)
    redde gx
}

incipit {
    fixum tensor<f32, [2, 8]> x ← vacua
    fixum tensor<f32, [2, 8]> w ← vacua
    fixum tensor<f32, [2, 8]> gx ← gradientes_simple_loss(x, w, 1.0)
    nota gx
}
"
    );
    let session = test_session(Target::MirLlvm);
    let mut analysis = analyze_source(&session, "generic_backward_concrete.fab", &source, AnalyzeRequest::default())
        .expect("concrete generic backward reduction should analyze cleanly");

    let mut lowered = expect_codegen_ok(
        lower_mir_with_context_for_target(&mut analysis, false),
        "concrete generic backward reduction should finalize after bundle application",
    );
    assert!(
        lowered
            .program
            .functions
            .iter_mut()
            .all(|function| !crate::mir::generic::function_contains_shape_size(function)),
        "the final whole executable MIR program must contain no ShapeSize survivor"
    );

    let companion_instance = lowered
        .monomorphizations
        .iter()
        .find(|monomorphization| monomorphization.key.source.0 & 0x8000_0000 != 0)
        .expect("concrete call site must form a companion instance");
    assert_eq!(companion_instance.key.shape_args.len(), 2);
    let types = lowered.validated.validation().types;
    assert!(companion_instance
        .key
        .shape_args
        .iter()
        .all(|shape| { matches!(types.get_index(*shape), IndexExpr::Literal(_)) }));
    let instance = lowered
        .program
        .functions
        .iter()
        .find(|function| function.id == companion_instance.function)
        .expect("companion instance must be executable MIR");
    for param in &instance.params[..2] {
        let Type::Tensor(_, shape) = types.get(param.ty.semantic_id()) else {
            panic!("companion input must remain a tensor");
        };
        let IndexExpr::Tuple(dims) = types.get_index(*shape) else {
            panic!("companion tensor shape must be a tuple");
        };
        assert!(dims
            .iter()
            .all(|dim| matches!(types.get_index(*dim), IndexExpr::Literal(_))));
    }
}

#[test]
fn unresolved_post_bundle_shape_size_uses_stable_survivor_diagnostic() {
    let session = test_session(Target::MirLlvm);
    let mut analysis = analyze_source(&session, "generic_backward_unresolved.fab", GENERIC_SIMPLE_LOSS_SOURCE, AnalyzeRequest::default())
        .expect("symbolic generic backward primal should analyze cleanly");
    let bundle = expect_codegen_ok(
        prepare_air_backward_bundle(&mut analysis),
        "backward bundle preparation should succeed",
    )
    .expect("backward bundle");
    let concrete_dim = analysis.types.intern_index(IndexExpr::Literal(2));
    let mut lowered = lower_analyzed_unit(&mut analysis, Default::default()).expect("initial MIR lowering succeeds");

    // Force the finalizer down its executable-program path without adding a
    // usable binding for the companion's [R,C] shape.
    lowered
        .monomorphizations
        .push(crate::mir::MirMonomorphization {
            key: crate::mir::MirMonomorphizationKey {
                source: crate::hir::DefId(0x7fff_ffff),
                type_args: Vec::new(),
                shape_args: vec![concrete_dim],
            },
            function: MirFunctionId(u32::MAX),
        });

    let error = apply_air_backward_bundle(&mut lowered, bundle, false)
        .expect_err("an unresolved executable ShapeSize must fail closed");
    assert_eq!(
        error
            .args
            .iter()
            .find(|arg| arg.name == "issue")
            .map(|arg| arg.value.as_str()),
        Some("unresolved_shape_size_constant_survives_monomorphization")
    );
}

// ── Test 1: generic backward primal admits a symbolic companion ──

#[test]
fn generic_backward_primal_generates_symbolic_companion_through_driver() {
    let session = test_session(Target::MirLlvm);
    let mut analysis = analyze_source(&session, "generic_backward.fab", GENERIC_SIMPLE_LOSS_SOURCE, AnalyzeRequest::default())
        .expect("generic backward primal should analyze cleanly");

    let bundle = expect_codegen_ok(
        prepare_air_backward_bundle(&mut analysis),
        "backward bundle preparation should succeed",
    )
    .expect("an AIR lane with backward annotations must produce a bundle");

    // The generic primal's `[R, C]` shapes arrive in AIR with symbolic
    // `IndexExpr::Param` dims (HIR→AIR lowering passes shapes through).
    let simple_loss_symbol = analysis.interner.intern("simple_loss");
    let primal = bundle
        .air_program
        .functions
        .iter()
        .find(|f| f.name == simple_loss_symbol)
        .expect("primal simple_loss must be present in the AIR program");
    assert_eq!(primal.params.len(), 2, "primal has two tensor params");
    for param in &primal.params {
        let (_, shape) = match analysis.types.get(param.ty) {
            Type::Tensor(elem, shape) => (*elem, *shape),
            other => panic!("primal param must be a tensor type, got: {other:?}"),
        };
        match analysis.types.get_index(shape) {
            IndexExpr::Tuple(dims) => {
                assert_eq!(dims.len(), 2, "primal param shape is rank 2 ([R, C])");
                assert!(
                    dims.iter()
                        .all(|dim| matches!(analysis.types.get_index(*dim), IndexExpr::Param(_))),
                    "both [R, C] dims must be symbolic Param dims"
                );
            }
            other => panic!("primal param shape must be a tuple of Param dims, got: {other:?}"),
        }
    }

    let mut lowered = lower_analyzed_unit(&mut analysis, Default::default()).expect("generic primal should lower to MIR");
    expect_codegen_ok(
        apply_air_backward_bundle(&mut lowered, bundle, false),
        "generic companion must install without a symbolic-shape error",
    );

    // The companion is generated once and installed as a distinct MIR
    // function next to the primal.
    let names = mir_function_names(&lowered.program, &lowered.interner);
    assert!(
        names.iter().any(|name| name == "loss_backward"),
        "companion 'loss_backward' must be installed in the lowered MIR; got functions: {names:?}"
    );
    assert!(
        names.iter().any(|name| name == "simple_loss"),
        "primal 'simple_loss' must remain in the lowered MIR; got functions: {names:?}"
    );

    // The companion-relation carrier records the primal→companion fact.
    assert!(
        !lowered.companions.is_empty(),
        "the companion relation carrier must record the installed companion"
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
    fixum tensor<f32, [R, C]> product ← x.multiplica(w)
    redde product.media()
}

functio nil() → vacuum {
    tacet
}

functio gradientes_simple_loss<magnitudo R, magnitudo C>(
    tensor<f32, [R, C]> x, tensor<f32, [R, C]> w, f32 upstream
) → tensor<f32, [R, C]> {
    fixum [gx, gw] ← loss_backward(x, w, nil(), upstream)
    redde gx
}
"#;

    let session = test_session(Target::MirLlvm);
    let mut analysis = analyze_source(&session, "generic_wrapper.fab", source, AnalyzeRequest::default())
        .expect("generic wrapper calling the generic companion must typecheck (OQ4)");

    // The wrapper's companion call is part of semantic analysis; reaching the
    // bundle path additionally proves the wrapper is not an AIR-lane function
    // (it calls the companion from plain code, mirroring gradus).
    let bundle = expect_codegen_ok(
        prepare_air_backward_bundle(&mut analysis),
        "backward bundle preparation should succeed",
    )
    .expect("an AIR lane with backward annotations must produce a bundle");

    let mut lowered = lower_analyzed_unit(&mut analysis, Default::default()).expect("generic wrapper should lower to MIR");
    expect_codegen_ok(
        apply_air_backward_bundle(&mut lowered, bundle, false),
        "generic companion must install without a symbolic-shape error",
    );
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
    tensor<f32, [R, 1]> a, tensor<f32, [R, C]> b
) → f32 {
    redde a.addita(b).media()
}
"#;

    let session = test_session(Target::MirLlvm);
    let mut analysis = analyze_source(&session, "generic_broadcast.fab", source, AnalyzeRequest::default())
        .expect("generic broadcast primal should analyze cleanly");

    let bundle = expect_codegen_ok(
        prepare_air_backward_bundle(&mut analysis),
        "backward bundle preparation should succeed",
    )
    .expect("an AIR lane with backward annotations must produce a bundle");

    // Load-bearing pre-intern check: the broadcast cotangent-reduction
    // intermediate `[R]` (a rank-1 shape of one `IndexExpr::Param` dim) must
    // now be interned in the type table, exactly as
    // `ensure_broadcast_reduction_types` does in
    // `prepare_air_backward_bundle`.
    let param_row_shape_interned = (0..analysis.types.index_count()).any(|i| {
        let id = crate::semantic::IndexId(u32::try_from(i).expect("index table fits u32"));
        matches!(
            analysis.types.get_index(id),
            IndexExpr::Tuple(dims)
                if dims.len() == 1
                    && matches!(analysis.types.get_index(dims[0]), IndexExpr::Param(_))
        )
    });
    assert!(
        param_row_shape_interned,
        "the broadcast-reduction intermediate shape [R] must be pre-interned \
         by prepare_air_backward_bundle (ensure_broadcast_reduction_types wiring)"
    );

    let mut lowered =
        lower_analyzed_unit(&mut analysis, Default::default()).expect("generic broadcast primal should lower to MIR");
    expect_codegen_ok(
        apply_air_backward_bundle(&mut lowered, bundle, false),
        "broadcast-using generic primal companion must generate without \
         reverse_ad_broadcast_reduction_missing_type",
    );

    let names = mir_function_names(&lowered.program, &lowered.interner);
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
    varia tensor<f32, [4]> acc ← x
    itera ab 0‥4 fixum i {
        acc ← acc.addita(x)
    }
    redde acc.media()
}
"#;

    let session = test_session(Target::MirLlvm);
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

    let mut lowered = lower_analyzed_unit(&mut analysis, Default::default()).expect("unrolled primal should lower to MIR");
    expect_codegen_ok(
        apply_air_backward_bundle(&mut lowered, bundle, false),
        "unrolled full-prefix body must generate a reverse_ad companion",
    );

    let names = mir_function_names(&lowered.program, &lowered.interner);
    assert!(
        names.iter().any(|name| name == "lane_sum_backward"),
        "companion 'lane_sum_backward' must be installed in the lowered MIR; got functions: {names:?}"
    );
    assert!(
        names.iter().any(|name| name == "lane_sum"),
        "primal 'lane_sum' must remain in the lowered MIR; got functions: {names:?}"
    );
}

// ── unit-3F: masked len ≤ N unrolled body generates a companion ──

#[test]
fn unrolled_masked_route_generates_companion_through_driver() {
    let source = r#"
@ radix lane "air"
@ radix backward "masked_mean_backward"
functio masked_lane_mean(tensor<f32, [4]> x, tensor<f32, [4]> mask) → f32 {
    varia tensor<f32, [4]> acc ← x
    itera ab 0‥4 fixum i {
        acc ← acc.addita(x)
    }
    redde acc.multiplica(mask).media()
}
"#;

    let session = test_session(Target::MirLlvm);
    let mut analysis =
        analyze_source(&session, "unroll_masked_mean.fab", source, AnalyzeRequest::default()).expect("mask-mode unrolled range loop must analyze");

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

    let mut lowered = lower_analyzed_unit(&mut analysis, Default::default()).expect("masked primal should lower to MIR");
    expect_codegen_ok(
        apply_air_backward_bundle(&mut lowered, bundle, false),
        "masked unrolled body must generate a reverse_ad companion",
    );

    let names = mir_function_names(&lowered.program, &lowered.interner);
    assert!(
        names.iter().any(|name| name == "masked_mean_backward"),
        "companion 'masked_mean_backward' must be installed in the lowered MIR; got functions: {names:?}"
    );
}
