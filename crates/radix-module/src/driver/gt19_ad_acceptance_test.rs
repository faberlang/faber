//! GT19 product-boundary AD acceptance receipts, la locale.
//!
//! GT19 AD acceptance — the rope_norm backward companion, end-to-end through
//! the driver.
//!
//! The single-file runner must install AIR backward companions just like the
//! package MIR path. This pins an annotated mean-of-product primal and its
//! interprocedural unit residual through the public source runner.
//!
//! # Front-end admission gap: indexed gather/scatter gradients
//!
//! The gather/scatter-add gradient chain is proven below at the AIR → MIR →
//! runtime-call boundary, not from source, because the front end admits no
//! source spelling for either indexed node on the AIR lane:
//!
//! - Source `gather` fails closed on a runtime `tensor<u32, [n]>` ids operand by
//!   design (K7c, `crates/radix-module/src/air/lower.rs:633-655`: "gather ids
//!   must be a literal integer array; a tensor<u32, [n]> ids operand is runtime
//!   data"). The AIR lane can only prove a statically literal id carrier.
//! - `scatter_add` has no source spelling at all
//!   (`crates/radix-air-nodes/src/nodes.rs:438-440`: source code cannot
//!   manufacture an AIR-only reverse node). It exists only as the compiler's
//!   reverse-AD VJP of `Gather`.
//! - No conversion on the AIR lane can turn a literal `lista<numerus>` into the
//!   literal ids carrier the node wants: `Conversio` fails closed
//!   (`crates/radix-module/src/air/lower.rs:433-480`, runtime conversion with no
//!   resolvable arm).
//!
//! Therefore no corpus exemplar for gather gradients exists in this need, and a
//! source-level admission is a separate language-change need rather than a
//! quiet extension here.

use crate::air::reverse_ad::transform_reverse_ad;
use crate::air::{
    AirBinding, AirBlock, AirExpr, AirFunction, AirFunctionId, AirLiteral, AirNameAllocator, AirParam, AirProgram,
    AirTensorOp, AirToMirContext, AirValueId, to_mir_program,
};
use crate::codegen::Target;
use crate::driver::{
    AnalyzeRequest, Config, Session, SpannedCodegenError, analyze_source, lower_mir_with_context_for_target,
};
use crate::hir::DefId;
use crate::lexer::{Span, Symbol};
use crate::mir::{
    BufferHost, MirAggregateFields, MirAggregateItem, MirCollectionOp, MirConstant, MirFunction, MirIntrinsic,
    MirOperand, MirPlaceBase, MirRuntimeCall, MirStatementKind, MirType, MirValidationContext, RunSourceError,
    run_source, validate_program as validate_mir_program,
};
use crate::test_support::test_session_la;
use handroll::FxHashMap;
use radix_types::{IndexExpr, NumericWidth, Primitive, Type, TypeId, TypeTable, i64s_from_literal_index};

/// Unwrap a driver result, panicking with the codegen error message
/// (`SpannedCodegenError` carries no `Debug` impl).
fn expect_codegen_ok<T>(result: Result<T, SpannedCodegenError>, what: &str) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("{what}: {}", error.message),
    }
}

/// Names of every lowered MIR function, resolved through the lowering
/// interner.
fn mir_function_names(program: &crate::mir::MirProgram, interner: &crate::lexer::Interner) -> Vec<String> {
    program
        .functions
        .iter()
        .filter_map(|f| f.name.map(|symbol| interner.resolve(symbol).to_owned()))
        .collect()
}

fn gt19_session() -> Session {
    Session::new(
        Config::default()
            .with_target(Target::MirFmirBinary)
            .with_dev_locale_root(),
    )
}

fn run_gt19_la(name: &str, source: &str) -> Vec<String> {
    let session = gt19_session();
    let mut host = BufferHost::default();
    match run_source(&session, name, source, &mut host) {
        Ok(()) => host.stdout_lines,
        Err(RunSourceError::Frontend(diagnostics)) => {
            panic!("frontend should analyze cleanly: {diagnostics:?}")
        }
        Err(RunSourceError::Mir(error)) => panic!("lowering should be clean: {error:?}"),
        Err(RunSourceError::Stepper(errors)) => panic!("runner should run: {errors:?}"),
    }
}

fn parse_fractus_list(line: &str) -> Vec<f64> {
    let inner = line.trim().trim_start_matches('[').trim_end_matches(']');
    inner
        .split(',')
        .map(|part| {
            part.trim()
                .parse::<f64>()
                .expect("list element must parse as fractus")
        })
        .collect()
}

/// Pinned `[2, 8]` fixture rows (mirrors `X_ROW_MAJOR` in
/// `rms_norm_intrinsic_test.rs`): row 0 is `[1.0..=8.0]`, row 1 is
/// `[0.5..=7.5]`. Both rows have non-zero mean.
const GT19_X_ROW_MAJOR: [f32; 16] = [
    1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, //
    0.5, 1.5, 2.5, 3.5, 4.5, 5.5, 6.5, 7.5,
];

/// Pinned affine gamma `[8]` (mirrors `GAMMA` in `rms_norm_intrinsic_test.rs`).
const GT19_GAMMA: [f32; 8] = [2.0, 1.0, 0.5, 1.5, 0.75, 1.25, 1.0, 0.5];

/// Analytic f64 reference for the pinned primal
/// `loss = media(x.rms_norm(axis 1, 1e-5, gamma))`. The forward mirrors
/// `rms_norm_reference` (per-row sum-of-squares mean, `r = 1/sqrt(mean+eps)`,
/// `y = x·r·gamma`); the chain rule through the full-tensor mean gives the
/// uniform upstream `1/(B·D)`, and per row with `s = mean(x²)+eps`:
///
/// - `dL/dx_i = gamma_i·u·r − x_i·r³·(Σ_j u·gamma_j·x_j)/n`
/// - `dL/dgamma_i = u·x_i·r`
fn gt19_rms_norm_mean_backward_reference(rows: usize, cols: usize) -> (f64, Vec<f64>, Vec<f64>) {
    const EPS: f64 = 1e-5;
    let n = f64::from(cols as u32);
    let upstream = 1.0 / f64::from(rows as u32 * cols as u32);
    let mut loss_sum = 0.0;
    let mut dx = vec![0.0; rows * cols];
    let mut dgamma = vec![0.0; cols];
    for row in 0..rows {
        let base = row * cols;
        let mean = GT19_X_ROW_MAJOR[base..base + cols]
            .iter()
            .map(|v| f64::from(*v) * f64::from(*v))
            .sum::<f64>()
            / n;
        let r = 1.0 / (mean + EPS).sqrt();
        let dot = (0..cols)
            .map(|j| upstream * f64::from(GT19_GAMMA[j]) * f64::from(GT19_X_ROW_MAJOR[base + j]))
            .sum::<f64>();
        for (j, cell) in dx[base..base + cols].iter_mut().enumerate() {
            let xi = f64::from(GT19_X_ROW_MAJOR[base + j]);
            *cell = f64::from(GT19_GAMMA[j]) * upstream * r - xi * r * r * r * dot / n;
            dgamma[j] += upstream * xi * r;
            loss_sum += xi * r * f64::from(GT19_GAMMA[j]);
        }
    }
    let loss = loss_sum / f64::from(rows as u32 * cols as u32);
    (loss, dx, dgamma)
}

/// Per-element close check (`atol = rtol = 1e-4`, the numeric-policy
/// gradient-row form used by `check_close` in `rms_norm_intrinsic_test.rs`).
fn gt19_check_close(name: &str, observed: &[f64], expected: &[f64]) {
    const ATOL: f64 = 1e-4;
    const RTOL: f64 = 1e-4;
    assert_eq!(observed.len(), expected.len(), "{name}: length mismatch");
    for (index, (observed, expected)) in observed.iter().zip(expected).enumerate() {
        assert!(
            (observed - expected).abs() <= ATOL + RTOL * expected.abs(),
            "{name}[{index}]: observed {observed} vs expected {expected}"
        );
    }
}

/// The GT19 rope backward companion fixture: an annotated `rope_norm` primal
/// over an interleaved `[rows, dim/2, 3]` table with
/// `theta_k = 8 * 100000^(-2k/8)`, cos and sin per pair, both rows identical.
/// `x` mirrors the pinned `rope_norm` fixture rows.
const GT19_ROPE_SOURCE: &str = r#"
@ radix lane "air"
@ radix backward "rope_backward"
functio rot(tf32[2, 8] x, tf32[2, 4, 3] table) → fractus {
    sit tensor<f32, [2, 8]> y ← x.rope_norm<8>(8, table)
    redde y.media()
}

functio unitas() → vacuum {
    tacet
}

incipit {
    sit lista<f32> flat ← [0.0, 0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 1.75, -1.5, -1.0, -0.5, 0.0, 0.5, 1.0, 1.5, -1.5]
    sit tensor<f32, [2, 8]> x ← flat ↦ tensor<f32, [2, 8]>
    sit lista<f32> table_flat ← [8.0, -0.1455000338, 0.9893582466, 0.4498730602, 0.9005023096, 0.434851228, 0.0252982213, 0.9996800171, 0.0252955229, 0.0014226235, 0.9999989881, 0.001422623, 8.0, -0.1455000338, 0.9893582466, 0.4498730602, 0.9005023096, 0.434851228, 0.0252982213, 0.9996800171, 0.0252955229, 0.0014226235, 0.9999989881, 0.001422623]
    sit tensor<f32, [2, 4, 3]> rope_table ← table_flat ↦ tensor<f32, [2, 4, 3]>
    sit [gx, gt] ← rope_backward(x, rope_table, unitas(), 1.0)
    nota gx
    nota gt
}
"#;

#[test]
fn gt19_rope_backward_companion_compiles() {
    let session = test_session_la(Target::MirLlvm);
    let mut analysis = analyze_source(&session, "gt19_rope_backward.fab", GT19_ROPE_SOURCE, AnalyzeRequest::default())
        .expect("rope backward should analyze cleanly");
    let lowered = expect_codegen_ok(
        lower_mir_with_context_for_target(&mut analysis, false),
        "rope backward should lower after bundle application",
    );
    let names = mir_function_names(lowered.validated.program(), &lowered.interner);
    assert!(
        names.iter().any(|name| name == "rope_backward"),
        "rope backward companion must be installed; got functions: {names:?}"
    );
}

#[test]
fn gt19_rms_norm_backward_runs_on_runner_path() {
    let source = r#"
functio unitas() → vacuum { tacet }

@ radix lane "air"
@ radix backward "norm_backward"
functio norm(tf32[2, 8] x, tf32[8] gamma) → fractus {
    redde x.rms_norm(axis 1, 1e-5, gamma).media()
}

incipit {
    sit lista<f32> flat ← [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 0.5, 1.5, 2.5, 3.5, 4.5, 5.5, 6.5, 7.5]
    sit lista<f32> gamma_flat ← [2.0, 1.0, 0.5, 1.5, 0.75, 1.25, 1.0, 0.5]
    sit tf32[2, 8] x ← flat ↦ tf32[2, 8]
    sit tf32[8] gamma ← gamma_flat ↦ tf32[8]
    sit iuncta<tf32[2, 8], tf32[8]> gs ← norm_backward(x, gamma, unitas(), 1.0)
    nota gs[0] ↦ lista<f32>
    nota gs[1] ↦ lista<f32>
}
"#;

    let stdout = run_gt19_la("gt19_rms_norm_backward.fab", source);
    assert_eq!(stdout.len(), 2, "both gradients should be printed");
    let grad_x = parse_fractus_list(&stdout[0]);
    let grad_gamma = parse_fractus_list(&stdout[1]);
    let (_, expected_dx, expected_dgamma) = gt19_rms_norm_mean_backward_reference(2, 8);
    gt19_check_close("grad_x", &grad_x, &expected_dx);
    gt19_check_close("grad_gamma", &grad_gamma, &expected_dgamma);
}

#[test]
fn gt19_mul_mean_backward_runs_on_runner_path() {
    let source = r#"
functio unitas() → vacuum { tacet }

@ radix lane "air"
@ radix backward "lin_backward"
functio lin(tf32[2] a, tf32[2] b) → fractus {
    sit tf32[2] p ← a ⊙ b
    redde p.media()
}

incipit {
    sit lista<f32> a_flat ← [2.0, 4.0]
    sit lista<f32> b_flat ← [1.0, 3.0]
    sit tf32[2] a ← a_flat ↦ tf32[2]
    sit tf32[2] b ← b_flat ↦ tf32[2]
    sit fractus loss ← lin(a, b)
    nota loss
    sit iuncta<tf32[2], tf32[2]> gs ← lin_backward(a, b, unitas(), 1.0)
    nota gs[0] ↦ lista<f32>
    nota gs[1] ↦ lista<f32>
}
"#;

    let stdout = run_gt19_la("gt19_mul_mean_backward.fab", source);
    assert_eq!(stdout.len(), 3, "primal and both gradients should be printed");
    let loss = stdout[0]
        .parse::<f64>()
        .expect("loss should render as a number");
    assert!((loss - 7.0).abs() < 1e-6, "mean product should be 7.0, got {loss}");
    let grad_a = parse_fractus_list(&stdout[1]);
    let grad_b = parse_fractus_list(&stdout[2]);
    assert_eq!(grad_a.len(), 2);
    assert_eq!(grad_b.len(), 2);
    for (observed, expected) in grad_a.iter().zip([0.5, 1.5]) {
        assert!((observed - expected).abs() < 1e-6, "grad_a: {grad_a:?}");
    }
    for (observed, expected) in grad_b.iter().zip([1.0, 2.0]) {
        assert!((observed - expected).abs() < 1e-6, "grad_b: {grad_b:?}");
    }
}

// ── Gather VJP → ScatterAdd: the GT19 planner receipt ────────────────────
//
// The AIR `Gather` VJP at the product boundary: the primal gathers rows of a
// `[V, D]` table with literal ids and the companion turns the upstream
// gradient into a zero-initialized `ScatterAdd` whose destination is implicit
// in its `[V, D]` result type. This test proves the whole hand-off — AIR
// companion → MIR `TensorScatterAdd` runtime call → pinned ids materialization
// → `[V, D]` result type — and fails if any of the three regresses.
//
// The runner-side execution of the identical intrinsic is pinned by
// `tensor_scatter_add_accumulates_repeated_ids_and_fails_closed_on_bad_id`
// (`crates/radix-mir-runner/src/lib_test.rs:4464`), which runs the same
// two-operand call on the interpreter: repeated ids sum in source order into
// their destination rows, untouched slots stay zero, and an out-of-range id
// fails closed. That interpreter (`Runner`) is `pub(crate)` to
// `radix-mir-runner` (`src/lib.rs:613`), so the runner itself is not reachable
// from this crate; the chain is asserted structurally here and the runner case
// cited for the semantics.

/// The gather ids from `reverse_ad_ops_test.rs` (`[2, 0, 2, 1, 2]`), narrowed
/// to `[2, 0, 2]` and paired with a `[4, 2]` table so destination row 3 is
/// disconnected: rows 1 and 3 are never touched and must stay zero.
const GT19_GATHER_IDS: [i64; 3] = [2, 0, 2];
/// Table rows of the pinned `[4, 2]` fixture (the scatter-add destination).
const GT19_TABLE_ROWS: u64 = 4;
/// Row width of the pinned fixture (gather rows and table rows share it).
const GT19_ROW_COLS: u64 = 2;

/// Shape extents are `u64` index literals; gradient shapes compare as `i64`.
fn gt19_extent(value: u64) -> i64 {
    i64::try_from(value).expect("a pinned extent fits in i64")
}

/// The gather primal `loss = sum(gathered rows)` as the AIR lane sees it:
/// `Gather(table<f32, [4, 2]>, ids<lista<numerus>>) -> f32[3, 2]`. Mirrors
/// `gather_fixture` in `crates/radix-air-bridge/src/reverse_ad_ops_test.rs`
/// with the narrower id vector.
fn gt19_gather_primal() -> (TypeTable, AirFunction, TypeId, TypeId) {
    let mut types = TypeTable::new();
    let value_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let id_ty = types.primitive(Primitive::Numerus);
    let ids_ty = types.array(id_ty);
    let gathered_rows = u64::try_from(GT19_GATHER_IDS.len()).expect("the pinned id vector is three ids");
    let dim_table = types.intern_index(IndexExpr::Literal(GT19_TABLE_ROWS));
    let dim_rows = types.intern_index(IndexExpr::Literal(gathered_rows));
    let dim_cols = types.intern_index(IndexExpr::Literal(GT19_ROW_COLS));
    let table_shape = types.intern_index(IndexExpr::Tuple(vec![dim_table, dim_cols]));
    let gather_shape = types.intern_index(IndexExpr::Tuple(vec![dim_rows, dim_cols]));
    let table_ty = types.tensor_with_shape(value_ty, table_shape);
    let gather_ty = types.tensor_with_shape(value_ty, gather_shape);
    types.unit();
    types.tuple(vec![table_ty]);

    let span = Span::new(0, 0);
    let mut bindings = Vec::with_capacity(GT19_GATHER_IDS.len() + 2);
    let mut id_values = Vec::with_capacity(GT19_GATHER_IDS.len());
    for (index, &id) in GT19_GATHER_IDS.iter().enumerate() {
        let value = AirValueId(10 + u32::try_from(index).expect("three pinned ids"));
        id_values.push(value);
        bindings.push(AirBinding { value, expr: AirExpr::Literal(AirLiteral::Int(id)), ty: id_ty, span });
    }
    let ids_value = AirValueId(100);
    bindings.push(AirBinding { value: ids_value, expr: AirExpr::Array(id_values), ty: ids_ty, span });
    let result_value = AirValueId(101);
    bindings.push(AirBinding {
        value: result_value,
        expr: AirExpr::TensorOp { op: AirTensorOp::Gather, args: vec![AirValueId(0), ids_value] },
        ty: gather_ty,
        span,
    });
    let primal = AirFunction {
        id: AirFunctionId(0),
        source: DefId(42),
        name: Symbol(0),
        params: vec![AirParam {
            value: AirValueId(0),
            source: DefId(42),
            name: Some(Symbol(0)),
            ty: table_ty,
            mode: crate::hir::HirParamMode::Owned,
            span,
        }],
        body: AirBlock::new(bindings, Some(result_value), span),
        return_ty: gather_ty,
        span,
    };
    (types, primal, table_ty, value_ty)
}

/// Intern the `lista<u32>` and `tensor<u32, [n]>` id carriers the MIR bridge
/// requires before it can materialize literal AIR ids (K7c).
fn gt19_intern_u32_id_carriers(types: &mut TypeTable) {
    let u32_ty = types.sized_numeric(Primitive::Numerus, NumericWidth::U32);
    let gathered_rows = u64::try_from(GT19_GATHER_IDS.len()).expect("the pinned id vector is three ids");
    let dim_count = types.intern_index(IndexExpr::Literal(gathered_rows));
    let ids_shape = types.intern_index(IndexExpr::Tuple(vec![dim_count]));
    types.array(u32_ty);
    types.tensor_with_shape(u32_ty, ids_shape);
}

fn gt19_runtime_calls(function: &MirFunction) -> Vec<&MirRuntimeCall> {
    function.blocks[0]
        .statements
        .iter()
        .filter_map(|statement| match &statement.kind {
            MirStatementKind::RuntimeCall { call, .. } => Some(call),
            _ => None,
        })
        .collect()
}

fn gt19_local_of(operand: &MirOperand) -> crate::mir::MirLocalId {
    let MirOperand::Place(place) = operand else {
        panic!("a collection intrinsic operand must be a local place, got {operand:?}");
    };
    assert!(
        place.projections.is_empty(),
        "a collection intrinsic operand must be a plain local"
    );
    match &place.base {
        MirPlaceBase::Local(local) => *local,
        other => panic!("expected a local place base, got {other:?}"),
    }
}

fn gt19_local_type(function: &MirFunction, local: crate::mir::MirLocalId) -> TypeId {
    function
        .locals
        .iter()
        .find(|declaration| declaration.id == local)
        .expect("the operand local is declared")
        .ty
        .semantic_id()
}

#[test]
fn gt19_gather_scatter_backward_chain() {
    let (mut types, primal, table_ty, value_ty) = gt19_gather_primal();
    let mut names = AirNameAllocator::new();
    let companion = transform_reverse_ad(
        &primal,
        &mut names,
        &types,
        &FxHashMap::default(),
        DefId(100),
        Symbol(42),
        &FxHashMap::default(),
    )
    .expect("Gather VJP transform");
    assert!(
        companion
            .body
            .bindings
            .iter()
            .any(|binding| matches!(&binding.expr, AirExpr::TensorOp { op: AirTensorOp::ScatterAdd, .. })),
        "the Gather VJP is a ScatterAdd node"
    );
    gt19_intern_u32_id_carriers(&mut types);

    let program = AirProgram { functions: vec![companion] };
    let context = AirToMirContext { interner: None, types: Some(&types), fusion_table: None };
    let mir = to_mir_program(&program, &context).expect("the ScatterAdd companion lowers to MIR");
    validate_mir_program(&mir, &MirValidationContext::new(&types)).expect("the lowered ScatterAdd companion validates");

    let function = &mir.functions[0];
    let calls = gt19_runtime_calls(function);
    let scatter_calls: Vec<&MirRuntimeCall> = calls
        .iter()
        .copied()
        .filter(|call| call.intrinsic == MirIntrinsic::Collection(MirCollectionOp::TensorScatterAdd))
        .collect();
    assert_eq!(scatter_calls.len(), 1, "exactly one ScatterAdd VJP call");
    let scatter = scatter_calls[0];
    // Runner-case operand shape (lib_test:4464): ids then updates, in that
    // order, with the destination implicit in the declared result type.
    assert_eq!(
        scatter.args.len(),
        2,
        "ScatterAdd takes [ids, updates]; the destination is implicit"
    );

    // The `[V, D]` result-type hand-off: the declared return type is the
    // table type, so the runner allocates a zeroed `[4, 2]` destination.
    assert_eq!(scatter.return_ty, MirType::semantic(table_ty));
    let Type::Tensor(result_element_ty, result_shape) = types.get(scatter.return_ty.semantic_id()) else {
        panic!("the ScatterAdd result type must be the table tensor type");
    };
    assert_eq!(*result_element_ty, value_ty);
    assert_eq!(
        i64s_from_literal_index(*result_shape, &types),
        Some(vec![gt19_extent(GT19_TABLE_ROWS), gt19_extent(GT19_ROW_COLS)])
    );

    // ids operand: a rank-1 `tensor<u32, [3]>` local, exactly the runner's
    // rank-1 id-tensor precondition.
    let ids_local = gt19_local_of(&scatter.args[0]);
    let u32_ty = types.sized_numeric(Primitive::Numerus, NumericWidth::U32);
    let Type::Tensor(ids_element_ty, ids_shape) = types.get(gt19_local_type(function, ids_local)) else {
        panic!("the ScatterAdd ids operand must be a tensor local");
    };
    assert_eq!(*ids_element_ty, u32_ty);
    assert_eq!(
        i64s_from_literal_index(*ids_shape, &types),
        Some(vec![gt19_extent(
            u64::try_from(GT19_GATHER_IDS.len()).expect("three pinned ids")
        )])
    );

    // updates operand: the `[3, 2]` upstream gradient of the gather result.
    let updates_local = gt19_local_of(&scatter.args[1]);
    let Type::Tensor(updates_element_ty, updates_shape) = types.get(gt19_local_type(function, updates_local)) else {
        panic!("the ScatterAdd updates operand must be a tensor local");
    };
    assert_eq!(*updates_element_ty, value_ty);
    assert_eq!(
        i64s_from_literal_index(*updates_shape, &types),
        Some(vec![
            gt19_extent(u64::try_from(GT19_GATHER_IDS.len()).expect("three pinned ids")),
            gt19_extent(GT19_ROW_COLS)
        ])
    );

    // The ids materialization: the ids local is produced by a
    // `TensorFromFlat(seed, lista<u32>, lista<numerus> shape)` call.
    let materialize = function.blocks[0]
        .statements
        .iter()
        .find_map(|statement| match &statement.kind {
            MirStatementKind::RuntimeCall { destination: Some(destination), call }
                if call.intrinsic == MirIntrinsic::Collection(MirCollectionOp::TensorFromFlat)
                    && destination.base == MirPlaceBase::Local(ids_local) =>
            {
                Some(call)
            }
            _ => None,
        })
        .expect("the ScatterAdd ids operand must be materialized by TensorFromFlat");
    assert_eq!(
        materialize.args.len(),
        3,
        "TensorFromFlat takes [seed, flat lista<u32>, shape lista<numerus>]"
    );
    let flat_local = gt19_local_of(&materialize.args[1]);
    let expected_list_ty = types
        .find_array(u32_ty)
        .expect("the lista<u32> id carrier is interned");
    assert_eq!(gt19_local_type(function, flat_local), expected_list_ty);

    // ...and that flat list carries the literal ids [2, 0, 2], in source order
    // with the repeat preserved: the runner sums repeated ids in source order
    // and leaves untouched destination rows zero.
    let fields = function.blocks[0]
        .statements
        .iter()
        .find_map(|statement| match &statement.kind {
            MirStatementKind::Construct { destination, aggregate }
                if destination.base == MirPlaceBase::Local(flat_local) =>
            {
                match &aggregate.fields {
                    MirAggregateFields::Ordered(fields) => Some(fields.clone()),
                    _ => None,
                }
            }
            _ => None,
        })
        .expect("the ids lista<u32> must be constructed in the block");
    let ids: Vec<u64> = fields
        .iter()
        .map(|field| match field {
            MirAggregateItem::Operand(MirOperand::Constant(MirConstant::UInt(id))) => *id,
            other => panic!("a materialized id must be a u32 constant, got {other:?}"),
        })
        .collect();
    assert_eq!(ids, vec![2, 0, 2], "the ids materialization carries the literal ids");
    // Destination row 3 is disconnected: ids stay inside the [4, 2] table and
    // never name row 3, so rows 1 and 3 are untouched and stay zero.
    assert!(
        ids.iter().all(|id| *id < GT19_TABLE_ROWS),
        "every id lies inside the destination table: {ids:?}"
    );
    assert!(!ids.contains(&3), "row 3 of the [4, 2] destination is disconnected: {ids:?}");
}
