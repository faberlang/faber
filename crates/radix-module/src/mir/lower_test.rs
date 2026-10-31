//! Tests for HIR-to-MIR lowering.
//!
//! Coverage-law W3-C: split with section headers, edge siblings,
//! and tightened assertions.
//!
//! Section map:
//!   A — Function shells, params, locals, entry prefix
//!   B — Constants, literals, return values
//!   C — Matrix / vector / tensor operations
//!   D — Control flow: si, elige, dum, fac/cape, rumpe/perge, itera
//!   E — Discerne & enum variant operations
//!   F — Failable functions & error handlers
//!   G — Direct calls, intrinsics, methods
//!   H — Aggregate construction (struct, array, map, set, tensor)
//!   I — Collection runtime methods
//!   J — Closures, callable values, captured environments
//!   K — Generic monomorphization & explicit type args
//!   L — Builder-level unit tests (manual HIR construction)
//!   M — Shader stage identity (compute / vertex / fragment)

use super::*;
use crate::codegen::Target;
use crate::driver::AnalyzedModule;
use crate::driver::{AnalyzeRequest, Config, ModuleCompileRequest, ModuleImportContract, Session};
use crate::hir::HirId;
use crate::mir::type_contains_param;
use crate::mir::validate_program;
use std::path::PathBuf;

fn la_config() -> Config {
    Config::default().with_latin_locale()
}

fn analyze(source: &str) -> AnalyzedModule {
    let session = Session::new(la_config().with_target(Target::HirTypeScript));
    crate::driver::analyze_source(&session, "test.fab", source, AnalyzeRequest::default()).expect("analysis succeeds")
}

fn dump_source(source: &str) -> String {
    let unit = analyze(source);
    dump_analyzed_unit(unit.application_view()).expect("MIR lowering succeeds")
}

#[test]
fn lowers_decimal_ordering_on_the_shared_scaled_carrier() {
    let dump = dump_source(
        r"
functio maior(d64 a, d64 b) → bivalens {
    redde a ≻ b
}
",
    );
    assert_eq!(
        dump.matches("runtime convert runtime").count(),
        0,
        "same-width decimals compare with no conversion:\n{dump}"
    );
    assert!(
        dump.contains(" > "),
        "comparison must remain a primitive MIR ordering op:\n{dump}"
    );
}

#[test]
fn lowers_decimal_integer_ordering_at_the_d64_scaled_carrier() {
    let dump = dump_source(
        r"
functio maior(d64 a, numerus n) → bivalens {
    redde a ≻ n
}
",
    );
    // Ruling 22: a decimal beside a non-constant integer compares by exact
    // value; neither operand is converted (a conversion could trap).
    assert_eq!(
        dump.matches("runtime convert runtime").count(),
        0,
        "decimal and integer operands must not be joined:\n{dump}"
    );
    assert!(
        dump.contains(" > "),
        "comparison must stay a primitive MIR ordering op:\n{dump}"
    );
}

#[test]
fn lowers_decimal_equality_on_the_shared_scaled_carrier() {
    let dump = dump_source(
        r"
functio aequalis(d64 a, d64 b) → bivalens {
    redde a ≅ b
}
",
    );
    assert_eq!(
        dump.matches("runtime convert runtime").count(),
        0,
        "same-width decimals compare with no conversion:\n{dump}"
    );
    assert!(
        dump.contains(" =="),
        "comparison must remain a primitive MIR equality op:\n{dump}"
    );
}

#[test]
fn lowers_decimal_literal_text_to_the_exact_carrier_across_mir() {
    // need: the original digit text — not the f64 re-derivation —
    // must decide the MIR constant. 92233720368.54775807 × 10⁸ is exactly
    // i64::MAX; a carrier derived from the rounded f64 leaves the width.
    let dump = dump_source(
        r"
functio exacta() → d64 {
    sit d64 x ← 92233720368.54775807
    redde x
}
",
    );
    assert!(
        dump.contains("const int 9223372036854775807"),
        "the exact scaled carrier must reach MIR:\n{dump}"
    );
    assert!(
        !dump.contains("const float"),
        "no float constant may intermediate on the decimal path:\n{dump}"
    );
}

#[test]
fn decimal_literal_executes_at_the_exact_scaled_carrier() {
    // need: the accepted literal runs at its exact carrier; any f64
    // intermediation rounds and the runner's re-derivation lands out of range.
    let mut host = crate::mir::BufferHost::default();
    crate::mir::run_source(
        &Session::new(la_config().with_target(Target::HirTypeScript)),
        "decimal-exact-carrier.fab",
        r"incipit {
    sit d64 x ← 92233720368.54775807
    nota x
}",
        &mut host,
    )
    .unwrap_or_else(|error| panic!("exact decimal carrier must run: {error:?}"));
    assert_eq!(host.stdout_lines, ["92233720368.54775807"]);
}

#[test]
fn decimal_potentia_executes_exactly_at_the_scaled_carrier() {
    // Audit GAP #24: a `d64` receiver runs `potentia`'s exponent at plain
    // `numerus`. Lowering the by-squaring loop with a decimal-typed exponent
    // made its bare `Int(2)` divisors 2e-8, so halving grew the exponent
    // until the carrier overflowed (`numerus division failed`) instead of
    // landing on the exact 2.25.
    let mut host = crate::mir::BufferHost::default();
    crate::mir::run_source(
        &Session::new(la_config().with_target(Target::HirTypeScript)),
        "decimal-potentia.fab",
        r"incipit {
    sit d64 a ← 1.5
    nota a.potentia(2)
}",
        &mut host,
    )
    .unwrap_or_else(|error| panic!("decimal potentia must run: {error:?}"));
    assert_eq!(host.stdout_lines, ["2.25"]);
}

#[test]
fn decimal_literal_switch_matches_at_the_exact_carrier() {
    // need: patterns lower through the same exact-carrier seam as
    // the scrutinee, so the case compares Int carrier against Int carrier.
    let mut host = crate::mir::BufferHost::default();
    crate::mir::run_source(
        &Session::new(la_config().with_target(Target::HirTypeScript)),
        "decimal-exact-switch.fab",
        r#"incipit {
    sit d64 x ← 4.2
    elige x {
        casu 4.2 { nota "hit" }
        ceterum { nota "miss" }
    }
}"#,
        &mut host,
    )
    .unwrap_or_else(|error| panic!("decimal switch must run: {error:?}"));
    assert_eq!(host.stdout_lines, ["hit"]);
}

#[test]
fn lowers_decimal_fuzzy_equality_on_the_d64_scaled_carrier() {
    let dump = dump_source(
        r"
functio aequalis(d64 a, d64 b) → bivalens {
    redde a ≈ b
}
",
    );
    assert_eq!(
        dump.matches("runtime convert runtime").count(),
        0,
        "same-width fuzzy equality needs no conversion:\n{dump}"
    );
}

#[test]
fn queue_and_stack_methods_lower_to_core_collection_walkers() {
    let queue = dump_source(
        r"incipit {
    varia queue<numerus> q ← vacua
    q.appende(1)
    sit numerus ∪ nihil item ← q.decapita()
    sit numerus size ← q.longitudo()
    nota item
    nota size
}",
    );
    assert!(queue.contains("runtime collection append"), "missing queue push MIR: {queue}");
    assert!(
        queue.contains("runtime collection remove_first"),
        "missing queue pop MIR: {queue}"
    );
    assert!(queue.contains("runtime collection length"), "missing queue length MIR: {queue}");

    let stack = dump_source(
        r#"incipit {
    varia stack<textus> s ← vacua
    s.appende("top")
    sit textus ∪ nihil item ← s.detrahe()
    nota item
}"#,
    );
    assert!(stack.contains("runtime collection append"), "missing stack push MIR: {stack}");
    assert!(
        stack.contains("runtime collection remove_last"),
        "missing stack pop MIR: {stack}"
    );
}

/// Return the body text of the first `bb0:` block in a MIR dump.
///
/// WHY: breakable-block CFG bugs seal the entry block with `unreachable` while
/// still emitting stray `goto bb` edges elsewhere. Asserting on the entry block
/// specifically catches regressions that whole-dump `contains` checks hide.
fn first_block_body(dump: &str) -> String {
    let start = dump.find("bb0:").expect("dump has an entry block");
    let body_start = start + "bb0:".len();
    let end = dump[body_start..]
        .find("\n  bb")
        .or_else(|| dump[body_start..].find("\n}"))
        .map_or(dump.len(), |offset| body_start + offset);
    dump[body_start..end].to_owned()
}

fn unsupported_range_expr(numerus: crate::semantic::TypeId, span: crate::lexer::Span, first_id: u32) -> HirExpression {
    // An interval clamp with a recovery value is the interval form MIR lowering
    // still declines (stepped interval values lower since sb314).
    let literal = |offset: u32, value: u64| HirExpression {
        id: HirId(first_id + offset),
        kind: HirExpressionKind::Literal(HirLiteral::Int(value)),
        ty: Some(numerus),
        span,
    };
    HirExpression {
        id: HirId(first_id),
        kind: HirExpressionKind::Conversio {
            source: Box::new(literal(1, 5)),
            target: crate::hir::HirConversioTarget::Intervallum(Box::new(HirExpression {
                id: HirId(first_id + 2),
                kind: HirExpressionKind::Intervallum {
                    start: Box::new(literal(3, 0)),
                    end: Box::new(literal(4, 1)),
                    step: None,
                    kind: HirRangeKind::Exclusive,
                },
                ty: Some(numerus),
                span,
            })),
            params: Vec::new(),
            recovery: Some(Box::new(literal(5, 0))),
            route: None,
            implicit: None,
        },
        ty: Some(numerus),
        span,
    }
}

// ===========================================================================
// Section A: Function shells, params, locals, entry prefix
// ===========================================================================

#[test]
fn lowers_empty_function_shell_to_mir_dump() {
    let dump = dump_source("functio saluta() {}");

    assert_eq!(
        dump,
        "\
function f0 -> ty#5 {
  bb0:
    return
}
"
    );
}

// ===========================================================================
// Section B: Constants, literals, return values
// ===========================================================================

#[test]
fn lowers_top_level_static_as_entry_prefix_local() {
    // The top-level constant is `generis` (a static, D5.8-D5.9) or, since
    // st1, a top-level `fixum T X = e`; both spellings carry the same HIR
    // constant. MIR entry lowering (`mir/lower.rs::lower_entry`) lowers every
    // top-level `HirItemKind::Constant` item as a local prefixed onto the
    // entry function; there is no separate MIR-level static/global
    // representation.
    let dump = dump_source(
        r"
fixum numerus age = 25

incipit {
    nota age
}
",
    );

    assert!(dump.contains("let _0: ty#1"));
    assert!(dump.contains("_0 = const int 25: ty#1"));
    assert!(dump.contains("runtime diagnostic nota(_0)"));
    assert!(!dump.contains("top-level const"));
}

#[test]
fn top_level_fixum_constant_lowers_like_the_static_including_the_hole() {
    // st1 R1/R2: `fixum T X = e` and `fixum _ X = e` at top level are the
    // same constant the `generis` spelling names; `_` infers the type.
    let static_dump = dump_source("fixum numerus age = 25\n\nincipit {\n    nota age\n}\n");
    for spelling in ["fixum numerus age = 25", "fixum _ age = 25"] {
        let dump = dump_source(&format!("{spelling}\n\nincipit {{\n    nota age\n}}\n"));
        assert_eq!(dump, static_dump, "{spelling}");
    }
}

#[test]
fn lowers_top_level_static_paths_inside_functions() {
    // D5.7 retired top-level `fixum`; the top-level equivalent is `generis`.
    let dump = dump_source(
        r"
fixum numerus answer = 42

functio readAnswer() → numerus {
    redde answer
}
",
    );

    assert!(dump.contains("let _0: ty#1"));
    assert!(dump.contains("_0 = const int 42: ty#1"));
    assert!(dump.contains("return _0"));
    assert!(!dump.contains("path that does not resolve to a local value"));
}

#[test]
fn top_level_conversio_init_rejects_as_top_level_binding() {
    // Used to prove: an item-scope `↤` conversion-directed initializer
    // lowers through the shared conversion owner exactly once. D5.7 retired
    // top-level `fixum`/`varia` (SEM062); `generis` statics accept only a
    // compile-time-evaluable `=` initializer (D5.9), which `↤` (a runtime
    // conversion) can never be — `generis`'s own grammar only accepts `=`.
    // There is no static equivalent for this fixture; assert the rejection.
    let session = Session::new(la_config().with_target(Target::HirTypeScript));
    let source = r#"
sit numerus answer ↤ "42"

incipit {
    nota answer
}
"#;
    let Err(errors) = crate::driver::analyze_source(&session, "test.fab", source, AnalyzeRequest::default()) else {
        panic!("item-scope ↤ init must fail closed at analysis (SEM062)")
    };
    assert!(
        errors
            .iter()
            .any(|error| error.issue() == Some("top_level_binding")),
        "expected top_level_binding rejection: {:?}",
        errors
            .iter()
            .map(|e| (e.code, e.issue()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn lowers_atomic_methods_to_dedicated_mir_intrinsics() {
    let dump = dump_source(
        r"
functio atomic_ops(varia atomic<i32> cell, i32 value) → bivalens {
    sit i32 loaded ← cell.load()
    cell.store(value)
    sit i32 old ← cell.exchange(loaded)
    redde cell.compare_exchange(old, value)
}
",
    );

    assert!(dump.contains("runtime atomic load(_0)"));
    assert!(dump.contains("runtime atomic store(_0, _1)"));
    assert!(dump.contains("runtime atomic exchange(_0, _2)"));
    assert!(dump.contains("runtime atomic compare_exchange(_0, _3, _1)"));
    assert!(!dump.contains("method call before runtime/provider MIR lowering"));
}

#[test]
fn lowers_function_params_into_mir_params() {
    let dump = dump_source("functio saluta(textus nomen, numerus aetas) {}");

    assert_eq!(
        dump,
        "\
function f0 -> ty#5 {
  params:
    _0: ty#0
    _1: ty#1
  locals:
    let _0: ty#0
    let _1: ty#1
  bb0:
    return
}
"
    );
}

#[test]
fn lowers_function_params_as_mir_locals() {
    let unit = analyze("functio saluta(textus nomen, numerus aetas) {}");
    let program = lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default())
        .map(|lowered| lowered.validated.into_parts().0)
        .expect("MIR lowering succeeds");
    let function = &program.functions[0];

    assert_eq!(function.params.len(), 2);
    assert_eq!(function.locals.len(), 2);
    assert_eq!(function.params[0].local, function.locals[0].id);
    assert_eq!(function.params[1].local, function.locals[1].id);
    assert!(!function.locals[0].mutable);
    assert!(!function.locals[1].mutable);
}

#[test]
fn scalar_mut_param_preserves_in_mode_through_mir_lowering() {
    let source = r#"
functio duplica(varia numerus value) → vacuum {
    value ← value * 2
}

incipit {
    nota "mutabile"
    varia numerus value ← 3
    duplica(varia value)
    nota value
    nota "salve"
}
"#;
    let unit = analyze(source);
    let program = lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default())
        .map(|lowered| lowered.validated.into_parts().0)
        .expect("MIR lowering succeeds");
    let function = &program.functions[0];

    assert_eq!(function.params[0].mode, MirParamMode::MutRef);
    assert!(function.locals[0].mutable, "in parameter must be writable in MIR");
    assert!(function.blocks[0].statements.iter().any(|statement| {
        matches!(
            &statement.kind,
            MirStatementKind::Assign { place, .. } if *place == MirPlace::local(function.params[0].local)
        )
    }));

    let mut host = crate::mir::BufferHost::default();
    crate::mir::run_source(
        &Session::new(la_config().with_target(Target::HirTypeScript)),
        "scalar-mut-param.fab",
        source,
        &mut host,
    )
    .unwrap_or_else(|error| panic!("MIR runner proof should execute: {error:?}"));
    assert_eq!(host.stdout_lines, ["mutabile", "6", "salve"]);
}

#[test]
fn runtime_size_declaration_rejects_negative_lift_and_accepts_nonnegative() {
    let session = Session::new(la_config().with_target(Target::HirTypeScript));
    let negative = r#"+++
locale = "en"
+++
main {
    mut int h ← -1
    const size H ← h
    print H
}
"#;
    let mut host = crate::mir::BufferHost::default();
    let error = crate::mir::run_source(&session, "negative-size-lift.fab", negative, &mut host)
        .expect_err("negative runtime size lift must fail");
    let crate::mir::RunSourceError::Stepper(errors) = error else {
        panic!("negative lift should fail in the runner: {error:?}");
    };
    assert!(
        errors
            .iter()
            .any(|error| error.issue == "size_lift_negative"),
        "{errors:?}"
    );

    let positive = negative.replace("h ← -1", "h ← 2");
    let mut host = crate::mir::BufferHost::default();
    crate::mir::run_source(&session, "positive-size-lift.fab", &positive, &mut host)
        .expect("nonnegative runtime size lift must succeed");
    assert_eq!(host.stdout_lines, ["2"]);
}

// ===========================================================================
// Section C: Matrix / vector / tensor operations
// ===========================================================================

#[test]
fn lowers_matrix_elementwise_method_to_matrix_cells() {
    let dump = dump_source(
        r"
functio matrix_ops(matrix<f32, [2, 2]> a, matrix<f32, [2, 2]> b) → matrix<f32, [2, 2]> {
    redde a.addita(b)
}
",
    );

    assert!(dump.contains("_0.cell0_0 + _1.cell0_0"), "{dump}");
    assert!(dump.contains("_0.cell0_1 + _1.cell0_1"), "{dump}");
    assert!(dump.contains("_0.cell1_0 + _1.cell1_0"), "{dump}");
    assert!(dump.contains("_0.cell1_1 + _1.cell1_1"), "{dump}");
    assert!(dump.contains("construct matrix"), "{dump}");
    assert!(!dump.contains("matrix method before MIR lowering"), "{dump}");
}

#[test]
fn lowers_matrix_matmul_to_cell_mul_add() {
    let dump = dump_source(
        r"
functio product(matrix<f32, [2, 2]> a, matrix<f32, [2, 2]> b) → matrix<f32, [2, 2]> {
    redde a.matmul(b)
}
",
    );

    assert!(dump.contains("_0.cell0_0 * _1.cell0_0"), "{dump}");
    assert!(dump.contains("_0.cell0_1 * _1.cell1_0"), "{dump}");
    assert!(dump.contains("construct matrix"), "{dump}");
    assert!(!dump.contains("matrix method before MIR lowering"), "{dump}");
}

#[test]
fn lowers_matrix_applica_to_cell_lane_mul_add() {
    let dump = dump_source(
        r"
functio apply(matrix<f32, [2, 2]> m, vector<f32, magnitudo 2> v) → vector<f32, magnitudo 2> {
    redde m.applica(v)
}
",
    );

    assert!(dump.contains("_0.cell0_0 * _1.lane0"), "{dump}");
    assert!(dump.contains("_0.cell0_1 * _1.lane1"), "{dump}");
    assert!(dump.contains("construct vector"), "{dump}");
    assert!(!dump.contains("matrix method before MIR lowering"), "{dump}");
}

#[test]
fn lowers_matrix_transpone_to_cell_permutation() {
    let dump = dump_source(
        r"
functio swap(matrix<f32, [2, 2]> m) → matrix<f32, [2, 2]> {
    redde m.transpone()
}
",
    );

    assert!(dump.contains("construct matrix"), "{dump}");
    assert!(dump.contains("_0.cell0_1"), "{dump}");
    assert!(dump.contains("_0.cell1_0"), "{dump}");
    assert!(!dump.contains('*'), "transpose is a pure permutation, no arithmetic\n{dump}");
    assert!(!dump.contains("matrix method before MIR lowering"), "{dump}");
}

#[test]
fn lowers_matrix_transpone_non_square_swaps_shape() {
    let dump = dump_source(
        r"
functio swap(matrix<f32, [2, 3]> m) → matrix<f32, [3, 2]> {
    redde m.transpone()
}
",
    );

    // Result [3, 2]: row-major fields visit receiver cells column-major.
    assert!(dump.contains("construct matrix"), "{dump}");
    assert!(
        dump.contains("[_0.cell0_0, _0.cell1_0, _0.cell0_1, _0.cell1_1, _0.cell0_2, _0.cell1_2]"),
        "transpose [2,3]→[3,2] must permute receiver cells column-major\n{dump}"
    );
    assert!(!dump.contains("matrix method before MIR lowering"), "{dump}");
}

#[test]
fn lowers_vector_norma_to_sqrt_of_dot() {
    // R-W3-lane (wave3 residual C-1): assert the full sum-of-squares structure.
    // The prior `contains(".lane0")` OR-fallback was near-vacuous for any vector dump.
    let dump = dump_source(
        r"
functio length(vector<f32, magnitudo 3> v) → f32 {
    redde v.norm()
}
",
    );
    assert!(dump.contains("sqrt"), "norm must lower through sqrt\n{dump}");
    assert!(
        dump.contains("_0.lane0 * _0.lane0"),
        "norm must square lane0 for dot(v,v)\n{dump}"
    );
    assert!(
        dump.contains("_0.lane1 * _0.lane1"),
        "norm must square lane1 for vector<f32,magnitudo 3>\n{dump}"
    );
    assert!(
        dump.contains("_0.lane2 * _0.lane2"),
        "norm must square lane2 for vector<f32,magnitudo 3>\n{dump}"
    );
}

#[test]
fn lowers_matrix_inversa_2x2_to_det_and_option() {
    let dump = dump_source(
        r"
functio inv(matrix<f32, [2, 2]> m) → matrix<f32, [2, 2]> ∪ nihil {
    redde m.inversa()
}
",
    );
    assert!(
        dump.contains("construct matrix"),
        "inversa must build a matrix in the some arm\n{dump}"
    );
    assert!(dump.contains("some("), "inversa must wrap the matrix in option some\n{dump}");
    assert!(
        dump.contains("none"),
        "inversa must produce option none for singular input\n{dump}"
    );
}

#[test]
fn lowers_matrix_materialize_from_nested_lista() {
    let dump = dump_source(
        r"
functio identitas() → matrix<f32, [2, 2]> {
    redde [[1.0, 0.0], [0.0, 1.0]] ↦ matrix<f32, [2, 2]>
}
",
    );

    assert!(dump.contains("construct matrix"), "{dump}");
    assert!(dump.contains("const float 1.0") || dump.contains("const float 1"), "{dump}");
}

#[test]
fn lowers_explicit_no_value_redde_as_trivial_return() {
    let dump = dump_source("functio saluta() → vacuum { redde }");

    assert_eq!(
        dump,
        "\
function f0 -> ty#5 {
  bb0:
    return
}
"
    );
}

#[test]
fn gpu_builtin_call_never_reaches_mir_lowering() {
    // A thread-id builtin is a raw lane id (gpu-reset rule 12.5): user source
    // that calls one is rejected at check time, so HIR-to-MIR lowering only
    // sees builtin reads the compiler binds itself (lane parameters).
    let session = Session::new(la_config().with_target(Target::HirTypeScript));
    let errors = crate::driver::analyze_source(
        &session,
        "test.fab",
        r"
@ nucleum
functio index() → u32 {
    redde global_invocation_x()
}
",
        AnalyzeRequest::default(),
    )
    .err()
    .expect("a user-written thread-id call is rejected");

    assert!(
        errors
            .iter()
            .any(|error| error.issue() == Some("nucleum_raw_lane_id")),
        "{errors:?}"
    );
}

#[test]
fn lowers_primitive_constants_and_explicit_redde() {
    let dump = dump_source(
        r#"functio constantes() → bivalens {
  varia textus t ← "salve"
  varia fractus f ← 1.5
  varia bivalens b ← verum
  redde b
}"#,
    );

    assert_eq!(
        dump,
        // `varia` (Batch E) and `verum` (Batch C) are tokenless, so they are
        // interned as identifiers and shift the string constant's symbol
        // index (sym#6 rather than sym#4).
        "\
function f0 -> ty#3 {
  locals:
    var _0: ty#0
    var _1: ty#2
    var _2: ty#3
  bb0:
    _0 = const string sym#6: ty#0
    _1 = const float 1.5: ty#2
    _2 = const bool true: ty#3
    return _2
}
"
    );
}

#[test]
fn lowers_vacua_to_empty_collection_aggregates() {
    let dump = dump_source(
        r"functio inanes() → numerus {
  sit lista<numerus> numeri ← vacua
  sit tabula<textus, numerus> pondera ← vacua
  sit copia<textus> nomina ← vacua
  redde 0
}",
    );

    assert!(dump.contains("construct array:"));
    assert!(dump.contains("construct map:"));
    assert!(dump.contains("construct set:"));
    assert!(dump.contains("[]"));
    assert!(dump.contains("{}"));
}

#[test]
fn lowers_tensor_vacua_to_rank_zero_aggregate() {
    let dump = dump_source(
        r"functio empty_tensor() → numerus {
  sit tensor<f32, []> grid ← vacua
  redde 0
}",
    );

    assert!(dump.contains("construct tensor:"));
}

#[test]
fn lowers_genus_method_calls_with_explicit_receiver_param() {
    let dump = dump_source(
        r"genus Capsa {
  varia numerus n = 1
  functio value() → numerus { redde ego.n }
}

incipit {
  sit _ c ← Capsa { n = 4 }
  nota c.value()
}",
    );

    assert!(dump.contains("function f0 -> ty#1 {\n  params:\n    _0:"));
    assert!(dump.contains("return _0.sym#"));
    assert!(dump.contains("%1 = call def#"));
    assert!(dump.contains("runtime diagnostic nota(%1)"));
}

#[test]
fn lowers_non_nullable_coalesce_without_requiring_nullable_lhs() {
    let dump = dump_source(
        r"incipit {
  sit _ zero ← 0
  nota zero vel 999
}",
    );

    assert!(dump.contains("option coalesce("));
    assert!(dump.contains("runtime diagnostic nota("));
}

#[test]
fn lowers_coalesce_with_effectful_fallback_to_a_nil_branch() {
    // Operator ruling 2026-09-24: `vel` short-circuits like `et`/`aut` — the
    // fallback runs only when the left operand is `nihil`, so an effectful
    // fallback must lower into its own block behind a nil check.
    let dump = dump_source(
        r"
functio forte() → numerus ∪ nihil { redde nulla }
functio latus() → numerus {
    nota 1
    redde 9
}
incipit {
  sit numerus x ← forte() vel latus()
  nota x
}",
    );
    let entry = dump.split("function f2").nth(1).unwrap_or_default();
    assert!(!entry.contains("option coalesce("), "{entry}");
    assert!(entry.contains("= is_nil %"), "{entry}");
    assert!(entry.contains("option unwrap_assume("), "{entry}");
    let calls: Vec<usize> = entry
        .lines()
        .enumerate()
        .filter(|(_, l)| l.contains("= call def#"))
        .map(|(i, _)| i)
        .collect();
    let branch_line = entry
        .lines()
        .position(|l| l.trim_start().starts_with("branch "));
    assert_eq!(calls.len(), 2, "{entry}");
    assert!(
        branch_line.is_some_and(|b| calls[0] < b && b < calls[1]),
        "fallback call must sit behind the nil branch:\n{entry}"
    );
}

#[test]
fn lowers_coalesce_with_literal_fallback_as_single_option_op() {
    let dump = dump_source(r#"functio maybe(textus ∪ nihil name) → textus { redde name vel "ignotus" }"#);
    assert!(dump.contains("option coalesce(_0, const string sym#"), "{dump}");
    assert!(!dump.contains("branch "), "{dump}");
}

#[test]
fn lowers_optional_chain_on_known_present_base() {
    let dump = dump_source(
        r#"genus Address {
  varia textus city
}

genus User {
  varia textus name
  varia Address address sponte
}

incipit {
  sit _ alice ← User { name = "Alice", address = Address { city = "Roma" } }
  nota alice?.address?.city
}"#,
    );

    assert!(dump.contains("option chain("));
    assert!(dump.contains("runtime diagnostic nota("));
}

// WHY: lowers_field_projection_from_object_literal_map_shape was removed in
// literal-family Stage 6. It tested field projection (`.x`) from anonymous
// object literals, a retired feature now that bare `{ }` is JSON valor.
#[test]
fn materializes_constant_int_redde_with_type() {
    let dump = dump_source("functio unum() → numerus { redde 1 }");
    assert_eq!(
        dump,
        "\
function f0 -> ty#1 {
  temps:
    %0: ty#1
  bb0:
    %0 = const int 1: ty#1
    return %0
}
"
    );
}

#[test]
fn materializes_constant_nil_redde_with_type() {
    let dump = dump_source("functio nullum() → nihil { redde nulla }");
    assert_eq!(
        dump,
        "\
function f0 -> ty#4 {
  temps:
    %0: ty#4
  bb0:
    %0 = const nil: ty#4
    return %0
}
"
    );
}

#[test]
fn allows_no_value_return_for_nihil_side_effect_function() {
    let dump = dump_source(
        r#"
functio log() → nihil {
    nota "ok"
}
"#,
    );

    assert!(dump.contains("runtime diagnostic nota"));
    assert!(dump.contains("return\n"));
}

#[test]
// ===========================================================================
// Section F: Failable functions — iace, return_error, mori, handlers
// ===========================================================================

fn failable_function_dump_renders_alternate_exit_type() {
    let dump = dump_source(r#"functio fail() → numerus ⇥ textus { iace "bad" }"#);

    assert!(dump.starts_with("function f0 -> ty#1 ⇥ ty#0 {\n"));
    assert!(dump.contains("  temps:\n    %0: ty#0\n"));
    assert!(dump.contains("    %0 = const string sym#"));
    assert!(dump.contains("    return_error %0\n"));
}

#[test]
fn lowers_iace_to_return_error() {
    let dump = dump_source(
        r#"functio divide(numerus a, numerus b) → numerus ⇥ textus {
  si b ≡ 0 ergo iace "division by zero"
  redde a / b
}"#,
    );

    assert!(dump.contains("function f0 -> ty#1 ⇥ ty#0 {\n"));
    assert!(dump.contains("branch %0 bb1 bb2"));
    assert!(dump.contains("return_error %1"));
    // The quotient is a checked return store: `i64::MIN / -1` does not fit.
    assert!(dump.contains("return %3"), "{dump}");
}

#[test]
fn lowers_failable_call_in_alternate_exit_function_to_return_error_edge() {
    let dump = dump_source(
        r#"
functio fail() → numerus ⇥ textus { iace "bad" }

functio caller() → numerus ⇥ textus {
    sit numerus x ← fail()
    redde x + 1
}
"#,
    );

    assert!(dump.contains("function f1 -> ty#1 ⇥ ty#0 {\n"));
    assert!(dump.contains("try_call def#4096()"));
    assert!(dump.contains("error %0 -> bb"));
    assert!(dump.contains("return_error %0"));
}

#[test]
fn lowers_declared_error_conversion_before_return_error() {
    let dump = dump_source(
        r"
discretio Sourceum { Failed }
discretio Destinatio { Converted }
@ conversio
functio ex_source(Sourceum err) → Destinatio { redde finge Converted {} ∷ Destinatio }
functio fail() → vacuum ⇥ Sourceum { iace finge Failed {} ∷ Sourceum }
functio caller() → vacuum ⇥ Destinatio { fail() }
",
    );
    // The enrolled row's ordinary function is the conversion call; it must
    // run before the propagated ReturnError.
    let call = dump
        .find("call def#")
        .expect("enrolled conversion function call");
    let return_error = dump[call..]
        .find("return_error %1")
        .map(|offset| call + offset)
        .expect("converted value is propagated");
    assert!(call < return_error, "conversion must precede ReturnError:\n{dump}");
}

#[test]
fn lowers_mori_to_panic_runtime_call_and_unreachable() {
    let dump = dump_source(r#"functio impossible() → vacuum { mori "impossible state" }"#);

    assert!(dump.contains("function f0 -> ty#5 {\n"));
    assert!(dump.contains("runtime panic(const string sym#"));
    assert!(dump.contains(") -> ty#6\n"));
    assert!(dump.contains("    unreachable\n"));
}

#[test]
fn lowers_params_and_local_reads_into_places() {
    let dump = dump_source("functio idem(numerus n) → numerus { redde n }");

    assert_eq!(
        dump,
        "\
function f0 -> ty#1 {
  params:
    _0: ty#1
  locals:
    let _0: ty#1
  bb0:
    return _0
}
"
    );
}

#[test]
fn lowers_locals_assignment_and_binary_ops() {
    let dump = dump_source("functio computa() → numerus { varia numerus x ← 1 x ← x + 2 redde x }");

    assert_eq!(
        dump,
        "\
function f0 -> ty#1 {
  locals:
    var _0: ty#1
  temps:
    %0: ty#1
    %1: ty#1
  bb0:
    _0 = const int 1: ty#1
    %0 = _0 + const int 2: ty#1
    %1 = runtime convert runtime ty#1 -> ty#1(%0) -> ty#1
    _0 = %1: ty#1
    return _0
}
"
    );
}

#[test]
fn lowers_postfix_increment_and_decrement() {
    let dump = dump_source("functio computa() → numerus { varia numerus x ← 1 x ↑ x ↓ redde x }");

    // The successor lands through the slot's checked store (cap2, ruling 17).
    assert!(dump.contains("%0 = _0 + const int 1: ty#1"));
    assert!(dump.contains("%1 = runtime convert runtime ty#1 -> ty#1(%0) -> ty#1"));
    assert!(dump.contains("_0 = %1: ty#1"));
    assert!(dump.contains("%2 = _0 - const int 1: ty#1"));
    assert!(dump.contains("%3 = runtime convert runtime ty#1 -> ty#1(%2) -> ty#1"));
    assert!(dump.contains("_0 = %3: ty#1"));
}

#[test]
fn lowers_postfix_increment_evaluates_index_once() {
    let dump = dump_source(
        r"functio f() → numerus { redde 0 }
functio bump(varia lista<numerus> xs) {
  xs[f()] ↑
}",
    );

    let f1_body = dump
        .split("function f1")
        .nth(1)
        .expect("missing f1 lowering");
    let f_calls = f1_body.matches("call def#").count();
    assert_eq!(f_calls, 1, "index expression must be evaluated once: {dump}");
}

#[test]
fn lowers_unary_ops_to_typed_temps() {
    let dump = dump_source("functio logicum(bivalens a) → bivalens { redde non a }");

    assert_eq!(
        dump,
        "\
function f0 -> ty#3 {
  params:
    _0: ty#3
  locals:
    let _0: ty#3
  temps:
    %0: ty#3
  bb0:
    %0 = not _0: ty#3
    return %0
}
"
    );
}

#[test]
fn lowers_canonical_predicate_replacements_to_mir_primitives() {
    let dump = dump_source(
        r"
functio pred(numerus n, fractus f, bivalens flag, textus ∪ nihil maybe) → bivalens {
    sit _ positive ← n ≻ 0
    sit _ negative ← f ≺ 0
    sit _ enabled ← flag ≡ verum
    sit _ disabled ← flag ≡ falsum
    sit _ absent ← maybe est nihil
    redde positive et negative et enabled et disabled et absent
}
",
    );

    assert!(dump.contains("%0 = _0 > const int 0: ty#3"));
    assert!(dump.contains("%1 = _1 < const int 0: ty#3"));
    assert!(dump.contains("%2 = _2 == const bool true: ty#3"));
    assert!(dump.contains("%3 = _2 == const bool false: ty#3"));
    assert!(dump.contains("%4 = is_nil _3: ty#3"));
    assert!(!dump.contains("unary operator without a MIR primitive"));
    assert!(!dump.contains("binary operator without a MIR primitive"));
}

#[test]
fn lowers_est_type_check_to_runner_dispatched_variant_tag_intrinsic() {
    // F-023: `est numerus` must NOT lower to `<inner> == false`.
    // Since S5-U3, `est`/`non est` lower to the runner-dispatched
    // MirIntrinsic::TypeCheck carrying the target type instead of the older
    // IsNil/IsNotNil nullity approximation.
    let dump = dump_source(
        r"
functio check(valor v) → bivalens {
    sit _ positive ← v est numerus
    sit _ negative ← v non est numerus
    redde positive et negative
}
",
    );

    // est numerus must NOT approximate as v == false
    assert!(!dump.contains("== const bool false"), "F-023: est must not lower to == false");
    // est numerus lowers to a positive variant-tag type check
    assert!(
        dump.contains("type_check positive=true"),
        "est numerus should lower to a positive type check"
    );
    // non est numerus lowers to a negated variant-tag type check
    assert!(
        dump.contains("type_check positive=false"),
        "non est numerus should lower to a negated type check"
    );
}

/// `match a and b { case X and Y ... default ... }` lowers to a chain of tag
/// probes over the subject tuple: one equality branch per variant column, a
/// miss falling to the next arm, and the default arm reached after the last.
#[test]
fn lowers_multi_subject_match_to_a_tag_probe_chain() {
    let dump = dump_source(
        r#"+++
locale = "en"
+++

union Forma {
    Circulus,
    Quadratum
}

fn pair(Forma a, Forma b) → string {
    match a and b {
        case Circulus and Quadratum {
            return "circle-square"
        }
        default {
            return "other"
        }
    }
}
"#,
    );
    assert_eq!(
        dump.matches("construct variant").count(),
        2,
        "one tag probe per column:\n{dump}"
    );
    assert_eq!(dump.matches("branch %").count(), 2, "one branch per column:\n{dump}");
    assert!(
        dump.contains("_0 == %0") && dump.contains("_1 == %2"),
        "columns probe a then b:\n{dump}"
    );
    assert_eq!(dump.matches("return %").count(), 2, "arm body and default both return:\n{dump}");
    assert!(!dump.contains("switch"), "no single-subject switch:\n{dump}");
}

// ===========================================================================
// Section G: Direct calls, intrinsics, methods
// ===========================================================================

/// `target ⇇ value`: the value is evaluated once and one `TensorCopyInto`
/// writes the target's existing storage. The statement is `vacuum`.
#[test]
fn lowers_copy_into_to_one_tensor_copy_into_with_the_value_evaluated_once() {
    let dump = dump_source(
        r"functio copy(tf32[3, 3] a, tf32[3, 3] b, varia tf32[3, 3] out) → vacuum {
    out ⇇ a + b
}",
    );

    assert_eq!(
        dump.matches("tensor_copy_into").count(),
        1,
        "exactly one copy-into call:\n{dump}"
    );
    assert_eq!(
        dump.matches("tensor_add").count(),
        1,
        "the value is evaluated exactly once:\n{dump}"
    );
    let add = dump.find("tensor_add").expect("add call");
    let copy = dump.find("tensor_copy_into").expect("copy-into call");
    assert!(add < copy, "the value is computed before the copy:\n{dump}");
}

/// A view source copies through the view: the transpose is evaluated once and
/// is the source operand of the single `TensorCopyInto`.
#[test]
fn lowers_copy_into_of_a_view_source_with_one_call() {
    let dump = dump_source(
        r"functio copy(tf32[2, 3] a, varia tf32[3, 2] out) → vacuum {
    out ⇇ a.transpone()
}",
    );

    assert_eq!(dump.matches("tensor_copy_into").count(), 1, "{dump}");
    assert_eq!(dump.matches("tensor_transpose").count(), 1, "{dump}");
}

#[test]
fn lowers_direct_calls_to_definition_callees() {
    let dump =
        dump_source("functio duplex(numerus n) → numerus { redde n * 2 } functio usa() → numerus { redde duplex(4) }");

    assert_eq!(
        dump,
        "\
function f0 -> ty#1 {
  params:
    _0: ty#1
  locals:
    let _0: ty#1
  temps:
    %0: ty#1
    %1: ty#1
  bb0:
    %0 = _0 * const int 2: ty#1
    %1 = runtime convert runtime ty#1 -> ty#1(%0) -> ty#1
    return %1
}

function f1 -> ty#1 {
  temps:
    %0: ty#1
  bb0:
    %0 = call def#4096(const int 4)
    return %0
}
"
    );
}

#[test]
fn lowers_vacuum_direct_calls_without_destinations() {
    let dump = dump_source("functio ping() → vacuum { redde } functio usa() → vacuum { ping() redde }");

    assert_eq!(
        dump,
        "\
function f0 -> ty#5 {
  bb0:
    return
}

function f1 -> ty#5 {
  bb0:
    call def#4096()
    return
}
"
    );
}

#[test]
fn lowers_empty_entry_block_as_synthetic_function() {
    let dump = dump_source("incipit {}");

    assert_eq!(
        dump,
        "\
function f0 -> ty#5 {
  bb0:
    return
}
"
    );
}

#[test]
fn lowers_non_empty_entry_block_as_synthetic_function_body() {
    let dump = dump_source(r"incipit { sit numerus n ← 1 + 2 }");

    assert_eq!(
        dump,
        "\
function f0 -> ty#5 {
  locals:
    let _0: ty#1
  temps:
    %0: ty#1
  bb0:
    %0 = const int 1 + const int 2: ty#1
    _0 = %0: ty#1
    return
}
"
    );
}

#[test]
fn lowers_incipit_exitus_tail_as_int_returning_entry() {
    let dump = dump_source(r"incipit exitus 3 { tacet }");

    assert!(
        !dump.starts_with("function f0 -> ty#5"),
        "an int exitus tail makes the entry non-vacuum; the dump must not open with the vacuum entry header:\n{dump}"
    );
    assert!(
        dump.starts_with("function f0 -> ty#1 {"),
        "the int-returning entry header pins ty#1 (numerus):\n{dump}"
    );
    assert!(
        dump.contains("const int 3"),
        "the exitus tail is evaluated inside the entry body:\n{dump}"
    );
    assert!(
        dump.contains("return %0"),
        "the entry terminates with Return(Some) carrying the tail temp: {dump}"
    );
}

#[test]
fn lowers_non_integer_exitus_tail_keeps_vacuum_entry() {
    let dump = dump_source(r#"incipit exitus "x" { tacet }"#);

    assert!(
        dump.starts_with("function f0 -> ty#5 {"),
        "a non-int exitus tail keeps the vacuum entry header:\n{dump}"
    );
    assert!(
        !dump.contains("return %"),
        "a non-int tail is never returned; the entry ends with a bare return:\n{dump}"
    );
}

#[test]
fn lowers_tacet_to_noop() {
    let dump = dump_source(r"incipit { tacet }");

    assert_eq!(
        dump,
        "\
function f0 -> ty#5 {
  bb0:
    return
}
"
    );
}

#[test]
// ===========================================================================
// Section L: Builder-level unit tests (L4 — manual HIR construction)
// ===========================================================================

fn failed_local_initializer_does_not_bind_local() {
    use crate::hir::{DefId, HirLocal};
    use crate::lexer::{Span, Symbol};
    use crate::semantic::{Primitive, TypeTable};

    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let span = Span::default();
    let mut builder = FunctionBuilder::new(&types);
    let entry = builder.fresh_block(span);
    builder.switch_to(entry);
    let local = HirLocal {
        def_id: DefId(9),
        name: Symbol(9),
        ty: Some(numerus),
        init: Some(unsupported_range_expr(numerus, span, 0)),
        mutable: false,
        is_await: false,
        runtime_provided: false,
        await_store: None,
        size_decl: false,
        size_static: false,
    };

    assert!(builder.lower_local(&local, span).is_none());
    assert!(!builder.bindings.contains_key(&DefId(9)));
    assert_eq!(builder.errors.len(), 1);
    assert_eq!(
        builder.errors[0].issue,
        "unsupported_mir_lowering_interval_clamp_recovery_before_interval_mir_lowering"
    );

    let deferred = HirLocal {
        def_id: DefId(10),
        name: Symbol(10),
        ty: Some(numerus),
        init: None,
        mutable: false,
        is_await: false,
        runtime_provided: false,
        await_store: None,
        size_decl: false,
        size_static: false,
    };
    assert!(builder.lower_local(&deferred, span).is_some());
    assert!(builder.bindings.contains_key(&DefId(10)));
}

#[test]
fn failed_top_level_const_initializer_does_not_bind_const() {
    use crate::hir::{DefId, HirConst};
    use crate::lexer::{Span, Symbol};
    use crate::semantic::{Primitive, TypeTable};

    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let span = Span::default();
    let mut builder = FunctionBuilder::new(&types);
    let entry = builder.fresh_block(span);
    builder.switch_to(entry);
    let konst = HirConst {
        name: Symbol(7),
        ty: Some(numerus),
        value: unsupported_range_expr(numerus, span, 0),
        is_await: false,
        statica: None,
        mutable: false,
    };

    assert!(
        builder
            .lower_const_binding(DefId(7), &konst, span)
            .is_none()
    );
    assert!(!builder.bindings.contains_key(&DefId(7)));
    assert_eq!(builder.errors.len(), 1);
    assert_eq!(
        builder.errors[0].issue,
        "unsupported_mir_lowering_interval_clamp_recovery_before_interval_mir_lowering"
    );
}

#[test]
fn ignores_top_level_type_metadata_items() {
    let unit = analyze(
        "genus Persona { varia textus nomen } discretio Eventus { Bonum { textus nuntius } } functio salve() {}",
    );
    let program = lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default())
        .map(|lowered| lowered.validated.into_parts().0)
        .expect("type metadata should not block MIR lowering");

    assert_eq!(program.functions.len(), 1);
}

#[test]
// ===========================================================================
// Section D: Control flow — si, elige, dum, fac/cape, rumpe/perge, itera
// ===========================================================================

fn lowers_si_to_branch_and_join_blocks() {
    let dump = dump_source("functio signum(numerus n) → numerus { si n ≻ 0 ergo redde n redde 0 }");

    assert_eq!(
        dump,
        "\
function f0 -> ty#1 {
  params:
    _0: ty#1
  locals:
    let _0: ty#1
  temps:
    %0: ty#3
    %1: ty#1
  bb0:
    %0 = _0 > const int 0: ty#3
    branch %0 bb1 bb2
  bb1:
    return _0
  bb2:
    %1 = const int 0: ty#1
    return %1
}
"
    );
}

#[test]
fn lowers_expression_valued_si_into_shared_destination() {
    let dump = dump_source("functio positum(numerus n) → numerus { sit numerus x ← n ≻ 0 ✓ 1 ✗ 0 redde x }");

    assert_eq!(
        dump,
        "\
function f0 -> ty#1 {
  params:
    _0: ty#1
  locals:
    let _0: ty#1
    let _1: ty#1
  temps:
    %0: ty#3
  bb0:
    %0 = _0 > const int 0: ty#3
    branch %0 bb1 bb2
  bb1:
    _1 = const int 1: ty#1
    goto bb3
  bb2:
    _1 = const int 0: ty#1
    goto bb3
  bb3:
    return _1
}
"
    );
}

#[test]
fn lowers_literal_elige_to_switch_terminator() {
    let dump = dump_source(
        r#"
functio status(numerus code) → textus {
    elige code {
        casu 200 { redde "ok" }
        casu 404 { redde "missing" }
        ceterum { redde "other" }
    }
}
"#,
    );

    assert!(dump.contains("switch _0 [const int 200: bb"));
    assert!(dump.contains("const int 404: bb"));
    assert!(dump.contains("default bb"));
    assert!(!dump.contains("discerne before switch MIR lowering"));
}

#[test]
// ===========================================================================
// Section E: Discerne & enum variant operations
// ===========================================================================

fn lowers_unit_variant_discerne_to_branch_chain() {
    let dump = dump_source(
        r#"discretio Status {
  Active,
  Inactive
}

functio describe(Status status) → vacuum {
  discerne status {
    casu Active { nota "active" }
    casu Inactive { nota "inactive" }
  }
}"#,
    );

    assert!(dump.contains("construct variant def#"));
    assert!(dump.contains("branch %"));
    assert!(dump.contains("runtime diagnostic nota("));
}

#[test]
fn lowers_payload_variant_discerne_to_branch_chain_and_field_bindings() {
    let dump = dump_source(
        r#"discretio Event {
  Click { numerus x numerus y },
  Quit
}

functio handle(Event event) → vacuum {
  discerne event {
    casu Click sit x, y {
      nota x
      nota y
    }
    casu Quit {
      nota "quit"
    }
  }
}"#,
    );

    assert!(dump.contains("construct variant def#"));
    assert!(dump.contains(".def#"));
    assert!(dump.contains("runtime diagnostic nota(_1)"));
    assert!(dump.contains("runtime diagnostic nota(_2)"));
    assert!(!dump.contains("non-literal discerne pattern"));
}

#[test]
fn lowers_dum_to_condition_body_and_after_blocks() {
    let dump = dump_source(
        "functio totalis(numerus n) → numerus { varia numerus i ← 0 varia numerus total ← 0 dum i ≺ n { total ← total + i i ← i + 1 } redde total }",
    );

    assert_eq!(
        dump,
        "\
function f0 -> ty#1 {
  params:
    _0: ty#1
  locals:
    let _0: ty#1
    var _1: ty#1
    var _2: ty#1
  temps:
    %0: ty#3
    %1: ty#1
    %2: ty#1
    %3: ty#1
    %4: ty#1
  bb0:
    _1 = const int 0: ty#1
    _2 = const int 0: ty#1
    goto bb1
  bb1:
    %0 = _1 < _0: ty#3
    branch %0 bb2 bb3
  bb2:
    %1 = _2 + _1: ty#1
    %2 = runtime convert runtime ty#1 -> ty#1(%1) -> ty#1
    _2 = %2: ty#1
    %3 = _1 + const int 1: ty#1
    %4 = runtime convert runtime ty#1 -> ty#1(%3) -> ty#1
    _1 = %4: ty#1
    goto bb1
  bb3:
    return _2
}
"
    );
}

#[test]
fn verum_dum_tail_return_lowers_valid_mir() {
    // `while true { ... return i }` never exits through its condition, so the
    // loop's after-block is dead. The implicit function return must not land
    // there: a `return` in that block is rejected for a value-returning
    // function.
    let dump = dump_source(
        r#"+++
locale = "en"
+++
fn f() → int {
    mut int i ← 0
    while true {
        i ← i + 1
        if i ≡ 3 {
            return i
        }
    }
}
main {
    print f()
}
"#,
    );
    assert!(dump.contains("bb0:"), "valid lowered dump must be emitted:\n{dump}");
}

#[test]
fn verum_dum_loop_in_vacuum_main_stays_valid() {
    // An intentional infinite loop in a void function still lowers: the sealed
    // after-block must not be the only terminator path.
    let dump = dump_source(
        r#"+++
locale = "en"
+++
main {
    while true {
        print "x"
    }
}
"#,
    );
    assert!(dump.contains("bb0:"), "valid lowered dump must be emitted:\n{dump}");
}

#[test]
fn verum_fac_dum_tail_return_lowers_valid_mir() {
    // `fac { ... } dum verum` is the do-while spelling of `while true`: the
    // loop never exits through its synthesized test, so the after-block is
    // dead. The implicit function return must not land there, or a
    // value-returning function fails MIR validation.
    let dump = dump_source(
        r"functio f() → numerus { varia numerus i ← 0 fac { i ← i + 1 si i ≡ 4 ergo redde i * 10 } dum verum } incipit { nota f() }",
    );
    assert!(dump.contains("bb0:"), "valid lowered dump must be emitted:\n{dump}");
}

#[test]
fn lowers_rumpe_inside_fac_to_breakable_exit() {
    let dump = dump_source("functio exemplo() → numerus { fac { rumpe } cape err { redde 0 } redde 1 }");
    // WHY: the entry block must reach the after-block via the rumpe goto edge,
    // not be sealed unreachable. A broken CFG leaves the entry block dead and
    // the after-block unreachable.
    let entry = first_block_body(&dump);
    assert!(entry.contains("goto bb"), "fac rumpe entry must emit a goto exit edge: {dump}");
    assert!(
        !entry.contains("unreachable"),
        "fac entry must not be sealed unreachable: {dump}"
    );
    assert!(dump.contains("const int 1"), "fac rumpe must reach redde 1: {dump}");
}

#[test]
fn rejects_statement_level_stream_ad_for_mir() {
    let result = crate::driver::compile(
        &crate::Session::new(crate::Config::default().with_latin_locale()),
        "stream-ad.fab",
        r"functio f() → vacuum { ad 'noop:ping' { tuus { rumpe } } }",
        ModuleCompileRequest::default(),
    );
    assert!(
        !result.success(),
        "statement-level ad should fail parse: {:?}",
        result.diagnostics
    );
}

#[test]
fn loop_rumpe_target_wins_over_enclosing_fac() {
    let dump = dump_source(
        "functio praecedentia() → numerus { fac { varia numerus x ← 0 dum x ≺ 3 { x ← x + 1 si x ≡ 2 ergo rumpe } redde x } cape err { redde 0 } }",
    );
    // The rumpe inside the dum loop must resolve to the dum loop's exit (the
    // `redde x` after the loop, which is bb6 in this dump), not to the enclosing
    // fac's cape/error path (bb1/bb2/bb3).  We verify by checking that the block
    // containing the rumpe targets the loop-exit block, not any fac-internal block.
    assert!(
        dump.contains("bb7:\n    goto bb6"),
        "rumpe must break to the dum loop exit, not the enclosing fac catch\n{dump}"
    );
}

#[test]
fn lowers_rumpe_and_perge_to_loop_targets() {
    let dump = dump_source(
        "functio primus(numerus n) → numerus { varia numerus i ← 0 dum i ≺ n { i ← i + 1 si i ≺ 3 ergo perge rumpe } redde i }",
    );

    assert_eq!(
        dump,
        "\
function f0 -> ty#1 {
  params:
    _0: ty#1
  locals:
    let _0: ty#1
    var _1: ty#1
  temps:
    %0: ty#3
    %1: ty#1
    %2: ty#1
    %3: ty#3
  bb0:
    _1 = const int 0: ty#1
    goto bb1
  bb1:
    %0 = _1 < _0: ty#3
    branch %0 bb2 bb3
  bb2:
    %1 = _1 + const int 1: ty#1
    %2 = runtime convert runtime ty#1 -> ty#1(%1) -> ty#1
    _1 = %2: ty#1
    %3 = _1 < const int 3: ty#3
    branch %3 bb4 bb5
  bb3:
    return _1
  bb4:
    goto bb1
  bb5:
    goto bb3
}
"
    );
}

#[test]
fn lowers_perge_inside_fac_to_enclosing_loop_head() {
    // Ruling R3: `perge` passes through a plain `fac` block to the enclosing
    // loop. The perge edge must goto the dum head (bb1), never the cape block.
    let dump = dump_source(
        "functio f(numerus n) → numerus { varia numerus i ← 0 dum i ≺ n { i ← i + 1 fac { si i ≡ 2 ergo perge } cape err { redde 0 } } redde i }",
    );
    assert!(dump.contains("bb1:\n    %0 = _1 < _0"), "dum head must be bb1: {dump}");
    let perge_edges = dump.matches("goto bb1").count();
    assert!(
        perge_edges >= 2,
        "perge inside fac must jump to the loop head alongside the back edge: {dump}"
    );
}

#[test]
fn lowers_perge_inside_fac_dum_to_the_condition_test() {
    // Ruling R3 (where `perge` lands): in `fac { } dum c`, `perge` goes to the
    // end of the body, so `c` is tested. The perge edge must goto the block
    // that evaluates the condition, never back to the body head.
    let dump = dump_source(
        "functio f(numerus n) → numerus { varia numerus i ← 0 fac { i ← i + 1 si i ≡ 2 ergo perge } dum i ≺ n redde i }",
    );
    let cond_block = dump
        .split("\n  bb")
        .find(|block| block.contains("_1 < _0"))
        .and_then(|block| block.split(':').next())
        .unwrap_or_else(|| panic!("condition block: {dump}"));
    let cond_edge = format!("goto bb{cond_block}\n");
    assert_eq!(
        dump.matches(&cond_edge).count(),
        2,
        "fallthrough and perge both reach the condition test (bb{cond_block}): {dump}"
    );
}

#[test]
fn closed_si_arms_do_not_emit_spurious_join_edges() {
    let dump = dump_source(
        "functio opta(numerus n) → numerus { varia numerus x ← n si x ≻ 0 { redde x } secus { x ← x + 1 } redde x }",
    );

    assert_eq!(
        dump,
        "\
function f0 -> ty#1 {
  params:
    _0: ty#1
  locals:
    let _0: ty#1
    var _1: ty#1
  temps:
    %0: ty#3
    %1: ty#1
    %2: ty#1
  bb0:
    _1 = _0: ty#1
    %0 = _1 > const int 0: ty#3
    branch %0 bb1 bb2
  bb1:
    return _1
  bb2:
    %1 = _1 + const int 1: ty#1
    %2 = runtime convert runtime ty#1 -> ty#1(%1) -> ty#1
    _1 = %2: ty#1
    goto bb3
  bb3:
    return _1
}
"
    );
}

#[test]
fn edge_si_no_secus_lowers_to_single_arm_branch_and_join() {
    let dump = dump_source("functio noop_if_zero(numerus n) → vacuum { si n ≻ 0 ergo nota \"positive\" }");

    assert!(
        dump.contains("branch %0 bb1 bb2"),
        "si without secus must branch to body and continue: {dump}"
    );
    assert!(
        dump.contains("runtime diagnostic nota(const string sym#"),
        "body must emit the nota: {dump}"
    );
    assert!(dump.contains("goto bb2"), "body must jump to the join block: {dump}");
    assert!(dump.contains("bb2:\n    return"), "join block after si arm must return: {dump}");
    assert!(!dump.contains("bb3"), "si without secus must not produce a third block: {dump}");
}

#[test]
fn lowers_numeric_range_itera_ab_to_loop_cfg() {
    let dump = dump_source(
        r"functio range_sum() → numerus {
  varia numerus total ← 0
  itera ab 0‥4 sit i {
    total ← total + i
  }
  redde total
}",
    );

    assert!(dump.contains("var _1: ty#1"));
    assert!(dump.contains("_1 = const int 0: ty#1"));
    assert!(dump.contains("_2 = const int 4: ty#1"));
    assert!(dump.contains("_3 = const int 1: ty#1"));
    assert!(dump.contains("_1 < _2: ty#3"));
    assert!(dump.contains("_0 + _1: ty#1"));
    assert!(dump.contains("_1 + _3: ty#1"));
    assert!(!dump.contains("itera before iterator MIR lowering"));
}

#[test]
fn range_itera_stops_when_bounds_fail_to_lower() {
    use crate::hir::{DefId, HirBlock, HirExpression, HirExpressionKind, HirId, HirLiteral, HirRangeKind};
    use crate::lexer::Symbol;
    use crate::semantic::{Primitive, TypeTable};

    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let vacuum = types.primitive(Primitive::Vacuum);
    let span = crate::lexer::Span::default();
    let mut builder = FunctionBuilder::new(&types);
    let entry = builder.fresh_block(span);
    builder.switch_to(entry);

    let unsupported_start = unsupported_range_expr(numerus, span, 0);
    let end =
        HirExpression { id: HirId(3), kind: HirExpressionKind::Literal(HirLiteral::Int(4)), ty: Some(numerus), span };
    let iter = HirExpression {
        id: HirId(4),
        kind: HirExpressionKind::Intervallum {
            start: Box::new(unsupported_start),
            end: Box::new(end),
            step: None,
            kind: HirRangeKind::Exclusive,
        },
        ty: Some(numerus),
        span,
    };
    let expr = HirExpression { id: HirId(5), kind: HirExpressionKind::Vacua, ty: Some(vacuum), span };
    let body = HirBlock { statements: Vec::new(), expr: None, span, ..Default::default() };

    assert!(
        builder
            .lower_itera_expr(
                crate::hir::HirIteraMode::Ab,
                &[],
                DefId(0),
                Symbol(0),
                false,
                &iter,
                &body,
                &expr
            )
            .is_none()
    );
    assert_eq!(builder.blocks.len(), 1);
    assert_eq!(builder.errors.len(), 1);
    assert_eq!(
        builder.errors[0].issue,
        "unsupported_mir_lowering_interval_clamp_recovery_before_interval_mir_lowering"
    );
}

#[test]
fn edge_empty_range_itera_ab_skips_loop_body() {
    let dump = dump_source(
        r"functio noop() → numerus {
  varia numerus total ← 0
  itera ab 0‥0 sit i {
    total ← total + i
  }
  redde total
}",
    );

    assert!(dump.contains("_0 = const int 0: ty#1"), "total must be initialized: {dump}");
    assert!(dump.contains("return _0"), "must return the unchanged total: {dump}");
    // The range itera lowering generates a loop CFG even for 0‥0; the
    // condition check ensures the body never executes at runtime.
    assert!(dump.contains("branch %"), "range itera must produce branches: {dump}");
}

#[test]
fn lowers_shaped_tensor_and_matrix_collections() {
    // AC-6 admits one unified shaped arm. The matrix source uses the same
    // rank-2 coordinate spelling as a tensor; its register cells are selected
    // through static MatrixCell projections behind the coordinate loop.
    let tensor_dump = dump_source(
        r"functio iterate(tensor<f32, [2, 3]> m) → vacuum {
  itera ex m apud [i, j] sit cell {
    nota i
    nota j
    nota cell
  }
}",
    );
    assert!(
        !tensor_dump.contains("itera collection before iterator MIR lowering"),
        "{tensor_dump}"
    );
    assert!(
        tensor_dump.contains("runtime collection tensor_get"),
        "tensor element binding must use TensorGet: {tensor_dump}"
    );
    assert!(
        tensor_dump.matches("branch ").count() >= 2,
        "rank-2 tensor needs two loop branches: {tensor_dump}"
    );

    let matrix_dump = dump_source(
        r"functio iterate(matrix<f32, [2, 3]> m) → vacuum {
  itera ex m apud [i, j] sit cell {
    nota i
    nota j
    nota cell
  }
}",
    );
    assert!(
        !matrix_dump.contains("itera collection before iterator MIR lowering"),
        "{matrix_dump}"
    );
    assert!(
        matrix_dump.contains(".cell0_0"),
        "matrix first cell must be bound: {matrix_dump}"
    );
    assert!(
        matrix_dump.contains(".cell1_2"),
        "matrix last cell must be bound: {matrix_dump}"
    );
    assert!(
        matrix_dump.matches("branch ").count() >= 2,
        "rank-2 matrix needs two loop branches: {matrix_dump}"
    );
}

#[test]
fn lowers_array_itera_ex_and_de_to_index_loops() {
    let ex_dump = dump_source(
        r"functio sumArray(lista<numerus> nums) → numerus {
  varia numerus total ← 0
  itera ex nums sit n {
    total ← total + n
  }
  redde total
}",
    );

    assert!(ex_dump.contains("runtime collection length(_0)"));
    assert!(ex_dump.contains("_3 = _0[_2]: ty#1"));
    assert!(ex_dump.contains("_1 + _3: ty#1"));
    assert!(!ex_dump.contains("itera collection before iterator MIR lowering"));

    let de_dump = dump_source(
        r"functio firstIndex(lista<numerus> nums) → numerus {
  itera de nums sit index {
    redde index
  }
  redde 0
}",
    );

    assert!(de_dump.contains("runtime collection length(_0)"));
    assert!(de_dump.contains("_2 = _1: ty#1"));
    assert!(de_dump.contains("return _2"));
    assert!(!de_dump.contains("itera collection before iterator MIR lowering"));
}

#[test]
fn lowers_lockstep_itera_to_one_aligned_index_walk() {
    // Need: N sources lower to ONE aligned index walk — the shared
    // length comes from the first source, each further source is guarded
    // equal before the loop, and every binder reads its own projection.
    let dump = dump_source(
        r"functio summaVecta(lista<numerus> ks, lista<numerus> vs) → numerus {
  varia numerus total ← 0
  itera ex ks, vs sit k, v {
    total ← total + k + v
  }
  redde total
}",
    );

    let length_calls = dump.matches("runtime collection length").count();
    assert_eq!(length_calls, 2, "one length read per source: {dump}");
    assert!(
        dump.contains("_3 = _4[_2]: ty#1") || dump.matches("[_").count() >= 2,
        "each binder reads its own projection: {dump}"
    );
    assert!(
        dump.contains("panic"),
        "the equal-length admission panics fail-closed before the walk: {dump}"
    );
    assert!(!dump.contains("itera collection before iterator MIR lowering"));
    assert!(!dump.contains("lockstep"));
}

#[test]
fn lockstep_itera_unequal_lengths_abort_fail_closed() {
    // The walk never truncates to the shorter source: the runtime admission
    // aborts before the first body visit (no partial output).
    let mut host = crate::mir::BufferHost::default();
    let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        crate::mir::run_source(
            &Session::new(la_config().with_target(Target::HirTypeScript)),
            "lockstep-mismatch.fab",
            r"incipit {
  varia lista<numerus> ks ← [1, 2, 3]
  varia lista<numerus> vs ← [10, 20]
  itera ex ks, vs sit k, v {
    nota k + v
  }
}",
            &mut host,
        )
    }));
    assert!(failed.is_err(), "unequal lockstep lengths must abort");
    assert_eq!(
        host.last_abort.as_deref(),
        Some("itera lockstep sources must have equal length")
    );
    assert!(host.stdout_lines.is_empty(), "no partial rows before the abort");
}

#[test]
fn lockstep_itera_runs_aligned_walks_with_shared_coordinate() {
    let mut host = crate::mir::BufferHost::default();
    crate::mir::run_source(
        &Session::new(la_config().with_target(Target::HirTypeScript)),
        "lockstep-run.fab",
        r#"incipit {
  varia lista<numerus> ks ← [1, 2, 3]
  varia lista<numerus> vs ← [10, 20, 30]
  varia lista<textus> ts ← ["a", "b", "c"]
  itera ex ks, vs sit k, v {
    nota k + v
  }
  itera ex ks, vs, ts apud [i] sit k, v, t {
    nota t
  }
}"#,
        &mut host,
    )
    .unwrap_or_else(|error| panic!("lockstep walks must run: {error:?}"));
    assert_eq!(host.stdout_lines, ["11", "22", "33", "a", "b", "c"]);
}

#[test]
fn range_product_two_axes_walk_row_major_leftmost_outermost() {
    // NR-3: the range product is one ordered Cartesian construct — the first
    // range is the outer axis, so (i, j) visits row-major leftmost-outermost.
    let mut host = crate::mir::BufferHost::default();
    crate::mir::run_source(
        &Session::new(la_config().with_target(Target::HirTypeScript)),
        "range-product-n2.fab",
        r"incipit {
  itera ab 0‥2, 0‥3 sit i, j {
    nota i * 10 + j
  }
}",
        &mut host,
    )
    .unwrap_or_else(|error| panic!("range product must run: {error:?}"));
    assert_eq!(host.stdout_lines, ["0", "1", "2", "10", "11", "12"]);
}

#[test]
fn range_product_three_axes_preserve_every_binder() {
    // NR-3: all N binders stay live in the single shared body — the encoded
    // row proves i, j, and p each contribute their axis digit.
    let mut host = crate::mir::BufferHost::default();
    crate::mir::run_source(
        &Session::new(la_config().with_target(Target::HirTypeScript)),
        "range-product-n3.fab",
        r"incipit {
  itera ab 0‥2, 0‥2, 0‥2 sit i, j, p {
    nota i * 100 + j * 10 + p
  }
}",
        &mut host,
    )
    .unwrap_or_else(|error| panic!("three-axis range product must run: {error:?}"));
    assert_eq!(host.stdout_lines, ["0", "1", "10", "11", "100", "101", "110", "111"]);
}

#[test]
fn range_product_keeps_per_axis_step_and_inclusive_bounds() {
    // NR-3: each axis retains the single-range semantics — axis 0 is an
    // exclusive stepped walk (0, 2), axis 1 is inclusive (0, 1, 2).
    let mut host = crate::mir::BufferHost::default();
    crate::mir::run_source(
        &Session::new(la_config().with_target(Target::HirTypeScript)),
        "range-product-semantics.fab",
        r"incipit {
  itera ab 0‥4 per 2, 0…2 sit i, j {
    nota i * 10 + j
  }
}",
        &mut host,
    )
    .unwrap_or_else(|error| panic!("mixed-bounds range product must run: {error:?}"));
    assert_eq!(host.stdout_lines, ["0", "1", "2", "20", "21", "22"]);
}

#[test]
fn range_product_lowers_as_ordered_nest_without_zip_guard() {
    // NR-3 keep-class: the product lowers through per-axis range CFG only —
    // no lockstep walk, no list length read, no equal-length admission.
    let dump = dump_source(
        r"functio grid(numerus m, numerus n) → vacuum {
  itera ab 0‥m, 0‥n sit i, j {
    nota i
    nota j
  }
}",
    );
    assert!(
        !dump.contains("range-product itera"),
        "the product must lower instead of declining: {dump}"
    );
    assert!(!dump.contains("lockstep"), "a range product is never a zip: {dump}");
    assert!(
        !dump.contains("runtime collection length"),
        "no list length read feeds a range product: {dump}"
    );
    assert!(!dump.contains("panic"), "no equal-length guard guards a range product: {dump}");
    assert!(
        dump.matches("branch ").count() >= 6,
        "two axes need their own sign-split condition CFG: {dump}"
    );
}

#[test]
fn range_product_rumpe_exits_innermost_axis_and_perge_continues_it() {
    // NR-4R decision test — design ruling (2026-08-26): for the
    // N-ary range PRODUCT, `rumpe`/`perge` bind the INNERMOST axis — the
    // hand-nested-loop reading matching the leftmost-outermost row-major
    // contract; outer axes continue after a break. `for from` (zip) keeps
    // whole-construct exit. NR-3's MIR routes `rumpe_target` at the innermost
    // level (control.rs pushes the breakable context only there); this pins
    // that routing as the contract.
    //
    // Discrimination: if `rumpe` bound the whole product, the first break
    // would end the walk and rows "10"/"11" could never print — the second
    // row's presence proves the outer axis advanced through its increment
    // after the break. If `perge` bound an outer axis, the inner axis would
    // restart from its start instead of skipping one inner value, so "0"
    // would print again where "2" appears.
    let mut host = crate::mir::BufferHost::default();
    crate::mir::run_source(
        &Session::new(la_config().with_target(Target::HirTypeScript)),
        "range-product-rumpe.fab",
        r"incipit {
  itera ab 0‥2, 0‥3 sit i, j {
    si j ≡ 2 {
      rumpe
    }
    nota i * 10 + j
  }
}",
        &mut host,
    )
    .unwrap_or_else(|error| panic!("rumpe range product must run: {error:?}"));
    assert_eq!(
        host.stdout_lines,
        ["0", "1", "10", "11"],
        "rumpe exits only the innermost axis; the outer axis continues: {:?}",
        host.stdout_lines
    );

    let mut host = crate::mir::BufferHost::default();
    crate::mir::run_source(
        &Session::new(la_config().with_target(Target::HirTypeScript)),
        "range-product-perge.fab",
        r"incipit {
  itera ab 0‥2, 0‥3 sit i, j {
    si j ≡ 1 {
      perge
    }
    nota i * 10 + j
  }
}",
        &mut host,
    )
    .unwrap_or_else(|error| panic!("perge range product must run: {error:?}"));
    assert_eq!(
        host.stdout_lines,
        ["0", "2", "10", "12"],
        "perge continues the innermost axis only, skipping one inner value per row: {:?}",
        host.stdout_lines
    );
}

#[test]
fn borrowed_receiver_itera_ex_lowers_by_pointee_with_by_value_identical_dump() {
    // MRL-2 flip of MRL-1's decline pins: a borrowed receiver (`de` param,
    // or a field read through one) classifies by pointee and lowers through
    // the same lanes as its by-value twin — byte-identical dump.
    let twin_dump = |mode: &str, source: &str| dump_source(&source.replace("MODUS", mode));

    let list_borrowed = twin_dump(
        "de",
        r"functio summa(MODUS lista<numerus> xs) → numerus {
  varia numerus total ← 0
  itera ex xs sit n {
    total ← total + n
  }
  redde total
}",
    );
    let list_by_value = twin_dump(
        "",
        r"functio summa(MODUS lista<numerus> xs) → numerus {
  varia numerus total ← 0
  itera ex xs sit n {
    total ← total + n
  }
  redde total
}",
    );
    assert!(
        !list_borrowed.contains("itera collection before iterator MIR lowering"),
        "itera ex over a de lista param must lower: {list_borrowed}"
    );
    assert_eq!(list_borrowed, list_by_value, "de lista receiver must match by-value twin");

    let field_borrowed = twin_dump(
        "de",
        r"genus Crater {
  varia lista<numerus> mensurae
}

functio summaCrateris(MODUS Crater c) → numerus {
  varia numerus total ← 0
  itera ex c.mensurae sit n {
    total ← total + n
  }
  redde total
}",
    );
    let field_by_value = twin_dump(
        "",
        r"genus Crater {
  varia lista<numerus> mensurae
}

functio summaCrateris(MODUS Crater c) → numerus {
  varia numerus total ← 0
  itera ex c.mensurae sit n {
    total ← total + n
  }
  redde total
}",
    );
    assert!(
        !field_borrowed.contains("itera collection before iterator MIR lowering"),
        "itera ex over a de struct param field must lower: {field_borrowed}"
    );
    assert_eq!(
        field_borrowed, field_by_value,
        "de struct field receiver must match by-value twin"
    );

    let map_borrowed = twin_dump(
        "de",
        r"functio summaPondereum(MODUS tabula<textus, numerus> pondera) → numerus {
  varia numerus total ← 0
  itera ex pondera sit v {
    total ← total + v
  }
  redde total
}",
    );
    let map_by_value = twin_dump(
        "",
        r"functio summaPondereum(MODUS tabula<textus, numerus> pondera) → numerus {
  varia numerus total ← 0
  itera ex pondera sit v {
    total ← total + v
  }
  redde total
}",
    );
    assert!(
        !map_borrowed.contains("itera collection before iterator MIR lowering"),
        "itera ex over a de tabula param must lower: {map_borrowed}"
    );
    // The `de` marker interns one extra `Ref` type, shifting later opaque
    // type-table ids by one (ty#31 vs ty#30); the lowering itself is
    // identical, so the map twins compare with type ids erased.
    assert_eq!(
        erase_type_ids(&map_borrowed),
        erase_type_ids(&map_by_value),
        "de tabula receiver must match by-value twin"
    );

    // OQ4: `itera de` key iteration shares the classifier — its borrowed arm
    // lowers through the same MapKeys lane as the by-value twin.
    let keys_borrowed = twin_dump(
        "de",
        r"functio numerateClaves(MODUS tabula<textus, numerus> pondera) → numerus {
  varia numerus total ← 0
  itera de pondera sit k {
    total ← total + pondera[k]
  }
  redde total
}",
    );
    let keys_by_value = twin_dump(
        "",
        r"functio numerateClaves(MODUS tabula<textus, numerus> pondera) → numerus {
  varia numerus total ← 0
  itera de pondera sit k {
    total ← total + pondera[k]
  }
  redde total
}",
    );
    assert!(
        !keys_borrowed.contains("itera collection before iterator MIR lowering"),
        "itera de over a de tabula param must lower: {keys_borrowed}"
    );
    assert_eq!(
        erase_type_ids(&keys_borrowed),
        erase_type_ids(&keys_by_value),
        "de tabula key iteration must match by-value twin"
    );
}

/// Need: iterating a `copia` lowers through a `copia ↦ lista<T>`
/// snapshot instead of failing closed.
#[test]
fn itera_ex_over_copia_lowers_through_a_list_snapshot() {
    let dump = dump_source(
        r"functio summa(copia<numerus> elementa) → numerus {
  varia numerus total ← 0
  itera ex elementa sit x {
    total ← total + x
  }
  redde total
}",
    );
    assert!(
        !dump.contains("itera collection before iterator MIR lowering"),
        "itera ex over a copia must lower: {dump}"
    );
    assert!(dump.contains("convert"), "copia walk reads a list snapshot: {dump}");
}

/// Erase opaque `ty#N` ids from a dump so twin comparisons ignore the
/// type-table interning shift a `de` marker's `Ref` wrapper introduces.
fn erase_type_ids(dump: &str) -> String {
    let mut parts = dump.split("ty#");
    let mut erased = String::from(parts.next().unwrap_or_default());
    for part in parts {
        erased.push_str("ty#");
        erased.push_str(part.trim_start_matches(|c: char| c.is_ascii_digit()));
    }
    erased
}

#[test]
fn borrowed_receiver_itera_runs_runtime_correct_results() {
    let source = r#"genus Crater {
  varia lista<numerus> mensurae
}

functio summa(de lista<numerus> xs) → numerus {
  varia numerus total ← 0
  itera ex xs sit n {
    total ← total + n
  }
  redde total
}

functio summaCrateris(de Crater c) → numerus {
  varia numerus total ← 0
  itera ex c.mensurae sit n {
    total ← total + n
  }
  redde total
}

functio summaPondereum(de tabula<textus, numerus> pondera) → numerus {
  varia numerus total ← 0
  itera ex pondera sit v {
    total ← total + v
  }
  redde total
}

functio numerateClaves(de tabula<textus, numerus> pondera) → numerus {
  varia numerus total ← 0
  itera de pondera sit k {
    total ← total + pondera[k]
  }
  redde total
}

incipit {
  varia lista<numerus> numeri ← [1, 2, 3]
  nota summa(numeri)
  sit Crater crater ← Crater { mensurae = [4, 5] }
  nota summaCrateris(crater)
  varia tabula<textus, numerus> pondera ← vacua
  pondera.pone("a", 10)
  pondera.pone("b", 32)
  nota summaPondereum(pondera)
  nota numerateClaves(pondera)
}
"#;
    let mut host = crate::mir::BufferHost::default();
    crate::mir::run_source(
        &Session::new(la_config().with_target(Target::HirTypeScript)),
        "borrowed-itera.fab",
        source,
        &mut host,
    )
    .unwrap_or_else(|error| panic!("borrowed-receiver itera must run: {error:?}"));
    assert_eq!(host.stdout_lines, ["6", "9", "42", "42"]);
}

#[test]
fn borrowed_tensor_receiver_itera_ex_lowers_like_its_by_value_twin() {
    // A borrowed receiver is classified by its pointee through the one
    // classifier, so a borrowed tensor walks the same lane as a by-value one
    // (the earlier decline pinned tensor admission as pending; it is landed).
    let twin = |mode: &str| {
        dump_source(
            &r"functio summaTensoris(MODUS tensor<numerus, [8]> t) → numerus {
  varia numerus total ← 0
  itera ex t sit x {
    total ← total + x
  }
  redde total
}"
            .replace("MODUS", mode),
        )
    };
    let borrowed = twin("de");
    assert!(
        !borrowed.contains("itera collection before iterator MIR lowering"),
        "itera ex over a de tensor param must lower: {borrowed}"
    );
    assert_eq!(
        erase_type_ids(&borrowed),
        erase_type_ids(&twin("")),
        "de tensor receiver must match by-value twin"
    );
}

#[test]
fn textus_itera_ex_lowers_to_index_loop_with_runtime_length() {
    let dump = dump_source(
        r#"functio numerumSeptem(textus s) → numerus {
  varia numerus count ← 0
  itera ex s sit c {
    sit textus t ← c ↦ textus
    si t ≡ "7" {
      count ← count + 1
    }
  }
  redde count
}"#,
    );
    assert!(
        !dump.contains("itera collection before iterator MIR lowering"),
        "itera ex over textus must lower: {dump}"
    );
    assert!(
        dump.contains("runtime collection length"),
        "the textus loop bound is the runtime character length: {dump}"
    );
    assert!(dump.contains("branch "), "the textus loop must produce loop CFG: {dump}");
}

#[test]
fn textus_itera_ex_runs_char_inspection_through_runner() {
    // Run-row law for need: `itera ex` over textus executes through
    // the MIR runner, yielding one character per iteration (the gradus
    // `_numeric` shape — convert-assign the lane binding and inspect it).
    let source = r#"functio numerumSeptem(textus s) → numerus {
  varia numerus count ← 0
  itera ex s sit c {
    sit textus t ← c ↦ textus
    si t ≡ "7" {
      count ← count + 1
    }
  }
  redde count
}

incipit {
  nota numerumSeptem("707")
  nota numerumSeptem("000")
  nota numerumSeptem("")
}"#;
    let mut host = crate::mir::BufferHost::default();
    crate::mir::run_source(
        &Session::new(la_config().with_target(Target::HirTypeScript)),
        "textus-itera.fab",
        source,
        &mut host,
    )
    .unwrap_or_else(|error| panic!("textus itera ex must run: {error:?}"));
    assert_eq!(host.stdout_lines, ["2", "0", "0"]);
}

#[test]
fn textus_itera_de_and_scalar_receivers_keep_fail_closed_decline() {
    // Only `ex` admits text value iteration (`de` would invent an index law);
    // non-iterable scalars keep the generic decline fail-closed.
    let de_case = r"functio indices(textus s) → numerus {
  itera de s sit i {
    nota i
  }
  redde 0
}";
    let unit = analyze(de_case);
    let errors = lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default())
        .expect_err("itera de over textus stays outside the ex-only admission")
        .into_iter()
        .map(|error| error.message)
        .collect::<Vec<_>>();
    assert!(
        errors
            .iter()
            .any(|message| message.contains("itera collection before iterator MIR lowering")),
        "itera de over textus must keep the decline: {errors:?}"
    );

    let scalar_case = r"functio overScalar(numerus n) → vacuum {
  itera ex n sit x {
    nota x
  }
}";
    // D19.4: a non-iterable source is now rejected at typecheck, before MIR.
    let session = Session::new(la_config().with_target(Target::HirTypeScript));
    let Err(errors) = crate::driver::analyze_source(&session, "test.fab", scalar_case, AnalyzeRequest::default())
    else {
        panic!("scalar itera ex must fail closed at analysis")
    };
    assert!(
        errors
            .iter()
            .any(|error| error.issue() == Some("itera_source_not_iterable")),
        "non-iterable scalar must be rejected: {:?}",
        errors
            .iter()
            .map(|e| (e.code, e.issue()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn tensor_itera_ex_rank1_lowers_to_literal_bound_index_loop() {
    let source = r"functio sumTensor(tensor<numerus, [8]> t) → numerus {
  varia numerus total ← 0
  itera ex t sit x {
    total ← total + x
  }
  redde total
}";
    let unit = analyze(source);
    let lowered =
        lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default()).expect("MIR lowering succeeds");
    validate_program(lowered.validated.program(), lowered.validated.validation())
        .expect("the tensor itera loop MIR must validate");
    let dump = dump_program(lowered.validated.program());
    assert!(
        !dump.contains("itera collection before iterator MIR lowering"),
        "rank-1 tensor itera ex must lower: {dump}"
    );
    assert!(
        !dump.contains("runtime collection length"),
        "the tensor loop bound is the literal 8, not a runtime length intrinsic: {dump}"
    );
    assert!(dump.contains("branch "), "the tensor loop must produce loop CFG: {dump}");
    assert!(dump.contains('8'), "the literal lane count 8 must appear as the bound: {dump}");
}

#[test]
fn tensor_itera_ex_rank2_lowers_to_nested_coordinate_loops() {
    let dump = dump_source(
        r"functio sumGrid(tensor<f32, [2, 2]> t) → f32 {
  itera ex t apud [i, j] sit x {
    nota i
    nota j
    nota x
  }
  redde 0.0
}",
    );
    assert!(!dump.contains("itera collection before iterator MIR lowering"), "{dump}");
    assert!(
        dump.contains("runtime collection tensor_get"),
        "rank-2 tensor element binding must use TensorGet: {dump}"
    );
    assert!(
        dump.matches("branch ").count() >= 2,
        "rank-2 tensor needs outer and inner loops: {dump}"
    );
}

#[test]
fn tensor_itera_ex_rank3_lowers_three_nested_coordinate_loops() {
    let dump = dump_source(
        r"functio visitCube(tensor<numerus, [2, 2, 2]> cube) → vacuum {
  itera ex cube apud [i, j, k] sit cell {
    nota i
    nota j
    nota k
    nota cell
  }
}",
    );
    assert!(!dump.contains("itera collection before iterator MIR lowering"), "{dump}");
    assert!(
        dump.contains("runtime collection tensor_get"),
        "rank-3 tensor element binding must use TensorGet: {dump}"
    );
    assert!(
        dump.matches("branch ").count() >= 3,
        "rank-3 tensor needs three loop branches: {dump}"
    );
    assert!(
        dump.matches("var _").count() >= 4,
        "rank-3 loop needs three coordinate locals plus the element local: {dump}"
    );
}

#[test]
fn rejects_deferred_control_flow_constructs_with_explicit_diagnostics() {
    let iter_de_unit = analyze(
        r"functio iterare(tabula<textus, numerus> nums) → vacuum {
            itera de nums sit key { nota key }
        }",
    );
    let iter_de_dump = dump_analyzed_unit(iter_de_unit.application_view()).expect("map itera de lowers");
    assert!(iter_de_dump.contains("runtime collection keys("));
    assert!(iter_de_dump.contains("runtime collection length("));

    let iter_ex_unit = analyze(
        r"functio sumValues(tabula<textus, numerus> nums) → numerus {
            varia numerus total ← 0
            itera ex nums sit value {
                total ← total + value
            }
            redde total
        }",
    );
    let iter_ex_dump = dump_analyzed_unit(iter_ex_unit.application_view()).expect("map itera ex lowers");
    assert!(iter_ex_dump.contains("runtime collection values("));
    assert!(iter_ex_dump.contains("runtime collection length("));
    assert!(!iter_ex_dump.contains("itera ex over tabula before map value iteration MIR lowering"));

    // A bare arm name that is not a member of the matched set never binds: the
    // resolver refuses it at analysis, so it no longer reaches MIR lowering.
    let session = Session::new(la_config().with_target(Target::HirTypeScript));
    let Err(discerne_errors) = crate::driver::analyze_source(
        &session,
        "test.fab",
        "functio handle(numerus n) → vacuum { discerne n { casu x { nota x } } }",
        AnalyzeRequest::default(),
    ) else {
        panic!("binding discerne arm must fail closed at analysis")
    };
    assert!(
        discerne_errors
            .iter()
            .any(|error| error.issue() == Some("discerne_arm_not_member")),
        "a non-member discerne arm must be rejected: {:?}",
        discerne_errors
            .iter()
            .map(|e| (e.code, e.issue()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn rejects_fabricated_iace_without_alternate_exit_type() {
    use crate::hir::{HirExpression, HirExpressionKind, HirId, HirLiteral, HirStatement, HirStatementKind};

    let mut unit = analyze("functio fail() → numerus { redde 0 }");
    let textus = unit.types.primitive(crate::semantic::Primitive::Textus);
    let span = crate::lexer::Span::default();
    let crate::hir::HirItemKind::Function(function) = &mut unit.hir.items[0].kind else {
        panic!("expected function");
    };
    function.err_ty = None;
    function.body = Some(crate::hir::HirBlock {
        statements: vec![HirStatement {
            id: HirId(10),
            kind: HirStatementKind::Expr(HirExpression {
                id: HirId(11),
                kind: HirExpressionKind::Throw(Box::new(HirExpression {
                    id: HirId(12),
                    kind: HirExpressionKind::Literal(HirLiteral::String(crate::lexer::Symbol(99))),
                    ty: Some(textus),
                    span,
                })),
                ty: Some(unit.types.primitive(crate::semantic::Primitive::Vacuum)),
                span,
            }),
            span,
        }],
        expr: None,
        span,
        ..Default::default()
    });

    let errors = lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default())
        .expect_err("fabricated iace without ⇥ should fail");
    assert_eq!(errors.len(), 1);
    assert_eq!(
        errors[0].issue,
        "unsupported_mir_lowering_iace_without_a_declared_alternate_exit_type"
    );
}

#[test]
fn lowers_fac_cape_iace_to_local_handler_edge() {
    let dump = dump_source(r#"functio handled() → numerus { fac { iace "bad" } cape err { redde 0 } redde 1 }"#);

    assert!(dump.starts_with("function f0 -> ty#1 {\n"));
    assert!(dump.contains("locals:\n    let _0: ty#0\n"));
    assert!(dump.contains("%0 = const string sym#"));
    assert!(dump.contains("_0 = %0: ty#0\n    goto bb1"));
    assert!(dump.contains("bb1:\n    %1 = const int 0: ty#1\n    return %1"));
    assert!(!dump.contains("return_error"));
}

#[test]
fn mori_ending_a_cape_body_keeps_mir_valid() {
    // A body that ends in `mori` diverges before any error edge can enter
    // the handler. The catch is dead, but the handler must not leave the
    // join as an orphaned successor target of an unreachable block.
    let dump = dump_source(
        r#"+++
locale = "en"
+++
main {
    do {
        print "in"
        panic "x"
    } catch e {
        print "caught"
    }
}
"#,
    );

    assert!(dump.contains("bb0:"), "{dump}");
}

#[test]
fn lowers_trap_failable_call_to_try_call_carrier_wrap_never_return_error() {
    // UT: `capta` lowers the failable call through the existing try_call
    // whose error block wraps the payload into the tagged `thrown` variant
    // and joins. The trapped error must never reach return_error inside the
    // trapping function (the callee's own return_error is its channel, not an escape).
    let dump = dump_source(
        r#"
functio fail() → numerus ⇥ textus { iace "bad" }
functio safe() → vacuum {
    sit _ r ← capta { fail() }
    discerne r {
        casu returned sit value { nota value }
        casu thrown sit error { nota error }
    }
}
"#,
    );

    assert!(
        dump.contains("function f0 -> ty#1 ⇥ ty#0"),
        "the failable callee keeps its channel: {dump}"
    );
    let (_, trapping_fn) = dump
        .split_once("function f1 -> ")
        .expect("the trapping function is emitted");
    let trapping_fn = trapping_fn
        .split_once("function f2 -> ")
        .map_or(trapping_fn, |(f1, _)| f1);
    assert!(
        trapping_fn.contains("try_call def#"),
        "the trapped call must ride the existing try_call: {trapping_fn}"
    );
    assert!(
        !trapping_fn.contains("return_error"),
        "a trapped error never reaches return_error: {trapping_fn}"
    );
}

#[test]
fn lowers_trap_iace_to_tagged_carrier_wrap() {
    // An explicit `iace` inside the trap body wraps into the `thrown` variant
    // instead of propagating; its no-normal-return path still lowers cleanly.
    let dump = dump_source(r#"functio safe() → vacuum { sit _ r ← capta { iace "bad" } nota r }"#);

    assert!(dump.contains("goto bb"), "trap lowering produced control flow: {dump}");
    assert!(
        dump.contains("construct variant def#"),
        "the captured iace must construct a tagged variant: {dump}"
    );
    assert!(
        !dump.contains("return_error"),
        "a trapped iace never reaches return_error: {dump}"
    );
}

#[test]
fn lowers_trap_thrown_variant_payload_projection() {
    // `thrown` binds the error slot of `trapped<R, T>` directly. A payload
    // projection inside that arm must use the applied error type.
    let dump = dump_source(
        r#"
        genus Errata { varia textus message }
        functio fail() → numerus ⇥ Errata { iace Errata { message = "bad" } }
        functio inspect() → vacuum {
            sit _ r ← capta { fail() }
            discerne r {
                casu thrown sit error { nota error.message }
                casu returned sit value { nota "success" }
            }
        }
        "#,
    );
    assert!(
        dump.contains("_1 = _0.def#") && dump.contains("runtime diagnostic nota(_1."),
        "the thrown payload field projection should lower: {dump}"
    );
}

#[test]
fn runs_trap_returned_variant_collection_payload_projection() {
    let source = r"
        functio make() → lista<numerus> { redde [1, 2] }
        incipit {
            sit _ r ← capta { make() }
            discerne r {
                casu returned sit values { nota values.longitudo() }
                casu thrown sit error { nota -1 }
            }
        }
        ";
    let mut host = crate::mir::BufferHost::default();
    crate::mir::run_source(
        &Session::new(la_config().with_target(Target::HirTypeScript)),
        "trap-returned-list.fab",
        source,
        &mut host,
    )
    .expect("tagged trap payload projection should run");
    assert_eq!(host.stdout_lines, ["2"]);
}

#[test]
fn lowers_trap_inside_fac_routes_to_innermost_boundary() {
    // `fac { capta { fail() } } cape err { … }`: the trap is the innermost
    // boundary, so the call wraps into the trap carrier — the outer cape
    // handler is not entered by the trapped call.
    let dump = dump_source(
        r#"
functio fail() → numerus ⇥ textus { iace "bad" }
functio safe() → vacuum {
    fac {
        sit _ r ← capta { fail() }
        discerne r {
            casu returned sit value { nota value }
            casu thrown sit error { nota error }
        }
    } cape err {
        nota err
    }
}
"#,
    );

    let (_, trapping_fn) = dump
        .split_once("function f1 -> ")
        .expect("the trapping function is emitted");
    let trapping_fn = trapping_fn
        .split_once("function f2 -> ")
        .map_or(trapping_fn, |(f1, _)| f1);
    assert!(
        !trapping_fn.contains("return_error"),
        "no alternate exit escapes the nested boundaries: {trapping_fn}"
    );
    assert!(
        trapping_fn.contains("try_call def#"),
        "the trapped call rides try_call: {trapping_fn}"
    );
}

#[test]
fn lowers_fac_cape_failable_call_to_try_call_edge() {
    let dump = dump_source(
        r#"
functio fail() → numerus ⇥ textus { iace "bad" }
functio safe() → numerus { fac { redde fail() } cape err { redde 0 } }
"#,
    );

    assert!(dump.contains("function f0 -> ty#1 ⇥ ty#0"));
    assert!(dump.contains("function f1 -> ty#1"));
    assert!(
        dump.contains("try_call def#4096()"),
        "fac body should still lower the failable call through try_call: {dump}"
    );
    assert!(
        dump.contains("error _0 -> bb"),
        "cape handler should receive the propagated error binding: {dump}"
    );
    assert!(
        dump.contains("const int 0: ty#1"),
        "cape handler should still return the recovery literal: {dump}"
    );
}

#[test]
fn lowers_dum_cape_iace_to_handler_and_loop_exit() {
    let dump = dump_source(
        r#"functio loop(bivalens ready) → numerus { varia numerus x ← 0 dum ready { iace "bad" } cape err { x ← 1 } redde x }"#,
    );

    assert!(dump.contains("branch _0 bb4 bb5"));
    assert!(dump.contains("_2 = %0: ty#0\n    goto bb1"));
    assert!(dump.contains("bb1:\n    _1 = const int 1: ty#1\n    goto bb2"));
    assert!(dump.contains("bb2:\n    return _1"));
}

#[test]
fn lowers_si_arm_cape_iace_to_arm_handler() {
    let dump = dump_source(
        r#"functio choose(bivalens ready) → numerus { si ready { iace "bad" } cape err { redde 0 } secus { redde 2 } redde 1 }"#,
    );

    assert!(dump.contains("branch _0 bb1 bb2"));
    assert!(dump.contains("_1 = %0: ty#0\n    goto bb4"));
    assert!(dump.contains("bb2:\n    %2 = const int 2: ty#1\n    return %2"));
    assert!(dump.contains("bb4:\n    %1 = const int 0: ty#1\n    return %1"));
    assert!(!dump.contains("return_error"));
}

#[test]
fn lowers_si_chain_inside_cape_handler_to_reachable_blocks() {
    let dump = dump_source(
        r#"
discretio FormaError {
    FormaInvalida { textus causa }
}
discretio NnError {
    DimensioNegativa { textus causa },
    DimensioSupraLimitem { textus causa },
    ProductumSupraLimitem { textus causa },
    GradusMismatch { textus causa },
    Incompatibilis { textus causa },
    FormaMismatch { textus causa },
    TypoMismatch { textus causa },
    ElementaMismatch { textus causa }
}
functio causa(FormaError e) → textus {
    discerne e {
        casu FormaInvalida sit causa { redde causa }
    }
}
functio mensura(lista<numerus> x, lista<numerus> w) → numerus ⇥ FormaError {
    si x.longitudo() ≢ 2 {
        iace finge FormaInvalida { causa = "matmul requires rank-2 operands" }
    }
    redde w.longitudo()
}
functio addendum(numerus a, numerus b) → numerus ⇥ FormaError {
    si a ≺ 0 {
        iace finge FormaInvalida { causa = "shapes not broadcastable" }
    }
    redde a + b
}
functio typo_par_tres(lista<numerus> a, lista<numerus> b, lista<numerus> c) → bivalens ⇥ NnError {
    sit textus ta ← "f32"
    sit textus tb ← "f32"
    sit textus tc ← "f32"
    si ta ≢ tb aut tb ≢ tc {
        iace finge TypoMismatch { causa = "dtype mismatch" }
    }
    si ta ≢ "f32" {
        iace finge TypoMismatch { causa = "unsupported dtype for nn primitive" }
    }
    redde verum
}
functio linear(lista<numerus> x, lista<numerus> w, lista<numerus> b) → numerus ⇥ NnError {
    sit lista<numerus> fx ← x
    sit lista<numerus> fw ← w
    si fx.longitudo() ≢ 2 {
        iace finge GradusMismatch { causa = "linear requires rank-2 input" }
    }
    si fw.longitudo() ≢ 2 {
        iace finge GradusMismatch { causa = "linear requires rank-2 weight" }
    }
    sit numerus m ← fx.accipe(0) vel 1
    sit numerus k ← fx.accipe(1) vel 1
    sit numerus n ← fw.accipe(1) vel 1
    si k ≢ (fw.accipe(0) vel 1) {
        iace finge Incompatibilis { causa = "inner dimensions mismatch" }
    }
    sit bivalens _ ← typo_par_tres(x, w, b)
    sit lista<numerus> fb ← b
    si fb.longitudo() ≡ 1 {
        si (fb.accipe(0) vel 1) ≢ n {
            iace finge FormaMismatch { causa = "bias does not match output width" }
        }
    }
    secus {
        si fb.longitudo() ≡ 2 {
            si (fb.accipe(0) vel 1) ≢ m aut (fb.accipe(1) vel 1) ≢ n {
                iace finge FormaMismatch { causa = "bias does not match output shape" }
            }
        }
        secus {
            iace finge FormaMismatch { causa = "bias must be per-channel or output-shaped" }
        }
    }
    fac {
        sit numerus prediction ← mensura(x, w)
        redde addendum(prediction, n)
    }
    cape err {
        sit textus c ← causa(err)
        si c ≡ "matmul requires rank-2 operands" {
            iace finge GradusMismatch { causa = c }
        }
        si c ≡ "inner dimensions mismatch" {
            iace finge Incompatibilis { causa = c }
        }
        si c ≡ "dtype mismatch" {
            iace finge TypoMismatch { causa = c }
        }
        si c ≡ "element count above ceiling" {
            iace finge ProductumSupraLimitem { causa = c }
        }
        si c ≡ "dimension above ceiling" {
            iace finge DimensioSupraLimitem { causa = c }
        }
        si c ≡ "negative dimension" {
            iace finge DimensioNegativa { causa = c }
        }
        si c ≡ "shapes not broadcastable" {
            iace finge FormaMismatch { causa = c }
        }
        iace finge FormaMismatch { causa = c }
    }
}
"#,
    );
    assert!(dump.contains("bb0:"), "{dump}");
    assert!(!dump.contains("not reachable from entry"), "{dump}");
}

#[test]
fn synthetic_path_callee_inside_cape_loses_failable_try_call() {
    // Bug shape: the callee's function type is dropped (`ty: None`), so the
    // direct-def path cannot recover failable-ness and emits a plain `Call`;
    // the cape handler is never referenced and its si-chain orphans.
    let errors = synthetic_path_cape_lowering(false).expect_err("expected the orphaned-handler reachability error");
    assert!(
        errors
            .iter()
            .any(|error| error.issue.contains("not_reachable_from_entry")),
        "expected an orphaned-handler reachability error, got {errors:?}"
    );
}

#[test]
fn synthetic_path_callee_with_type_inside_cape_lowers_failable() {
    // Fix shape: the rewritten callee keeps its function type, so the
    // direct-def path falls back to the callee type and emits the try-call —
    // the handler stays reachable and the MIR validates.
    let dump = synthetic_path_cape_lowering(true).expect("synthetic-path cape lowering should succeed");
    assert!(
        dump.contains("try_call"),
        "expected a try_call to the synthetic callee, got {dump}"
    );
}

/// Lower the gradus `_quantitas_valid` shape with the fac-body callee rewritten
/// to a synthetic `Path` — simulating faber's package-MIR namespace rewrite
/// with (`preserve_ty`) or without (`!preserve_ty`) the callee function type.
fn synthetic_path_cape_lowering(preserve_ty: bool) -> Result<String, Vec<crate::mir::MirError>> {
    let source = r#"
discretio FormaError {
    FormaInvalida { textus causa }
}
discretio NnError {
    DimensioNegativa { textus causa },
    DimensioSupraLimitem { textus causa },
    ProductumSupraLimitem { textus causa },
    FormaMismatch { textus causa }
}
functio causa(FormaError e) → textus {
    discerne e {
        casu FormaInvalida sit causa { redde causa }
    }
}
functio quantitas(lista<numerus> f) → numerus ⇥ NnError {
    fac {
        redde mensura(f)
    }
    cape err {
        sit textus c ← causa(err)
        si c ≡ "negative dimension" {
            iace finge DimensioNegativa { causa = c }
        }
        iace finge FormaMismatch { causa = c }
    }
}
functio mensura(lista<numerus> f) → numerus ⇥ FormaError {
    si f.longitudo() ≻ 100 {
        iace finge FormaInvalida { causa = "too many" }
    }
    redde f.longitudo()
}
"#;
    let mut unit = analyze(source);

    let mensura_def = unit
        .hir
        .items
        .iter()
        .find_map(|item| {
            if let HirItemKind::Function(function) = &item.kind {
                (unit.interner.resolve(function.name) == "mensura").then_some(item.def_id)
            } else {
                None
            }
        })
        .expect("mensura function def");

    use crate::hir::visit::{HirVisitorMut, walk_expr_mut};
    // The analyzer leaves direct-call callee paths untyped; the package
    // rewrite must COMPUTE the function type (here from the HIR signature,
    // in faber from the resolver's file-interface `FuncSig`).
    let mensura_item = unit
        .hir
        .items
        .iter()
        .find(|item| item.def_id == mensura_def)
        .expect("mensura item");
    let HirItemKind::Function(mensura_function) = &mensura_item.kind else {
        unreachable!("mensura is a function item");
    };
    let mensura_function_ty = unit.types.function(crate::semantic::FuncSig {
        type_params: Vec::new(),
        params: mensura_function
            .params
            .iter()
            .map(|param| crate::semantic::ParamType {
                ty: param.ty,
                mode: crate::semantic::SemanticParamMode::Owned,
                optional: param.optional,
                sponte: param.sponte,
            })
            .collect(),
        ret: mensura_function.ret_ty.expect("mensura return type"),
        err: mensura_function.err_ty,
        is_async: false,
        is_generator: false,
    });
    struct SyntheticCalleeRewriter {
        mensura_def: DefId,
        function_ty: Option<TypeId>,
    }
    impl HirVisitorMut for SyntheticCalleeRewriter {
        fn visit_expr_mut(&mut self, expr: &mut HirExpression) {
            if let HirExpressionKind::Call(callee, ..) = &mut expr.kind
                && let HirExpressionKind::Path(def_id) = callee.kind
                && def_id == self.mensura_def
            {
                callee.kind = HirExpressionKind::Path(DefId(2_000_000_000));
                callee.ty = self.function_ty;
            }
            walk_expr_mut(self, expr);
        }
    }
    crate::hir::visit::walk_program_mut(
        &mut SyntheticCalleeRewriter { mensura_def, function_ty: preserve_ty.then_some(mensura_function_ty) },
        &mut unit.hir,
    );

    match lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default()) {
        Ok(lowered) => Ok(crate::mir::dump_program(lowered.validated.program())),
        Err(errors) => Err(errors),
    }
}

#[test]
// ===========================================================================
// Section H: Aggregate construction — struct, array, map, set, tensor
// ===========================================================================

fn lowers_struct_construction_and_field_read() {
    let dump = dump_source(
        r#"
genus Persona { varia textus nomen varia numerus aetas }
functio nomen() → textus {
    sit Persona p ← Persona { nomen = "Ada", aetas = 36 }
    redde p.nomen
}
"#,
    );

    assert!(dump.contains("construct struct def#"));
    assert!(dump.contains("sym#"));
    assert!(dump.contains("const string sym#"));
    assert!(dump.contains("const int 36"));
    assert!(dump.contains("return _0.sym#"));
}

#[test]
fn lowers_struct_construction_field_defaults() {
    let dump = dump_source(
        r#"
genus Persona { varia textus nomen varia numerus aetas = 0 }
functio aetas() → numerus {
    sit Persona p ← Persona { nomen = "Ada" }
    redde p.aetas
}
"#,
    );

    assert!(dump.contains("construct struct def#"));
    assert!(dump.contains("const string sym#"));
    assert!(dump.contains("const int 0"));
    assert!(dump.contains("return _0.sym#"));
}

#[test]
fn lowers_array_spread_and_index_read() {
    let dump = dump_source(
        r"
functio primus() → numerus {
    sit lista<numerus> xs ← [1, 2]
    sit lista<numerus> ys ← [0, sparge xs]
    redde ys[0]
}
",
    );

    assert!(dump.contains("construct array"));
    assert!(dump.contains("[const int 0, ..._0]"));
    assert!(dump.contains("return _1[const int 0]"));
}

#[test]
fn lowers_array_literal_with_spread_of_inferred_lists() {
    // WHY: the spread sources are inferred bounded-array literals; the outer
    // list construction routes through an inferred local because a
    // return-position spread of a bounded local is rejected by the frontend
    // expected-element check, not by MIR lowering.
    let dump = dump_source(
        r"
functio build() → lista<numerus> {
    sit _ a ← [1, 2]
    sit _ b ← [3, 4]
    sit _ c ← [0, sparge a, sparge b, 5]
    redde c
}
",
    );

    assert!(dump.contains("construct array"));
    assert!(dump.contains("[const int 0, ..._"));
}

#[test]
fn lowers_nested_json_valor_literal() {
    let dump = dump_source(
        r#"
incipit {
    sit valor nidum ← { "extra": { "medium": 1 }, "items": [1, { "x": 2 }] } ↦ valor
    nota nidum
}
"#,
    );

    assert!(dump.contains("construct map"));
    assert!(dump.contains("construct array"));
    assert!(dump.contains("=> const int 1"));
    assert!(dump.contains("=> const int 2"));
    assert!(!dump.contains("nested JSON valor before nested aggregate MIR lowering"));
    assert!(!dump.contains("path that does not resolve to a local value"));
}

#[ignore = "MIR JsonValor — map/set construction lowering not yet implemented"]
#[test]
fn lowers_map_and_set_construction() {
    let map_dump = dump_source(
        r#"
functio lectio() → numerus {
    sit tabula<textus, numerus> xs ← { "a": 1, "b": 2 }
    redde xs["a"]
}
"#,
    );
    assert!(map_dump.contains("construct map"));
    assert!(map_dump.contains("=> const int 1"));
    assert!(map_dump.contains("return _0[const string sym#"));

    let set_dump = dump_source("functio setum() → copia<numerus> { redde [1, 2] ∷ copia<numerus> }");
    assert!(set_dump.contains("construct set"));
    assert!(set_dump.contains("[const int 1, const int 2]"));
}

#[test]
fn edge_empty_lista_ascribed_to_tabula_rejects_semantically() {
    let session = Session::new(la_config().with_target(Target::HirTypeScript));
    let errors = crate::driver::analyze_source(
        &session,
        "test.fab",
        r#"
incipit {
    varia tabula<textus, numerus> vides ← [] ∷ tabula<textus, numerus>
    vides["alpha"] ← 10
    nota vides["alpha"]
}
"#,
        AnalyzeRequest::default(),
    );
    let errors = match errors {
        Ok(_) => panic!("an array ascribed to tabula must be rejected semantically"),
        Err(errors) => errors,
    };
    assert!(
        errors
            .iter()
            .any(|error| error.issue() == Some("array_ascription_to_map")),
        "the rejection must identify the array-to-map ascription rule: {errors:?}"
    );
}

#[test]
fn mir_rejects_map_construction_without_object_entries() {
    let mut types = TypeTable::new();
    let textus = types.primitive(Primitive::Textus);
    let numerus = types.primitive(Primitive::Numerus);
    let array = types.intern(Type::Array(numerus));
    let map = types.intern(Type::Map(textus, numerus));
    let span = Span::default();
    let source = HirExpression { id: HirId(0), kind: HirExpressionKind::Array(Vec::new()), ty: Some(array), span };
    let expr = HirExpression { id: HirId(1), kind: HirExpressionKind::Vacua, ty: Some(map), span };
    let mut builder = FunctionBuilder::new(&types);
    let entry = builder.fresh_block(span);
    builder.switch_to(entry);

    assert!(builder.lower_verte(&source, map, None, &expr).is_none());
    assert!(
        builder.blocks[0].statements.is_empty(),
        "rejected construction must emit no MIR"
    );
    assert_eq!(builder.errors.len(), 1, "the fail-closed guard must report one error");
    assert_eq!(
        builder.errors[0].issue,
        "unsupported_mir_lowering_map_construction_without_object_entries_before_aggregate_mir_lowering"
    );
}

#[test]
fn sanctioned_empty_tabula_constructions_still_lower() {
    let object_dump = dump_source(
        r#"
incipit {
    varia tabula<textus, numerus> vides ← { "alpha": 1 }
    nota vides["alpha"]
}
"#,
    );
    assert!(
        object_dump.contains("construct map:"),
        "the object/entry construction must still build the map: {object_dump}"
    );

    let braces_dump = dump_source(
        r"
incipit {
    varia tabula<textus, numerus> vides ← {}
    nota vides.longitudo()
}
",
    );
    assert!(
        braces_dump.contains("construct map:"),
        "the empty braced literal is the no-entry object spelling and must construct the empty map: {braces_dump}"
    );

    let vacua_dump = dump_source(
        r"
incipit {
    varia tabula<textus, numerus> vides ← vacua
    nota vides.longitudo()
}
",
    );
    assert!(
        vacua_dump.contains("construct map:"),
        "the vacua marker must still construct the empty map: {vacua_dump}"
    );

    let en_empty_dump = dump_source(
        r#"+++
locale = "en"
+++
main {
    mut map<string, int> seen ← empty
    print seen.length()
}
"#,
    );
    assert!(
        en_empty_dump.contains("construct map:"),
        "the en-locale empty marker must still construct the empty map: {en_empty_dump}"
    );
}

#[test]
fn lowers_field_and_index_assignment_places() {
    let field_dump = dump_source(
        r#"
genus Persona { varia textus nomen varia numerus aetas }
functio muta() → numerus {
    varia Persona p ← Persona { nomen = "Ada", aetas = 36 }
    p.aetas ← 37
    redde p.aetas
}
"#,
    );
    assert!(field_dump.contains("_0.sym#"));
    assert!(field_dump.contains("= const int 37"));

    let index_dump = dump_source(
        r"
functio muta() → numerus {
    varia lista<numerus> xs ← [1, 2]
    xs[0] ← 5
    redde xs[0]
}
",
    );
    assert!(index_dump.contains("_0[const int 0] = const int 5"));
    assert!(index_dump.contains("return _0[const int 0]"));
}

#[test]
fn lowers_tensor_bracket_access_to_tensor_runtime_ops() {
    let dump = dump_source(
        r"
functio muta(tf32[4] input, varia tf32[4] output, u32 id) → f32 {
    sit f32 value ← input[id]
    output[id] ← value
    redde value
}
",
    );

    assert!(dump.contains("construct array"));
    assert!(dump.contains("runtime collection tensor_get"));
    assert!(dump.contains("runtime collection tensor_set"));
    assert!(!dump.contains("Tensor"));
}

#[test]
fn lowers_tensor_activatio_relu_to_tensor_relu_runtime_call() {
    let dump = dump_source(
        r"
functio muta(tf32[2] input) → tf32[2] {
    redde input.activatio_relu()
}
",
    );

    assert!(dump.contains("runtime collection tensor_relu"));
    assert!(!dump.contains("method call before runtime/provider MIR lowering"));
}

#[test]
fn lowers_tensor_silu_to_gi3_composed_elementwise_ops() {
    let source = r"
@ nucleum
functio silu(tf32[4] input) → tf32[4] {
    redde input.silu()
}
";
    let unit = analyze(source);
    let lowered =
        lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default()).expect("MIR lowering succeeds");
    validate_program(lowered.validated.program(), lowered.validated.validation())
        .expect("the composed SiLU MIR must validate");
    let dump = dump_program(lowered.validated.program());
    assert!(dump.contains("runtime collection tensor_neg"), "missing negation: {dump}");
    assert!(dump.contains("exp "), "missing tensor Exp unary: {dump}");
    assert!(
        dump.contains("runtime collection tensor_add"),
        "missing denominator add: {dump}"
    );
    assert!(dump.contains("runtime collection tensor_div"), "missing SiLU division: {dump}");
    assert!(!dump.contains("method call before runtime/provider MIR lowering"), "{dump}");
    assert!(!dump.contains("tensor_silu"), "SiLU must not grow a dedicated MIR op: {dump}");
}

#[test]
fn indexed_tensor_view_kernel_lowers_device_brackets_to_projections() {
    let dump = dump_source(
        r"
@ nucleum
functio copy(tf32[4] input, varia tf32[4] output, u32 id) → vacuum {
    output[id] ← input[id]
}
",
    );

    assert!(dump.contains("_1[_2] = _0[_2]"));
    assert!(!dump.contains("runtime collection tensor_get"));
    assert!(!dump.contains("runtime collection tensor_set"));
}

#[test]
fn lowers_optional_chain_non_null_and_coalesce() {
    let chain_dump = dump_source(
        r"
genus Persona { varia textus nomen }
functio maybe(Persona ∪ nihil p) → textus ∪ nihil {
    redde p?.nomen
}
",
    );
    // The receiver is captured into a temp before the guard.
    assert!(chain_dump.contains("option chain(%0, .sym#"), "{chain_dump}");

    let non_null_dump = dump_source(
        r"
genus Persona { varia textus nomen }
functio certum(Persona ∪ nihil p) → textus {
    redde p!.nomen
}
",
    );
    assert!(non_null_dump.contains("option unwrap_assert(_0)"));
    assert!(non_null_dump.contains("return %0.sym#"));

    let coalesce_dump = dump_source(r#"functio maybe(textus ∪ nihil name) → textus { redde name vel "ignotus" }"#);
    assert!(coalesce_dump.contains("option coalesce(_0, const string sym#"));
}

#[test]
fn lowers_non_null_method_call_with_store_back_only_for_receiver_writes() {
    // audit1-a2: `xs!.appende(1)` writes the unwrapped payload temp, so the
    // storage is re-wrapped from it; the read `xs!.longitudo()` is not.
    let write_dump = dump_source(
        r"
functio f() → vacuum {
    varia lista<numerus> ∪ nihil xs ← [1]
    xs!.appende(1)
}
",
    );
    assert!(write_dump.contains("option unwrap_assert("), "{write_dump}");
    assert!(write_dump.contains("option some("), "{write_dump}");
    assert!(!write_dump.contains("method call before runtime/provider MIR lowering"));

    let read_dump = dump_source(
        r"
functio f(lista<numerus> ∪ nihil xs) → numerus {
    redde xs!.longitudo()
}
",
    );
    assert!(read_dump.contains("option unwrap_assert(_0)"), "{read_dump}");
    assert!(!read_dump.contains("option some("), "{read_dump}");
}

#[test]
fn lowers_enum_variant_construction() {
    let dump = dump_source(
        r#"
discretio Eventus { Bonum { textus nuntius } }
functio crea() → Eventus {
    redde finge Bonum { nuntius = "ok" } ∷ Eventus
}
"#,
    );

    assert!(dump.contains("construct variant def#"));
    assert!(dump.contains("{sym#"));
    assert!(dump.contains("const string sym#"));
}

#[test]
fn lowers_unit_enum_variant_path_values() {
    let source = r"
ordo Color { rubrum, viridis }
functio elige_color() → Color {
    redde Color.rubrum
}
";
    let dump = dump_source(source);

    assert!(dump.contains("construct variant def#"));
    assert!(dump.contains("return %"));
    assert!(!dump.contains("path that does not resolve to a local value"));

    let unit = analyze(source);
    let lowered =
        lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default()).expect("MIR lowering succeeds");
    let (enum_def, variants) = lowered
        .validated
        .validation()
        .enum_variants
        .iter()
        .next()
        .expect("unit enum metadata");
    assert_eq!(variants.len(), 2);
    for variant in variants {
        assert_eq!(lowered.validated.validation().variant_parents.get(variant), Some(enum_def));
        assert!(
            lowered
                .validated
                .validation()
                .variant_fields
                .get(variant)
                .is_some_and(std::collections::HashMap::is_empty)
        );
    }
}

#[test]
fn varia_nominal_struct_with_tagged_union_field_gets_recursive_zero_init() {
    // M-007 no-initializer `varia` of a nominal struct whose fields include a
    // tagged union without a `nihil` member (mirrors `varia parametrum.Parametrum
    // p2` in gradus/src/optimize.fab:390). The decl-site zero must recurse:
    // struct → f32 zero temp + enum zero (first variant) instead of falling
    // through to the `_ =>` unsupported arm.
    let dump = dump_source(
        r"
discretio Statio { Trainabilis, Gelida }
genus Parametrum {
    varia numerus versio
    varia f32 lentus
    varia Statio statio
}
functio f() → numerus {
    varia Parametrum p2
    redde 1
}
",
    );

    assert!(
        dump.contains("construct struct def#"),
        "struct zero must construct a struct aggregate"
    );
    assert!(
        dump.contains("construct variant def#"),
        "statio field zero must construct the first variant"
    );
    assert!(dump.contains("const int 0"), "numerus field zero");
    assert!(dump.contains("const float 0.0"), "f32 field zero through the sized typed temp");
    assert!(!dump.contains("type zero-init before MIR lowering completed"));
}

#[test]
fn lowers_sized_numeric_varia_zero_init_through_typed_temp() {
    // LIB-MIR fire-22 F2 regression: lock the SizedNumeric zero-init arms in
    // type_zero_operand (radix 43c0102ba). `varia` of a sized numerus/fractus
    // (e.g. f32, numerus<u8>) must wrap its zero in a typed temp carrying the
    // sized type via assign_temp — a bare constant carries the primitive
    // numerus/fractus type, which the validator rejects against a sized
    // destination. Without the arms the decl falls to the unsupported
    // "type zero-init before MIR lowering completed" diagnostic.
    let dump = dump_source(
        r"
functio f() → numerus {
    varia f32 x
    varia u8 y
    redde 1
}
",
    );

    assert!(
        dump.contains("%0 = const float 0.0"),
        "f32 zero must go through the sized typed temp: {dump}"
    );
    assert!(
        dump.contains("%1 = const int 0"),
        "u8 zero must go through the sized typed temp: {dump}"
    );
    assert!(!dump.contains("type zero-init before MIR lowering completed"), "{dump}");
    assert!(dump.contains("return"), "lowering must reach return: {dump}");
}

#[test]
fn lowers_policy_float_varia_zero_init_through_typed_temp() {
    // N7E-3 (D2): a policy-float cell zero-inits like the bare float of its
    // width: a typed temp carrying the cell type, never the unsupported
    // "type zero-init before MIR lowering completed" diagnostic.
    let dump = dump_source(
        r"
functio f() → numerus {
    varia exactus<f32> x
    varia saturatus<f64> y
    redde 1
}
",
    );

    assert!(
        dump.matches("const float 0.0").count() >= 2,
        "each cell zero must go through a typed temp: {dump}"
    );
    assert!(!dump.contains("type zero-init before MIR lowering completed"), "{dump}");
    assert!(dump.contains("return"), "lowering must reach return: {dump}");
}

#[test]
fn lowers_ordo_elige_statement_continues_after_match() {
    let dump = dump_source(
        r#"
ordo Color { rubrum, viridis }
incipit {
    sit _ color ← Color.rubrum
    elige color {
        casu Color.rubrum { nota "rubrum" }
        casu Color.viridis { nota "viridis" }
    }
    nota "finis"
}
"#,
    );

    assert_eq!(
        dump.matches("runtime diagnostic nota(const string sym#")
            .count(),
        3
    );
    assert!(dump.contains("unreachable"));
    assert!(
        dump.contains("return"),
        "statement after exhaustive unit-variant elige must remain reachable: {dump}"
    );
}

#[test]
fn lowers_all_returning_variant_discerne_without_fallthrough_return() {
    let dump = dump_source(
        r#"
discretio Condicio { Agens, Iners }
functio nomina(Condicio s) → textus {
    discerne s {
        casu Agens { redde "agens" }
        casu Iners { redde "iners" }
    }
}
"#,
    );

    assert!(dump.contains("return %"));
    assert!(
        !dump.contains("return\n"),
        "all-returning variant discerne must not synthesize a no-value return: {dump}"
    );
}

#[test]
fn lowers_all_returning_bivalens_discerne_without_fallthrough_return() {
    let dump = dump_source(
        r#"
functio signum(bivalens b) → numerus {
    discerne b {
        casu verum { redde 1 }
        casu falsum { redde 0 }
    }
}
"#,
    );

    assert!(dump.contains("return %") || dump.contains("return 1"));
    assert!(
        !dump.contains("return\n"),
        "all-returning bivalens discerne must not synthesize a no-value return: {dump}"
    );
}

#[test]
fn lowers_all_returning_type_test_discerne_without_fallthrough_return() {
    let dump = dump_source(
        r#"
functio mensura(numerus ∪ textus u) → numerus {
    discerne u {
        casu numerus sit n { redde n }
        casu textus sit s { redde 0 }
    }
}
"#,
    );

    assert!(
        !dump.contains("return\n"),
        "all-returning type-test discerne must not synthesize a no-value return: {dump}"
    );
}

#[test]
fn lowers_scalar_verte_as_target_typed_passthrough() {
    let dump = dump_source(
        r"
functio absent() → textus ∪ nihil {
    redde nulla ∷ textus ∪ nihil
}
",
    );

    assert!(dump.contains("const nil: ty#"));
    assert!(dump.contains("return %"));
    assert!(!dump.contains("verte cast before aggregate MIR lowering"));
}

#[test]
fn lowers_inter_membership_to_collection_contains() {
    let dump = dump_source(
        r#"
functio validum(textus status) → bivalens {
    redde status ∈ ["pending", "active"]
}
"#,
    );

    assert!(dump.contains("runtime collection contains"));
    assert!(!dump.contains("binary operator without a MIR primitive"));
}

// WHY: rejects_unsupported_map_spread_shape was removed in literal-family
// Stage 6. It tested MIR rejection of `sparge` on anonymous objects, a retired
// feature. JSON valor literals reject `sparge` at parse time, so the MIR-level
// rejection no longer has a producer.

#[test]
fn lowers_diagnostic_verbs_to_runtime_intrinsics() {
    let dump = dump_source(
        r#"
functio log(textus name) → vacuum {
    nota "salve"
    vide name
    mone "cave"
}
"#,
    );

    assert!(dump.contains("runtime diagnostic nota(const string sym#"));
    assert!(dump.contains("runtime diagnostic vide(_0)"));
    assert!(dump.contains("runtime diagnostic mone(const string sym#"));
}

#[test]
fn lowers_multi_arg_diagnostics_to_unary_runtime_calls() {
    let dump = dump_source(
        r#"
functio log(textus name, numerus age) → vacuum {
    nota "Name:", name, "Age:", age
}
"#,
    );

    assert_eq!(dump.matches("runtime diagnostic nota(").count(), 1, "{dump}");
    assert!(dump.contains("runtime convert runtime ty#"), "{dump}");
    assert!(!dump.contains(", _0,"));
}

#[test]
fn lowers_adfirma_to_assert_runtime_intrinsics() {
    let dump = dump_source(
        r#"
functio check(textus name, numerus count) → vacuum {
    adfirma count ≻ 0
    adfirma name ≢ "" mori "name must not be empty"
}
"#,
    );

    assert!(dump.contains("runtime assert(%0) -> ty#"));
    assert!(dump.contains("runtime assert(%1, const string sym#"));
    assert!(!dump.contains("adfirma before assert intrinsic MIR lowering"));
}

#[test]
fn lowers_ascii_format_hole_with_materialized_conversion_target() {
    let dump = dump_source(
        r#"+++
locale = "en"
+++
main {
    let _ inferred_hex ← 255 ↦ ascii<_> via Hex
    let _ inferred_bin ← 255 ↦ ascii<_> via Bin
    let _ inferred_oct ← 255 ↦ ascii<_> via Oct
}
"#,
    );
    assert_eq!(
        count_occurrences(&dump, "runtime convert runtime"),
        3,
        "each inferred ascii format pack must lower to one conversion:\n{dump}"
    );
}

#[test]
fn lowers_format_conversion_collection_and_provider_runtime_intrinsics() {
    let format_dump = dump_source(r#"functio greet(textus name) → textus { redde "Salve, §!"(name) }"#);
    assert!(format_dump.contains("runtime format_string template sym#"));
    assert!(!format_dump.contains("format!"));

    let conversion_dump = dump_source(r"functio parse(textus raw) → i32 { redde raw ↦ i32 via Hex ⊥ 0 ∷ i32 }");
    assert!(conversion_dump.contains("runtime convert runtime ty#"));
    assert!(conversion_dump.contains(" -> ty#"));
    assert!(conversion_dump.contains("hints [sym#"));
    assert!(conversion_dump.contains("recovery"));

    let collection_dump = dump_source(r"functio count(lista<numerus> xs) → numerus { redde xs.longitudo() }");
    assert!(collection_dump.contains("runtime collection length(_0)"));
    assert!(!collection_dump.contains(".len"));

    let provider_dump = dump_source(
        r#"
importa ex "norma:consolum" consolum
functio read() → ignotum {
    redde consolum.lege()
}
"#,
    );
    assert!(provider_dump.contains("runtime provider sym#"));
    assert!(provider_dump.contains("::sym#"));
}

#[test]
fn lowers_expression_ad_to_sermo_runtime_intrinsics() {
    let dump = dump_source(
        r#"
incipit {
    sit textus t ← ad 'runtime:echo' ("salve") ↦ textus
    adfirma t ≡ "salve"
}
"#,
    );

    assert!(dump.contains("runtime sermo_open(const string sym#"));
    assert!(dump.contains("runtime convert runtime ty#"));
    assert!(dump.contains(" -> ty#"));
    assert!(dump.contains("runtime sermo_set_opener(%0, %1)"));
    assert!(!dump.contains("ad expression before stream conversation MIR lowering"));
}

#[test]
fn lowers_numeric_operator_methods_to_binary_mir() {
    let dump = dump_source(
        r"
incipit {
    sit fractus x ← 2.0
    sit fractus chained ← x.addita(1.0).multiplicata(3.0)
    varia fractus acc ← 10.0
    acc.adde(5.0)
    sit numerus mask ← 12
    sit numerus bits ← mask.coniuncta(10)
    nota chained
    nota acc
    nota bits
}
",
    );

    assert!(dump.contains("%0 = _0 + const float 1.0"));
    assert!(dump.contains("%1 = %0 * const float 3.0"));
    assert!(dump.contains("%2 = _2 + const float 5.0"));
    assert!(dump.contains("_2 = %2"));
    assert!(dump.contains("%3 = _3 & const int 10"));
    assert!(!dump.contains("method call before runtime/provider MIR lowering"));
}

#[test]
fn lowers_logical_operators_to_short_circuit_branches() {
    let dump = dump_source(
        r#"
functio side() → bivalens {
    nota "bad"
    redde verum
}

incipit {
    sit _ left_false ← falsum et side()
    sit _ left_true ← verum aut side()
    nota left_false
    nota left_true
}
"#,
    );

    assert!(dump.contains("branch const bool false"));
    assert!(dump.contains("branch const bool true"));
    assert!(!dump.contains("const bool false and"));
    assert!(!dump.contains("const bool true or"));
}

#[test]
fn rejects_unsupported_runtime_method_shapes() {
    // Need: a method absent from a builtin receiver's registry is
    // refused at the checker as `unknown_method`, so the call never reaches
    // MIR lowering. The fail-closed boundary moved from lowering to analysis.
    let session = Session::new(la_config().with_target(Target::HirTypeScript));
    let diagnostics = crate::driver::analyze_source(
        &session,
        "test.fab",
        r"functio malum(lista<numerus> xs) → numerus { redde xs.praepone(1) }",
        AnalyzeRequest::default(),
    )
    .map(|_| ())
    .expect_err("unsupported method remains fail-closed at analysis");

    assert!(
        diagnostics.iter().any(|diag| diag
            .args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == "unknown_method")),
        "expected the named unknown_method error: {diagnostics:?}"
    );
}

#[test]
fn lowers_imported_nominal_method_to_provider_runtime_call() {
    use crate::file_interface::{
        FileExport, FileExportKind, FileInterface, InterfaceCallable, InterfaceMethodExport, InterfaceStructExport,
        InterfaceTypeSnapshot,
    };
    let mut interface = FileInterface::new();
    interface.insert(FileExport {
        name: "Sample".to_owned(),
        kind: FileExportKind::Struct(InterfaceStructExport {
            is_json: false,
            implements: Vec::new(),
            params: Vec::new(),
            name: "Sample".to_owned(),
            fields: Vec::new(),
            methods: vec![InterfaceMethodExport {
                name: "metire".to_owned(),
                callable: InterfaceCallable {
                    type_params: Vec::new(),
                    params: Vec::new(),
                    ret: Box::new(InterfaceTypeSnapshot::Primitive(Primitive::Numerus)),
                    err: None,
                    is_async: false,
                    is_generator: false,
                    tensor_edge_policy_requirements: Vec::new(),
                    device: None,
                },
                visibility: crate::syntax::MemberVisibility::Publica,
                unavailable: Vec::new(),
            }],
            annotation_contract: None,
        }),
    });

    let session = Session::new(la_config().with_target(Target::HirTypeScript));
    let unit = crate::driver::analyze_source(
        &session,
        "test.fab",
        r#"
importa ex "demo:lib" demo
functio legebatur(demo.Sample value) → numerus {
    redde value.metire()
}
"#,
        crate::driver::AnalyzeRequest {
            cli_program: None,
            contract: ModuleImportContract {
                namespace_exports: std::collections::BTreeMap::new(),
                file_interfaces: std::collections::BTreeMap::from([("demo".to_owned(), interface)]),
                ..ModuleImportContract::default()
            },
        },
    )
    .expect("imported nominal method source analyzes");
    let dump = dump_analyzed_unit(unit.application_view()).expect("imported nominal method lowers");

    assert!(dump.contains("runtime provider"), "expected provider MIR, got:\n{dump}");
    assert!(!dump.contains("method call before runtime/provider MIR lowering"));
}

#[test]
fn lowers_cross_family_scalar_conversion_to_a_runtime_convert() {
    // Cross-family scalar changes (float -> integer, narrowing) are `↦`
    // conversions; `∷` no longer admits them (SEM016). The MIR has to carry
    // the conversion (`1.9 ↦ numerus via Trunc`, `300.0 ↦ i8 via Trunc`); a pass-through
    // operand left the runner storing the raw float carrier in a numerus slot
    //. Same-family provable ascriptions stay pass-through and
    // keep no conversion.
    let dump = dump_source(
        r"
functio ascriptiones() → numerus ⇥ textus {
  sit numerus truncatum ← 1.9 ↦ numerus via Trunc
  sit i8 saturatum ← 300.0 ↦ i8 via Trunc
  sit i32 idem ← 7 ∷ i32
  redde truncatum
}",
    );

    assert_eq!(dump.matches("runtime convert runtime").count(), 2, "{dump}");
    assert!(!dump.contains("runtime convert cast"), "{dump}");
}

#[test]
// ===========================================================================
// Section I: Collection runtime methods — lista, tabula, copia, textus, intervallum
// ===========================================================================

fn lowers_collection_runtime_methods_to_intrinsics() {
    let dump = dump_source(
        r"
incipit {
    varia lista<numerus> items ← vacua
    items.appende(1)
    nota items.longitudo()
    nota items.primus()
}
",
    );

    assert!(dump.contains("runtime collection append"));
    assert!(dump.contains("runtime collection length"));
    assert!(dump.contains("runtime collection first"));
    assert!(!dump.contains("method call before runtime/provider MIR lowering"));
}

#[test]
fn lowers_lista_mutatio_methods() {
    let dump = dump_source(
        r"
incipit {
    varia _ res ← [3, 1, 2] ∷ lista<numerus>
    res.appende(4)
    sit _ primusremotus ← res.decapita()
    sit _ ultimusremotus ← res.detrahe()
    res.inverte()
    res.ordina()
    nota res, primusremotus, ultimusremotus
}
",
    );

    assert!(dump.contains("runtime collection append"));
    assert!(dump.contains("runtime collection remove_first"));
    assert!(dump.contains("runtime collection reverse_in_place"));
    assert!(dump.contains("runtime collection sort"));
    assert!(dump.contains("_0 = %"));
    assert!(!dump.contains("method call before runtime/provider MIR lowering"));
    assert!(!dump.contains("path that does not resolve to a local value"));
}

#[test]
fn lowers_lista_copy_view_methods() {
    let dump = dump_source(
        r"
incipit {
    sit _ res ← [1, 2, 3, 4, 5] ∷ lista<numerus>
    sit _ sectio ← res.sectio(1, 4)
    sit _ primaduo ← res.prima(2)
    sit _ ultimaduo ← res.ultima(2)
    sit _ omissaduo ← res.omissa(2)
    nota sectio, primaduo, ultimaduo, omissaduo
}
",
    );

    assert!(dump.contains("runtime collection slice"));
    assert!(dump.contains("runtime collection take"));
    assert!(dump.contains("runtime collection take_last"));
    assert!(dump.contains("runtime collection drop_first"));
    assert!(!dump.contains("method call before runtime/provider MIR lowering"));
    assert!(!dump.contains("path that does not resolve to a local value"));
}

#[test]
fn lowers_tensor_longitudo_to_collection_length() {
    let dump = dump_source(
        r"
incipit {
    sit tensor<f32, []> empty_tensor ← vacua
    nota empty_tensor.longitudo()
}
",
    );

    assert!(dump.contains("runtime collection length"));
    assert!(!dump.contains("method call before runtime/provider MIR lowering"));
}

#[test]
fn lowers_tensor_intrinsic_methods_to_collection_ops() {
    let dump = dump_source(
        r"
incipit {
    sit lista<f32> flat ← [1.0, 2.0, 3.0, 4.0]
    varia tf32[2, 2] grid ← flat ↦ tf32[2, 2]
    nota grid.magnitudines()
    sit tf32[4] reshaped ← grid.forma([4]).materialize()
    grid.ponde([0, 0], 9.0)
    sit f32 corner ← grid.accipe([0, 0])
    varia tf32[2, 2] filler ← tf32[2, 2] {}.reple(1.0)
    filler.reple(4.0)
    sit tf32[1, 2] slice ← grid.sectio(0, 1)
    sit lista<f32> out ← slice.planata()
    sit tf32[2, 2] sum ← grid + filler
    sit tf32[2, 2] diff ← sum - filler
    sit tf32[2, 2] prod ← diff ⊙ grid
    nota prod.summa()
    nota prod.media()
    nota reshaped.longitudo()
    nota corner
    nota out.longitudo()
}
",
    );

    for op in [
        "tensor_from_flat",
        "tensor_shape",
        "tensor_reshape",
        "tensor_set",
        "tensor_get",
        "tensor_create",
        "tensor_fill",
        "tensor_slice",
        "tensor_flatten",
        "tensor_add",
        "tensor_sub",
        "tensor_mul",
        "sum",
        "tensor_mean",
    ] {
        assert!(dump.contains(&format!("runtime collection {op}")), "missing {op} in:\n{dump}");
    }
    assert!(!dump.contains("method call before runtime/provider MIR lowering"));
}

#[test]
fn lowers_generic_tensor_formata_to_tensor_reshape() {
    let dump = dump_source(
        r"
incipit {
    sit lista<f32> flat ← [1.0, 2.0, 3.0, 4.0]
    varia tf32[2, 2] grid ← flat ↦ tf32[2, 2]
    sit tf32[4] view ← grid.formata<tf32[4]>()
    nota view.longitudo()
}
",
    );

    assert!(
        dump.contains("runtime collection tensor_reshape"),
        "missing tensor_reshape in:\n{dump}"
    );
    assert!(!dump.contains("method call before runtime/provider MIR lowering"));
}

fn assert_lista_to_tensor_conversio_lowers_to_tensor_from_flat(source: &str) {
    let dump = dump_source(source);
    assert!(
        dump.contains("runtime collection tensor_from_flat"),
        "missing tensor_from_flat in:\n{dump}"
    );
    assert!(
        !dump.contains("runtime convert runtime"),
        "unexpected generic convert in:\n{dump}"
    );
}

#[test]
fn lowers_shaped_lista_to_tensor_conversio_to_tensor_from_flat() {
    assert_lista_to_tensor_conversio_lowers_to_tensor_from_flat(
        r"
incipit {
    sit lista<f32> flat ← [1.0, 2.0, 3.0, 4.0]
    sit tf32[2, 2] grid ← flat ↦ tf32[2, 2]
    nota grid.magnitudines()
}
",
    );
}

#[test]
fn lowers_rectangular_lista_literal_to_tensor_conversio_to_tensor_from_flat() {
    assert_lista_to_tensor_conversio_lowers_to_tensor_from_flat(
        r"
incipit {
    sit tf32[2, 2] grid ← [[1.0, 2.0], [3.0, 4.0]] ↦ tf32[2, 2]
    nota grid.magnitudines()
}
",
    );
}

#[test]
fn lowers_literal_lista_to_narrow_float_tensor_conversio_to_tensor_from_flat() {
    // Typecheck admits the literal's untyped float constants into the `f32`
    // tensor element (padded snapshot route); lowering must take the same
    // route instead of leaving a generic aggregate-to-aggregate convert.
    assert_lista_to_tensor_conversio_lowers_to_tensor_from_flat(
        r"
incipit {
    sit tf32[3] row ← [1.0, 2.0, 3.0] ↦ tf32[3]
    nota row.longitudo()
}
",
    );
}

#[test]
fn lowers_copia_runtime_methods_to_intrinsics() {
    let dump = dump_source(
        r"
incipit {
    varia copia<numerus> left ← vacua
    varia copia<numerus> right ← vacua
    left.adde(1)
    right.adde(2)
    nota left.habet(1)
    nota left.dele(1)
    nota left.vacua()
    nota left.unio(right)
    nota left.intersectio(right)
    nota left.differentia(right)
    nota left.symmetrica(right)
    nota left.subcopia(right)
    nota left.supercopia(right)
}
",
    );

    assert!(dump.contains("runtime collection append"));
    assert!(dump.contains("runtime collection contains"));
    assert!(dump.contains("runtime collection delete"));
    assert!(dump.contains("runtime collection is_empty"));
    assert!(dump.contains("runtime collection union"));
    assert!(dump.contains("runtime collection intersection"));
    assert!(dump.contains("runtime collection difference"));
    assert!(dump.contains("runtime collection symmetric_difference"));
    assert!(dump.contains("runtime collection is_subset"));
    assert!(dump.contains("runtime collection is_superset"));
    assert!(!dump.contains("method call before runtime/provider MIR lowering"));
}

#[test]
fn lowers_intervallum_coercere_to_interval_clamp() {
    let dump = dump_source(
        r"
incipit {
    sit intervallum<numerus> wide ← 0‥10
    nota wide.coercere(2…8)
}
",
    );

    assert!(dump.contains("runtime collection interval_clamp"));
    assert!(!dump.contains("method call before runtime/provider MIR lowering"));
}

#[test]
fn lowers_tabula_runtime_methods_to_intrinsics() {
    let dump = dump_source(
        r#"
incipit {
    varia tabula<textus, numerus> items ← vacua
    items.pone("a", 1)
    nota items.accipe("a")
    nota items.habet("a")
    nota items.dele("a")
    nota items.claves()
    nota items.valores()
    nota items.longitudo()
    nota items.vacua()
}
"#,
    );

    assert!(dump.contains("runtime collection put"));
    assert!(dump.contains("runtime collection index"));
    assert!(dump.contains("runtime collection contains"));
    assert!(dump.contains("runtime collection delete"));
    assert!(dump.contains("runtime collection keys"));
    assert!(dump.contains("runtime collection values"));
    assert!(dump.contains("runtime collection length"));
    assert!(dump.contains("runtime collection is_empty"));
    assert!(!dump.contains("method call before runtime/provider MIR lowering"));
}

#[test]
fn edge_empty_tabula_habet_lowers_to_collection_contains() {
    let dump = dump_source(
        r#"
incipit {
    sit tabula<textus, numerus> items ← vacua
    nota items.habet("a")
}
"#,
    );

    assert!(
        dump.contains("runtime collection contains"),
        "habet on empty tabula must lower to contains: {dump}"
    );
    assert!(
        dump.contains("construct map:"),
        "empty tabula init must lower to construct map: {dump}"
    );
}

#[test]
fn lowers_textus_runtime_methods_to_intrinsics() {
    let dump = dump_source(
        r#"
incipit {
    sit _ nuntius ← " Ave Roma "
    nota nuntius.longitudo()
    nota nuntius.sectio(1, 4)
    nota nuntius.continet("Roma")
    nota nuntius.initium(" Ave")
    nota nuntius.finis(" ")
    nota nuntius.maiuscula()
    nota nuntius.minuscula()
    nota nuntius.recide()
    nota nuntius.divide(" ")
    nota nuntius.muta("Roma", "Munde")
}
"#,
    );

    assert!(dump.contains("runtime collection length"));
    assert!(dump.contains("runtime collection slice"));
    assert!(dump.contains("runtime collection contains"));
    assert!(dump.contains("runtime collection starts_with"));
    assert!(dump.contains("runtime collection ends_with"));
    assert!(dump.contains("runtime collection uppercase"));
    assert!(dump.contains("runtime collection lowercase"));
    assert!(dump.contains("runtime collection trim"));
    assert!(dump.contains("runtime collection split"));
    assert!(dump.contains("runtime collection replace"));
    assert!(!dump.contains("method call before runtime/provider MIR lowering"));
}

#[test]
fn lowers_textus_accipe_to_collection_index_intrinsic() {
    // LIB-MIR fire-22 F2 regression: lock the Textus `accipe` registry row
    // (radix 43c0102ba). Without the row, collection_method_op → lookup
    // for (Textus, accipe, 1) → None, and the call fails with the deferred
    // "method call before runtime/provider MIR lowering" diagnostic.
    let dump = dump_source(
        r#"
incipit {
    sit textus nuntius ← "Ave Roma"
    nota nuntius.accipe(0)
}
"#,
    );

    assert!(
        dump.contains("runtime collection index"),
        "textus.accipe must lower to the collection Index intrinsic: {dump}"
    );
    assert!(
        !dump.contains("method call before runtime/provider MIR lowering"),
        "textus.accipe must not fall through to the deferred method-call diagnostic: {dump}"
    );
    assert!(dump.contains("return"), "lowering must reach return: {dump}");
}

#[test]
fn edge_empty_textus_longitudo_lowers_to_collection_length() {
    let dump = dump_source(
        r#"
incipit {
    sit _ len ← "".longitudo()
    nota len
}
"#,
    );

    assert!(
        dump.contains("runtime collection length(const string sym#"),
        "empty string len must lower to collection length on const: {dump}"
    );
    assert!(dump.contains("return"), "lowering must reach return: {dump}");
    assert!(!dump.contains("method call before runtime/provider MIR lowering"), "{dump}");
}

#[test]
fn lowers_numeric_runtime_methods_to_intrinsics() {
    let dump = dump_source(
        r"
incipit {
    sit numerus n ← -7
    nota n.absolutum()
    nota n.signum()
    nota n.minimus(3)
    nota n.maximus(3)

    sit fractus f ← -3.75
    nota f.absolutum()
    nota f.signum()
    nota f.minimus(2.5)
    nota f.maximus(2.5)
    nota f.approximata(3.8, 0.1)
    nota f.rotunda()
    nota f.pavimentum()
    nota f.tectum()
    nota f.trunca()
}
",
    );

    assert!(dump.contains("runtime collection absolute"));
    assert!(dump.contains("runtime collection signum"));
    assert!(dump.contains("runtime collection minimum"));
    assert!(dump.contains("runtime collection maximum"));
    assert!(dump.contains("runtime collection approximate"));
    assert!(dump.contains("runtime collection round"));
    assert!(dump.contains("runtime collection floor"));
    assert!(dump.contains("runtime collection ceiling"));
    assert!(dump.contains("runtime collection truncate"));
    assert!(!dump.contains("method call before runtime/provider MIR lowering"));
}

#[test]
fn lowers_potentia_to_exponentiation_identity() {
    // `potentia` is a CodegenOnly numeric registry row (no `MirCollectionOp`
    // surface). Fractus receivers lower to IEEE 754 `pow` (operator ruling
    // 2026-09-24) composed from existing MIR ops — special-case branches, an
    // exact squaring path, and `(y · ln(x)).exp()` — no new MIR op. Regression for the PKG001
    // "method call before runtime/provider MIR lowering" the gradus nn.fab
    // `_exponens` (`(2.718281828459045 ∷ f32).potentia(x)`) tripped on the
    // PML4 FMIR lane (fails on revert of the `lower_potentia` arm).
    let source = r"
functio _exponens(f32 x) → f32 {
    redde (2.718281828459045 ∷ f32).potentia(x)
}
";
    let unit = analyze(source);
    let lowered =
        lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default()).expect("MIR lowering succeeds");
    validate_program(lowered.validated.program(), lowered.validated.validation())
        .expect("the IEEE pow MIR must validate (special-case/power/join blocks reachable)");
    let dump = dump_program(lowered.validated.program());
    assert!(
        !dump.contains("method call before runtime/provider MIR lowering"),
        "potentia must not trip the unsupported-method fallthrough: {dump}"
    );
    assert!(
        dump.contains("log ") && dump.contains("exp "),
        "the exponentiation identity (exp(exp·ln(base))) must compose from MIR unary ops: {dump}"
    );
    assert!(dump.contains("branch "), "the IEEE special-case branches must appear: {dump}");
}

#[test]
fn lowers_collection_higher_order_methods_with_synthetic_closures() {
    let dump =
        dump_source(r"functio evens(lista<numerus> xs) → lista<numerus> { redde xs.filtrata(numerus x ∴ x % 2 ≡ 0) }");

    assert!(dump.contains("runtime collection length"));
    assert!(dump.contains("runtime collection append"));
    assert!(dump.contains("call f1"));
    assert!(!dump.contains("collection higher-order methods before callable-value MIR lowering"));
}

#[test]
fn lowers_nested_collection_higher_order_synthetic_closures() {
    let dump = dump_source(
        r"functio counts(lista<numerus> xs) → lista<numerus> {
  redde xs.mappata(numerus x ∴ [x].filtrata(numerus y ∴ y ≥ 0).longitudo())
}",
    );

    assert!(dump.contains("function f1"));
    assert!(dump.contains("function f2"));
    assert!(dump.contains("call f2"));
    assert!(dump.contains("call f1"));
}

#[test]
fn production_named_public_kernel_metadata_reaches_device_program_selection() {
    let unit = analyze(
        r"
@ nucleum
@ publica
functio publica_entry(f32 value) → f32 {
    redde value
}

@ nucleum
functio privata_helper(f32 value) → f32 {
    redde value
}
",
    );
    let lowered =
        lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default()).expect("MIR lowering succeeds");
    let (program, _semantics, _steps) = radix_mir::device_program::build_device_program(
        &lowered.validated,
        &lowered.interner,
        &radix_mir::device::MirCompanionMap::default(),
        None,
        radix_mir::device_program::DeviceProgramLifetime::SingleRun,
        0,
        &lowered.device_function_metadata,
    )
    .expect("device-program admission succeeds");

    assert_eq!(program.kernels.len(), 1, "only the public named kernel is admitted");
    let admitted = &program.kernels[0];
    // ca2499dc-3.1: the admitted entry carries the compiler-owned `k<id>`
    // spelling; the source name survives only as decoration.
    assert_eq!(
        admitted.entry, "k0",
        "the public named kernel is the selected entry under its k<id> name"
    );
}

#[test]
fn lowers_mappata_callback_to_result_element_type() {
    let dump = dump_source(
        r#"functio labels(lista<numerus> xs) → lista<textus> {
  redde xs.mappata(numerus x → textus ∴ "§"(x))
}"#,
    );

    assert!(dump.contains("function f1 -> ty#0"));
    assert!(dump.contains("call f1"));
    assert!(dump.contains("runtime collection append"));
}

#[test]
fn lowers_captured_collection_callback_environment() {
    let dump = dump_source(
        r"functio scale(lista<numerus> xs, numerus factor) → lista<numerus> {
  redde xs.mappata(numerus x ∴ x * factor)
}",
    );

    assert!(dump.contains("construct closure_env env"), "{dump}");
    assert!(dump.contains("call closure f"), "{dump}");
    assert!(dump.contains(".env"), "{dump}");
    assert!(!dump.contains("path that does not resolve to a local value"), "{dump}");
}

#[test]
fn lowers_lista_reducta_with_synthetic_reducer() {
    let dump = dump_source(
        r"functio total(lista<numerus> xs) → numerus {
  redde xs.reducta((numerus acc, numerus x) ∴ acc + x, 0)
}",
    );

    assert!(dump.contains("runtime collection length"));
    assert!(dump.contains("function f1"));
    assert!(dump.contains("call f1"));
    assert!(!dump.contains("method call before runtime/provider MIR lowering"));
    assert!(!dump.contains("path that does not resolve to a local value"));
}

#[test]
fn reducta_does_not_overwrite_its_initial_value() {
    // The fold rewrites its accumulator every step; the initial value is a
    // caller binding and must read the same after the call (need:
    // gradus `kv.extend` reduced onto `c.history` and corrupted the cache).
    let source = r"functio main2() → vacuum {
  sit lista<numerus> base ← [1]
  sit lista<numerus> folded ← [3, 4].reducta((lista<numerus> acc, numerus x) ∴ acc.addita(x), base)
  nota folded.longitudo()
  nota base.longitudo()
}

incipit {
  main2()
}";
    let mut host = crate::mir::BufferHost::default();
    crate::mir::run_source(
        &Session::new(la_config().with_target(Target::HirTypeScript)),
        "reducta-initial.fab",
        source,
        &mut host,
    )
    .unwrap_or_else(|error| panic!("reducta initial run must execute: {error:?}"));
    assert_eq!(host.stdout_lines, ["3", "1"]);
}

#[test]
fn reducta_arg_extrema_keep_first_index_for_all_nan_sources() {
    let source = r"functio maximum(tensor<f64, [3]> values) → numerus {
  redde reducta via argmaxima ex values
}
functio minimum(tensor<f64, [3]> values) → numerus {
  redde reducta via argminima ex values
}
incipit {
  sit lista<f64> flat ← [nonnumerus, nonnumerus, nonnumerus]
  sit tf64[3] values ← flat ↦ tf64[3]
  nota maximum(values)
  nota minimum(values)
}";
    let mut host = crate::mir::BufferHost::default();
    crate::mir::run_source(
        &Session::new(la_config().with_target(Target::HirTypeScript)),
        "reducta-arg-all-nan.fab",
        source,
        &mut host,
    )
    .expect("all-NaN arg-extrema should execute");
    assert_eq!(host.stdout_lines, ["0", "0"]);
}

#[test]
fn reducta_two_key_group_sums_into_a_rank_two_bucket_tensor() {
    let source = r"functio buckets(tensor<f64, [5]> xs, tensor<numerus, [5]> rows, tensor<numerus, [5]> cols) → tensor<f64, [2, 3]> {
  redde reducta via summa ex xs apud [i] pro [rows[i], cols[i]] sit v { redde v }
}
functio counts(tensor<f64, [5]> xs, tensor<numerus, [5]> rows, tensor<numerus, [5]> cols) → tensor<numerus, [2, 3]> {
  redde reducta via numeratio ex xs apud [i] pro [rows[i], cols[i]] sit v { redde v ≻ 0.0 }
}
incipit {
  sit lista<f64> xs_data ← [1.0, 2.0, 4.0, 8.0, 16.0]
  sit tensor<f64, [5]> xs ← xs_data ↦ tensor<f64, [5]>
  sit lista<numerus> rows_data ← [0, 1, 0, 1, 1]
  sit tensor<numerus, [5]> rows ← rows_data ↦ tensor<numerus, [5]>
  sit lista<numerus> cols_data ← [2, 0, 2, 1, 0]
  sit tensor<numerus, [5]> cols ← cols_data ↦ tensor<numerus, [5]>
  sit tensor<f64, [2, 3]> grid ← buckets(xs, rows, cols)
  nota grid
  sit tensor<numerus, [2, 3]> tally ← counts(xs, rows, cols)
  nota tally
}";
    let mut host = crate::mir::BufferHost::default();
    crate::mir::run_source(
        &Session::new(la_config().with_target(Target::HirTypeScript)),
        "reducta-two-key-group.fab",
        source,
        &mut host,
    )
    .expect("two-key grouped reduction should execute");
    assert_eq!(host.stdout_lines, ["[0.0, 0.0, 5.0, 18.0, 8.0, 0.0]", "[0, 0, 2, 2, 1, 0]"]);
}

#[test]
fn axis_reduction_on_a_rank_three_tensor_sums_and_means_each_axis() {
    // v[i, j, k] = 1 + 12 i + 4 j + k over [2, 3, 4].
    let source = r"functio over_first(tensor<f64, [2, 3, 4]> g) → tensor<f64, [3, 4]> { redde g.summa(axis 0) }
functio over_second(tensor<f64, [2, 3, 4]> g) → tensor<f64, [2, 4]> { redde g.summa(axis 1) }
functio over_third(tensor<f64, [2, 3, 4]> g) → tensor<f64, [2, 3]> { redde g.summa(axis 2) }
functio mean_second(tensor<f64, [2, 3, 4]> g) → tensor<f64, [2, 4]> { redde g.media(axis 1) }
incipit {
  sit lista<f64> data ← [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0, 17.0, 18.0, 19.0, 20.0, 21.0, 22.0, 23.0, 24.0]
  sit tensor<f64, [2, 3, 4]> g ← data ↦ tensor<f64, [2, 3, 4]>
  nota over_first(g)
  nota over_second(g)
  nota over_third(g)
  nota mean_second(g)
}";
    let mut host = crate::mir::BufferHost::default();
    crate::mir::run_source(
        &Session::new(la_config().with_target(Target::HirTypeScript)),
        "axis-reduction-rank-three.fab",
        source,
        &mut host,
    )
    .expect("rank-three axis reductions should execute");
    assert_eq!(
        host.stdout_lines,
        [
            "[14.0, 16.0, 18.0, 20.0, 22.0, 24.0, 26.0, 28.0, 30.0, 32.0, 34.0, 36.0]",
            "[15.0, 18.0, 21.0, 24.0, 51.0, 54.0, 57.0, 60.0]",
            "[10.0, 26.0, 42.0, 58.0, 74.0, 90.0]",
            "[5.0, 6.0, 7.0, 8.0, 17.0, 18.0, 19.0, 20.0]",
        ]
    );
}

#[test]
fn reducta_grouping_preserves_source_and_key_tensors() {
    let source = r"functio count(tensor<bivalens, [3]> values, tensor<numerus, [3]> keys) → tensor<numerus, [2]> {
  redde reducta via numeratio ex values apud [i] pro [keys[i]] sit v { redde v }
}
incipit {
  sit lista<bivalens> values_data ← [verum, falsum, verum]
  sit tensor<bivalens, [3]> values ← values_data ↦ tensor<bivalens, [3]>
  sit lista<numerus> keys_data ← [0, 1, 0]
  sit tensor<numerus, [3]> keys ← keys_data ↦ tensor<numerus, [3]>
  nota count(values, keys)
  nota values[0]
  nota keys[0]
}";
    let mut host = crate::mir::BufferHost::default();
    crate::mir::run_source(
        &Session::new(la_config().with_target(Target::HirTypeScript)),
        "reducta-group-immutable.fab",
        source,
        &mut host,
    )
    .expect("grouped reduction should execute");
    assert_eq!(host.stdout_lines, ["[2, 0]", "verum", "0"]);
}

#[test]
fn reducta_group_out_of_range_uses_existing_tensor_write_failure() {
    let source = r"functio count(tensor<bivalens, [3]> values, tensor<numerus, [3]> keys) → tensor<numerus, [2]> {
  redde reducta via numeratio ex values apud [i] pro [keys[i]] sit v { redde v }
}
incipit {
  sit lista<bivalens> values_data ← [verum, falsum, verum]
  sit tensor<bivalens, [3]> values ← values_data ↦ tensor<bivalens, [3]>
  sit lista<numerus> keys_data ← [0, 1, 2]
  sit tensor<numerus, [3]> keys ← keys_data ↦ tensor<numerus, [3]>
  nota count(values, keys)
}";
    let mut host = crate::mir::BufferHost::default();
    let errors = crate::mir::run_source(
        &Session::new(la_config().with_target(Target::HirTypeScript)),
        "reducta-group-out-of-range.fab",
        source,
        &mut host,
    )
    .expect_err("an out-of-range bucket must use the existing tensor write failure");
    let crate::mir::RunSourceError::Stepper(errors) = errors else {
        panic!("out-of-range grouped key should reach the runner: {errors:?}");
    };
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].issue, "runner_tensor_ponde_invalid_index");
}

#[test]
fn reducta_two_key_group_out_of_range_key_never_aliases_a_bucket() {
    // Column 3 of row 0 would be flat bucket 3 (row 1, column 0) under a bare
    // flat-index compare; each key is checked against its own coordinate.
    let source = r"functio buckets(tensor<f64, [2]> xs, tensor<numerus, [2]> rows, tensor<numerus, [2]> cols) → tensor<f64, [2, 3]> {
  redde reducta via summa ex xs apud [i] pro [rows[i], cols[i]] sit v { redde v }
}
incipit {
  sit lista<f64> xs_data ← [1.0, 2.0]
  sit tensor<f64, [2]> xs ← xs_data ↦ tensor<f64, [2]>
  sit lista<numerus> rows_data ← [0, 1]
  sit tensor<numerus, [2]> rows ← rows_data ↦ tensor<numerus, [2]>
  sit lista<numerus> cols_data ← [3, 0]
  sit tensor<numerus, [2]> cols ← cols_data ↦ tensor<numerus, [2]>
  nota buckets(xs, rows, cols)
}";
    let mut host = crate::mir::BufferHost::default();
    let errors = crate::mir::run_source(
        &Session::new(la_config().with_target(Target::HirTypeScript)),
        "reducta-two-key-out-of-range.fab",
        source,
        &mut host,
    )
    .expect_err("an out-of-range coordinate must use the existing tensor write failure");
    let crate::mir::RunSourceError::Stepper(errors) = errors else {
        panic!("out-of-range grouped key should reach the runner: {errors:?}");
    };
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].issue, "runner_tensor_ponde_invalid_index");
}

#[test]
fn cumulata_does_not_overwrite_its_initial_value() {
    // The scan rewrites its accumulator at each step; its initial value is a
    // caller binding and must read the same after the call.
    let source = r"functio main2() → vacuum {
  sit lista<numerus> base ← [1]
  sit lista<lista<numerus>> scanned ← [3, 4].cumulata((lista<numerus> acc, numerus x) ∴ acc.addita(x), base)
  nota scanned.longitudo()
  nota base.longitudo()
}

incipit {
  main2()
}";
    let mut host = crate::mir::BufferHost::default();
    crate::mir::run_source(
        &Session::new(la_config().with_target(Target::HirTypeScript)),
        "cumulata-initial.fab",
        source,
        &mut host,
    )
    .unwrap_or_else(|error| panic!("cumulata initial run must execute: {error:?}"));
    assert_eq!(host.stdout_lines, ["2", "1"]);
}

#[test]
fn lowers_lista_cumulata_with_synthetic_reducer() {
    let dump = dump_source(
        r"functio prefixes(lista<numerus> xs) → lista<numerus> {
  redde xs.cumulata((numerus acc, numerus x) ∴ acc + x, 0)
}",
    );

    assert!(dump.contains("runtime collection length"));
    assert!(dump.contains("runtime collection append"));
    assert!(dump.contains("function f1"));
    assert!(dump.contains("call f1"));
    assert!(!dump.contains("method call before runtime/provider MIR lowering"));
    assert!(!dump.contains("path that does not resolve to a local value"));
}

#[test]
fn collection_callback_errors_do_not_publish_synthetic_functions() {
    use crate::hir::{DefId, HirExpression, HirExpressionKind, HirId, HirParam, HirParamMode};
    use crate::lexer::{Span, Symbol};
    use crate::semantic::{Primitive, TypeTable};

    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let list = types.array(numerus);
    let span = Span::default();
    let mut builder = FunctionBuilder::new(&types);
    builder.add_param(DefId(0), Symbol(0), list, crate::hir::HirParamMode::Owned, span);

    let receiver = HirExpression { id: HirId(0), kind: HirExpressionKind::Path(DefId(0)), ty: Some(list), span };
    let callback_body = unsupported_range_expr(numerus, span, 1);
    let closure = HirExpression {
        id: HirId(4),
        kind: HirExpressionKind::Clausura(
            vec![HirParam {
                def_id: DefId(1),
                name: Symbol(1),
                ty: numerus,
                mode: HirParamMode::Owned,
                optional: false,
                sponte: false,
                default: None,
                span,
            }],
            Some(numerus),
            None,
            Box::new(callback_body),
        ),
        ty: None,
        span,
    };
    let expr = HirExpression { id: HirId(5), kind: HirExpressionKind::Vacua, ty: Some(list), span };

    assert!(builder.lower_mappata(&receiver, &closure, &expr).is_none());
    assert_eq!(builder.take_synthetic_functions().len(), 0);
    assert_eq!(builder.errors.len(), 1);
    assert_eq!(
        builder.errors[0].issue,
        "unsupported_mir_lowering_interval_clamp_recovery_before_interval_mir_lowering"
    );
}

#[test]
fn rejects_assignment_targets_that_are_not_places() {
    use crate::hir::{DefId, HirExpression, HirExpressionKind, HirId, HirLiteral};
    use crate::lexer::Symbol;
    use crate::semantic::{Primitive, TypeTable};

    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let span = crate::lexer::Span::default();
    let mut builder = FunctionBuilder::new(&types);
    builder.add_param(DefId(0), Symbol(0), numerus, crate::hir::HirParamMode::Owned, span);
    let entry = builder.fresh_block(span);
    builder.switch_to(entry);

    let path = HirExpression { id: HirId(0), kind: HirExpressionKind::Path(DefId(0)), ty: Some(numerus), span };
    let one =
        HirExpression { id: HirId(1), kind: HirExpressionKind::Literal(HirLiteral::Int(1)), ty: Some(numerus), span };
    let lhs = HirExpression {
        id: HirId(2),
        kind: HirExpressionKind::Binary(crate::hir::HirBinOp::Add, Box::new(path), Box::new(one)),
        ty: Some(numerus),
        span,
    };
    let rhs =
        HirExpression { id: HirId(3), kind: HirExpressionKind::Literal(HirLiteral::Int(2)), ty: Some(numerus), span };
    let assignment = HirExpression {
        id: HirId(4),
        kind: HirExpressionKind::Assign(Box::new(lhs), Box::new(rhs)),
        ty: Some(numerus),
        span,
    };

    assert!(builder.lower_assignment_expr(&assignment).is_none());
    assert!(builder.blocks[0].statements.is_empty());
    assert_eq!(builder.errors.len(), 1);
    assert_eq!(
        builder.errors[0].issue,
        "unsupported_mir_lowering_assignment_target_that_is_not_an_addressable_place"
    );
}

#[test]
fn function_builder_hir_visitor_lowers_statement_blocks() {
    use crate::hir::{HirBlock, HirExpression, HirExpressionKind, HirId, HirLiteral, HirStatement, HirStatementKind};
    use crate::semantic::{Primitive, TypeTable};

    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let span = crate::lexer::Span::default();
    let mut builder = FunctionBuilder::new(&types);
    let entry = builder.fresh_block(span);
    builder.switch_to(entry);

    let block = HirBlock {
        statements: vec![HirStatement {
            id: HirId(0),
            kind: HirStatementKind::Redde(Some(HirExpression {
                id: HirId(1),
                kind: HirExpressionKind::Literal(HirLiteral::Int(1)),
                ty: Some(numerus),
                span,
            })),
            span,
        }],
        expr: None,
        span,
        ..Default::default()
    };

    <FunctionBuilder as HirVisitor>::visit_block(&mut builder, &block);

    assert!(builder.errors.is_empty());
    assert_eq!(builder.temps.len(), 1);
    assert_eq!(builder.blocks[0].statements.len(), 1);
    assert!(matches!(
        builder.blocks[0]
            .terminator
            .as_ref()
            .map(|terminator| &terminator.kind),
        Some(MirTerminatorKind::Return(Some(MirOperand::Temp(MirTempId(0)))))
    ));
}

#[test]
fn function_builder_expr_visitor_emits_value_mir() {
    use crate::hir::{HirBinOp, HirExpression, HirExpressionKind, HirId, HirLiteral};
    use crate::mir::MirPlaceBase;
    use crate::semantic::{Primitive, TypeTable};

    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let span = crate::lexer::Span::default();
    let mut builder = FunctionBuilder::new(&types);
    let entry = builder.fresh_block(span);
    builder.switch_to(entry);

    let lhs =
        HirExpression { id: HirId(0), kind: HirExpressionKind::Literal(HirLiteral::Int(1)), ty: Some(numerus), span };
    let rhs =
        HirExpression { id: HirId(1), kind: HirExpressionKind::Literal(HirLiteral::Int(2)), ty: Some(numerus), span };
    let expr = HirExpression {
        id: HirId(2),
        kind: HirExpressionKind::Binary(HirBinOp::Add, Box::new(lhs), Box::new(rhs)),
        ty: Some(numerus),
        span,
    };

    let operand = <FunctionBuilder as HirExpressionLoweringVisitor>::lower_expr_value(&mut builder, &expr);

    assert_eq!(operand, Some(MirOperand::Temp(MirTempId(0))));
    assert!(builder.errors.is_empty());
    assert_eq!(builder.temps.len(), 1);
    assert!(matches!(
        builder.blocks[0].statements.first().map(|stmt| &stmt.kind),
        Some(MirStatementKind::Assign { place: MirPlace { base: MirPlaceBase::Temp(MirTempId(0)), .. }, value })
            if matches!(value.kind, MirValueKind::Binary { op: MirBinOp::Add, .. })
    ));
}

#[test]
fn lowers_fac_cape_handler_nuntius_projection() {
    let dump = dump_source(
        r#"
incipit {
    fac {
        iace "simulated failure"
    } cape err {
        nota "Caught error: §"(err.nuntius)
    }
}
"#,
    );
    assert!(dump.contains("_0 = %0: ty#0\n    goto bb1"));
    assert!(dump.contains("runtime diagnostic nota"));
    assert!(!dump.contains("field sym#"));
}

#[test]
fn fixum_deferred_local_declares_without_zero_init() {
    let dump = dump_source(
        r"
functio scale(bivalens compact, numerus base) → numerus {
    sit numerus factor
    si compact { factor ← 10 } secus { factor ← 100 }
    redde base * factor
}
",
    );
    assert!(dump.contains("let _"));
    assert!(!dump.contains("_0 = const int 0"));
}

#[test]
fn lowers_sponte_struct_field_with_type_zero() {
    let source = r#"
genus User {
    varia textus name
    varia textus email sponte
}
incipit {
    sit User u ← User { name = "Ada" }
    nota u.name
}
"#;
    let mut unit = analyze(source);
    let name_field = unit.interner.intern("name");
    let email_field = unit.interner.intern("email");
    let ada = unit.interner.intern("Ada");
    let program = lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default())
        .map(|lowered| lowered.validated.into_parts().0)
        .expect("MIR lowering succeeds");

    let fields = program
        .functions
        .iter()
        .flat_map(|function| &function.blocks)
        .flat_map(|block| &block.statements)
        .find_map(|statement| match &statement.kind {
            MirStatementKind::Construct { aggregate, .. } if matches!(aggregate.kind, MirAggregateKind::Struct(_)) => {
                match &aggregate.fields {
                    MirAggregateFields::Named(named) => Some(named.as_slice()),
                    _ => None,
                }
            }
            _ => None,
        })
        .expect("struct literal lowers to construct struct");

    assert_eq!(fields.len(), 2, "expected explicit name plus sponte email fill");

    let name = fields
        .iter()
        .find(|field| field.name == name_field)
        .expect("name field present in struct construct");
    let email = fields
        .iter()
        .find(|field| field.name == email_field)
        .expect("email sponte field present in struct construct");

    assert_eq!(
        name.value,
        MirOperand::Constant(MirConstant::String(ada)),
        "provided name literal must lower unchanged"
    );
    assert_eq!(
        email.value,
        MirOperand::Constant(MirConstant::Nil),
        "omitted sponte email must receive nihil"
    );
}

#[test]
fn lowers_multi_arm_nullable_sponte_struct_field_to_nil_zero() {
    let source = r#"
genus User {
    varia textus name
    varia textus ∪ numerus ∪ nihil tag sponte
}
incipit {
    sit User u ← User { name = "Ada" }
    nota u.name
}
"#;
    let mut unit = analyze(source);
    let tag_field = unit.interner.intern("tag");
    let program = lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default())
        .map(|lowered| lowered.validated.into_parts().0)
        .expect("MIR lowering succeeds");

    let fields = program
        .functions
        .iter()
        .flat_map(|function| &function.blocks)
        .flat_map(|block| &block.statements)
        .find_map(|statement| match &statement.kind {
            MirStatementKind::Construct { aggregate, .. } if matches!(aggregate.kind, MirAggregateKind::Struct(_)) => {
                match &aggregate.fields {
                    MirAggregateFields::Named(named) => Some(named.as_slice()),
                    _ => None,
                }
            }
            _ => None,
        })
        .expect("struct literal lowers to construct struct");

    let tag = fields
        .iter()
        .find(|field| field.name == tag_field)
        .expect("tag sponte field present in struct construct");

    assert_eq!(
        tag.value,
        MirOperand::Constant(MirConstant::Nil),
        "multi-arm unions that include nihil keep the MIR zero-init policy"
    );
}

#[test]
fn lowers_optional_prefix_before_defaulted_param() {
    let dump = dump_source(
        r"
functio f(textus a sponte, numerus b sponte vel 1) → numerus {
    redde b
}
incipit {
    nota f()
}
",
    );
    assert!(dump.contains("const nil"));
    assert!(dump.contains("const int 1"));
}

#[test]
// ===========================================================================
// Section J: Closures, callable values, captured environments
// ===========================================================================

fn lowers_callable_closure_values_and_indirect_calls() {
    let dump = dump_source(
        r"incipit {
  sit _ duplica ← numerus x ∴ x * 2
  nota duplica(5)
}",
    );
    assert!(dump.contains("const function"));
    assert!(dump.contains("%0 = const function f1"), "{dump}");
    assert!(dump.contains("_0 = %0"), "{dump}");
    assert!(dump.contains("call _0(const int 5)"));
    assert!(!dump.contains("closures before callable-value MIR lowering"));
    assert!(!dump.contains("indirect calls before callable-value MIR lowering"));
}

#[test]
fn lowers_captured_closure_environment_layout() {
    let dump = dump_source(
        r"incipit {
  sit _ base ← 10
  sit _ adde ← numerus x ∴ x + base
  nota adde(5)
}",
    );
    assert!(dump.contains("construct closure_env env"), "{dump}");
    assert!(dump.contains("closure f"), "{dump}");
    assert!(dump.contains(".env"), "{dump}");
    assert!(dump.contains("call closure f"), "{dump}");
    assert!(dump.contains("env env1 via %0(const int 5)"), "{dump}");
    assert!(!dump.contains("path that does not resolve to a local value"), "{dump}");
}

#[test]
fn lowers_failable_captured_closure_call_to_try_call() {
    let dump = dump_source(
        r"incipit {
  sit _ base ← 10
  sit _ adde ← (numerus x) → numerus ⇥ textus ∴ x + base
  fac {
    sit _ value ← adde(5)
    nota value
  }
  cape err {
    nota err
  }
}",
    );
    assert!(dump.contains("call closure f"), "{dump}");
    assert!(dump.contains("try_call closure f"), "{dump}");
    assert!(dump.contains("error"), "{dump}");
}

#[test]
fn validates_captured_closure_callee_environment_function_match() {
    let unit = analyze(
        r"incipit {
  sit _ base ← 10
  sit _ adde ← numerus x ∴ x + base
  nota adde(5)
}",
    );
    let lowered =
        lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default()).expect("MIR lowering succeeds");
    let mut corrupted_context = lowered.validated.validation().clone();
    let environment_id = corrupted_context
        .closure_environments
        .keys()
        .next()
        .copied()
        .expect("captured closure environment recorded");
    corrupted_context
        .closure_environments
        .get_mut(&environment_id)
        .expect("captured closure environment metadata")
        .function = MirFunctionId(999);

    let errors =
        validate_program(lowered.validated.program(), &corrupted_context).expect_err("corrupt closure metadata fails");
    assert!(
        errors
            .iter()
            .any(|error| error.issue == "mir_validation_closure_callee_function_does_not_match_environment_metadata"),
        "{errors:#?}"
    );
}

#[test]
fn lowers_nested_captured_closure_environment_layout() {
    let dump = dump_source(
        r"incipit {
  sit _ base ← 10
  sit _ maker ← numerus y ∴ numerus z ∴ z + y + base
  sit _ adde ← maker(2)
  nota adde(5)
}",
    );
    assert!(dump.matches("construct closure_env env").count() >= 2, "{dump}");
    assert!(dump.contains(".env"), "{dump}");
    assert!(dump.contains("call closure f"), "{dump}");
    assert!(dump.contains("env env1 via %0(const int 2)"), "{dump}");
    assert!(dump.contains("call _2(const int 5)"), "{dump}");
    assert!(!dump.contains("path that does not resolve to a local value"), "{dump}");
}

#[test]
fn rejects_legacy_typed_ad_statement_for_mir() {
    let result = crate::driver::compile(
        &crate::Session::new(crate::Config::default().with_latin_locale()),
        "legacy-ad.fab",
        r#"incipit {
  ad "fasciculus:lege" ("salve.txt") → textus textus ⇥ textus {
    nota textus
  }
}"#,
        ModuleCompileRequest::default(),
    );
    assert!(
        !result.success(),
        "legacy typed ad statement should fail parse: {:?}",
        result.diagnostics
    );
}

#[test]
fn lowers_cede_yield_in_cursor_function() {
    let dump = dump_source(
        r"functio stream() fiunt → numerus {
  cede 1
  cede 2
}
incipit {
  nota stream()
}",
    );
    assert!(dump.contains("runtime cede"));
    assert!(!dump.contains("cede before async MIR lowering"));
}

#[test]
fn lowers_function_typed_genus_field_call_as_value_callee() {
    let dump = dump_source(
        r"genus H {
  varia (numerus) → numerus f
}
incipit {
  sit _ h ← H { f = numerus x ∴ x + 100 }
  nota h.f(1)
  sit _ g ← h.f
  nota g(2)
}",
    );
    assert!(
        !dump.contains("method call before runtime/provider MIR lowering"),
        "a function-typed genus field call lowers as a value call:\n{dump}"
    );
    assert!(dump.contains("const int 1"), "both call arguments lower:\n{dump}");
    assert!(dump.contains("const int 2"), "both call arguments lower:\n{dump}");
    assert!(dump.contains("call "), "the value call is emitted:\n{dump}");
}

#[test]
fn arrowless_do_closure_head_lowers_without_return_clause() {
    let dump = dump_source(
        r"incipit {
  sit (numerus) → vacuum p ← numerus n ∴ fac { nota n }
  p(5)
  varia numerus total ← 0
  sit (numerus) → vacuum adde ← numerus n ∴ fac { total ← total + n }
  adde(21)
  nota total
}",
    );
    assert!(
        dump.contains("const function f"),
        "the capture-free closure constant lowers:\n{dump}"
    );
    assert!(
        dump.contains("construct closure_env env"),
        "the capturing closure lowers:\n{dump}"
    );
    assert!(dump.contains("call closure f"), "the capturing closure call lowers:\n{dump}");
    assert!(dump.contains("const int 21"), "the argument literal lowers:\n{dump}");
}

#[test]
// ===========================================================================
// Section K: Generic monomorphization & explicit type args
// ===========================================================================

fn generic_call_with_explicit_type_args_lowers_and_validates() {
    let source = r#"
functio identitas<T>(T x) → T {
    redde x
}

incipit {
    nota identitas<textus>("creata")
}
"#;
    let unit = analyze(source);
    let lowered =
        lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default()).expect("MIR lowering succeeds");
    validate_program(lowered.validated.program(), lowered.validated.validation())
        .expect("generic call-site MIR validates");
}

#[test]
fn generic_tensor_limes_resolves_builtin_and_custom_policies_in_mir_instances() {
    let source = include_str!("../../../../corpus/tensor/edge-policy-forwarding.fab");
    let unit = analyze(source);
    let lowered = lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default())
        .expect("generic tensor edge policy calls lower to concrete MIR dispatches");

    let limes = lowered
        .validated
        .program()
        .functions
        .iter()
        .flat_map(|function| function.blocks.iter())
        .flat_map(|block| block.statements.iter())
        .filter_map(|statement| match &statement.kind {
            MirStatementKind::RuntimeCall { call, .. } => match &call.intrinsic {
                MirIntrinsic::TensorLimes(policy) => Some(policy),
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(limes.len(), 2, "both generic call sites must have an instantiated limes call");
    assert!(
        limes
            .iter()
            .any(|policy| matches!(policy, MirEdgePolicy::Builtin(MirBuiltinEdgePolicy::Clamp))),
        "the builtin policy must resolve to its tagged MIR dispatch"
    );
    let custom = limes
        .iter()
        .find_map(|policy| match policy {
            MirEdgePolicy::Custom { policy_def, remap, transform } => Some((*policy_def, *remap, *transform)),
            MirEdgePolicy::Builtin(_) => None,
        })
        .expect("the custom policy must resolve to callback MIDs");
    assert!(
        lowered
            .validated
            .program()
            .functions
            .iter()
            .any(|function| function.id == custom.1),
        "the custom remap MID must resolve in the final program"
    );
    let Some(transform) = custom.2 else {
        panic!("the fixture's custom policy has a value transform");
    };
    assert!(
        lowered
            .validated
            .program()
            .functions
            .iter()
            .any(|function| function.id == transform),
        "the custom transform MID must resolve in the final program"
    );
}

#[test]
fn generic_enum_payload_binding_lowers_with_concrete_type() {
    let source = r#"
discretio Resultatus<T> {
    Ok { T valor },
    Err { textus nuntius }
}

functio describe(Resultatus<numerus> r) → textus {
    discerne r {
        casu Ok sit valor { redde "ok: §"(valor) }
        casu Err sit nuntius { redde "err: §"(nuntius) }
    }
}
"#;
    let unit = analyze(source);
    let lowered = lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default())
        .expect("generic enum payload MIR lowers");
    validate_program(lowered.validated.program(), lowered.validated.validation())
        .expect("generic enum payload MIR validates");
}

/// A generic `rope_norm<magnitudo d>(pos)` template lowers its compile-time dimension to
/// the existing symbolic shape-size MIR constant, so the non-AIR MIR
/// monomorphizer can bind it later. The path must never resolve to a runtime
/// local.
#[test]
fn generic_rope_dim_lowers_to_symbolic_shape_size_operand() {
    let source = r"
functio rope_q<T, magnitudo D, magnitudo d, magnitudo P>(tensor<T, [D, d]> input, tensor<T, [P, d, 3]> rota, varia tensor<T, [D, d]> output) → vacuum {
    output ⇇ input.rope_norm<magnitudo d>(0, rota)
}
";
    let unit = analyze(source);
    let lowered = lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default())
        .expect("generic rope template must lower, not reject the size-param path");

    assert_eq!(lowered.generic_templates.len(), 1, "rope_q stays one generic template");
    let template = &lowered.generic_templates[0];
    let rope_calls = template
        .blocks
        .iter()
        .flat_map(|block| block.statements.iter())
        .filter_map(|statement| match &statement.kind {
            MirStatementKind::RuntimeCall { call, .. } => Some(call),
            _ => None,
        })
        .filter(|call| matches!(call.intrinsic, MirIntrinsic::Collection(MirCollectionOp::TensorRope)))
        .collect::<Vec<_>>();
    assert_eq!(rope_calls.len(), 1, "one tensor-rope call in the template");
    let call = rope_calls[0];
    assert!(
        matches!(call.args.get(1), Some(MirOperand::Constant(MirConstant::Int(0)))),
        "rope position stays a literal: {:?}",
        call.args
    );
    let Some(MirOperand::Constant(MirConstant::ShapeSize { shape, axis: None })) = call.args.get(2) else {
        panic!("rope dim must carry the symbolic size operand: {:?}", call.args);
    };
    match unit.types.get_index(*shape) {
        IndexExpr::Param(symbol) => assert_eq!(
            unit.interner.resolve(*symbol),
            "d",
            "the operand must reference the size parameter's own interned node"
        ),
        other => panic!("shape node must be the d parameter, got {other:?}"),
    }
}

#[test]
// Sparse tensor operations (interleaved with generic monomorphization above).
fn sparsa_nonnihil_lowers_to_distinct_runtime_operation() {
    let dump = dump_source(
        r"incipit {
    sit tf32[2, 2] dense ← [[1.0, 0.0], [0.0, 2.0]] ↦ tf32[2, 2]
    sit sf32[2, 2] sparse ← dense ↦ sf32[2, 2]
    nota sparse.nonnihil()
}",
    );
    assert!(dump.contains("runtime collection sparse_nonzero_count("), "{dump}");
}

#[test]
fn sparse_representation_vacua_access_and_densify_lower_distinctly() {
    let dump = dump_source(
        r"incipit {
    varia sf32[2, 3] sparse ← vacua
    sparse.ponde([0, 1], 4.0)
    sit _ value ← sparse.accipe([0, 1])
    sit tf32[2, 3] dense ← sparse.densata()
    nota value, dense.longitudo()
}",
    );
    assert!(dump.contains("construct sparse_tensor:"), "{dump}");
    assert!(dump.contains("runtime collection sparse_set("), "{dump}");
    assert!(dump.contains("runtime collection sparse_get("), "{dump}");
    assert!(dump.contains("runtime collection sparse_densify("), "{dump}");
    assert!(!dump.contains("construct tensor:"), "{dump}");
}

#[test]
fn sparsa_shape_lowers_to_tensor_shape_and_validates() {
    let source = r"incipit {
    sit sf32[2, 3] sparse ← vacua
    sit _ shape ← sparse.magnitudines()
    nota shape
}";
    let dump = dump_source(source);
    assert!(dump.contains("runtime collection tensor_shape("), "{dump}");

    let unit = analyze(source);
    let lowered =
        lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default()).expect("sparsa shape MIR lowers");
    validate_program(lowered.validated.program(), lowered.validated.validation()).expect("sparsa shape MIR validates");
}

#[test]
fn stage3_mir_closeout_preserves_interval_and_sermo_operations() {
    let interval = dump_source(
        r"incipit {
    sit intervallum<numerus> left ← 0‥6
    sit intervallum<numerus> right ← 4…10
    sit _ overlap ← left.inter(right)
    sit _ values ← left ↦ lista<numerus>
    nota overlap, values
}",
    );
    assert!(interval.contains("construct interval_exclusive:"), "{interval}");
    assert!(interval.contains("runtime collection interval_intersect("), "{interval}");
    assert!(interval.contains("runtime collection interval_materialize("), "{interval}");

    let sermo = dump_source(
        r#"incipit {
    sit sermo stream ← ad 'runtime:echo' ("salve")
    itera ex stream.tuus<textus>().cursor() sit frame {
        adfirma frame.data ≡ "salve"
    }
}"#,
    );
    assert!(sermo.contains("runtime sermo_view_tuus("), "{sermo}");
    assert!(sermo.contains("runtime sermo_cursor("), "{sermo}");
    assert!(sermo.contains("runtime sermo_drain_cursor("), "{sermo}");
}

#[test]
fn mir_generic_call_with_composite_explicit_type_args_lowers_and_validates() {
    let source = r#"
functio identitas_lista<T>(lista<T> xs) → lista<T> {
    redde xs
}

incipit {
    sit lista<textus> xs ← ["creata"]
    sit lista<textus> ys ← identitas_lista<textus>(xs)
    nota ys
}
"#;
    let unit = analyze(source);
    let lowered =
        lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default()).expect("MIR lowering succeeds");
    validate_program(lowered.validated.program(), lowered.validated.validation())
        .expect("composite generic call-site MIR validates");
}

#[test]
fn mir_generic_call_with_nullable_return_lowers_and_validates() {
    let source = r#"
functio fortasse<T>(T x) → T ∪ nihil {
    redde x
}

incipit {
    sit textus ∪ nihil value ← fortasse<textus>("creata")
    nota value
}
"#;
    let unit = analyze(source);
    let lowered =
        lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default()).expect("MIR lowering succeeds");
    validate_program(lowered.validated.program(), lowered.validated.validation())
        .expect("nullable generic return MIR validates");
}

#[test]
fn collection_length_keeps_numerus_call_result_when_context_is_nullable() {
    let source = r"
functio count(lista<fractus> values) → numerus ∪ nihil {
    redde values.longitudo()
}
";
    let unit = analyze(source);
    let numerus = unit.types.primitive(Primitive::Numerus);
    let lowered = lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default())
        .expect("nullable collection length lowers");
    validate_program(lowered.validated.program(), lowered.validated.validation())
        .expect("nullable collection length validates");

    let function = lowered
        .validated
        .program()
        .functions
        .iter()
        .find(|function| function.source.is_some())
        .expect("count function is lowered");
    let length_return_ty = function
        .blocks
        .iter()
        .flat_map(|block| &block.statements)
        .find_map(|statement| {
            let MirStatementKind::RuntimeCall { call, .. } = &statement.kind else {
                return None;
            };
            matches!(call.intrinsic, MirIntrinsic::Collection(MirCollectionOp::Length)).then_some(call.return_ty)
        });
    assert_eq!(length_return_ty, Some(MirType::semantic(numerus)));

    let return_value = function.blocks.iter().find_map(|block| {
        let MirTerminatorKind::Return(Some(value)) = &block.terminator.kind else {
            return None;
        };
        Some(value)
    });
    let Some(MirOperand::Temp(temp)) = return_value else {
        panic!("nullable return wraps the collection length in an option temp");
    };
    let returned_ty = function
        .temps
        .iter()
        .find(|candidate| candidate.id == *temp)
        .map(|candidate| candidate.ty);
    assert_eq!(returned_ty, Some(function.return_ty));
}

#[test]
fn narrowed_nullable_collection_projection_emits_valid_mir() {
    let source = r"
functio first(lista<numerus> ∪ nihil values) → numerus {
    si values non est nihil {
        redde values[0]
    }
    redde 0
}
";
    let unit = analyze(source);
    let lowered = lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default())
        .expect("narrowed collection lowers");
    validate_program(lowered.validated.program(), lowered.validated.validation())
        .expect("narrowed collection projection validates");
}

#[test]
fn mir_generic_explicit_type_arg_for_nil_lowers_and_validates() {
    let source = r"
functio consume<T>(T ∪ nihil x) → vacuum {}

incipit {
    consume<textus>(nulla)
}
";
    let unit = analyze(source);
    let lowered =
        lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default()).expect("MIR lowering succeeds");
    validate_program(lowered.validated.program(), lowered.validated.validation())
        .expect("explicit nil generic call validates");
}

#[test]
fn mir_generic_call_with_projected_struct_field_lowers_and_validates() {
    let source = r#"
genus Capsa {
    varia textus nomen
}

functio identitas<T>(T x) → T {
    redde x
}

incipit {
    sit Capsa capsa ← Capsa { nomen = "creata" }
    sit textus nomen ← identitas(capsa.nomen)
    nota nomen
}
"#;
    let unit = analyze(source);
    let lowered =
        lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default()).expect("MIR lowering succeeds");
    validate_program(lowered.validated.program(), lowered.validated.validation())
        .expect("projected struct field generic call validates");
}

#[test]
fn mir_generic_call_with_indexed_list_element_lowers_and_validates() {
    let source = r#"
functio identitas<T>(T x) → T {
    redde x
}

incipit {
    sit lista<textus> verba ← ["creata"]
    sit textus verbum ← identitas(verba![0])
    nota verbum
}
"#;
    let unit = analyze(source);
    let lowered =
        lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default()).expect("MIR lowering succeeds");
    validate_program(lowered.validated.program(), lowered.validated.validation())
        .expect("indexed list element generic call validates");
}

#[test]
fn mir_generic_monomorphization_deduplicates_and_rewrites_direct_calls() {
    let source = r#"
functio identitas<T>(T x) → T {
    redde x
}

incipit {
    sit _ a ← identitas<textus>("creata")
    sit _ b ← identitas<textus>("iterum")
    sit _ c ← identitas<numerus>(7)
    nota a
    nota b
    nota c
}
"#;
    let unit = analyze(source);
    let lowered =
        lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default()).expect("MIR lowering succeeds");

    assert_eq!(lowered.monomorphizations.len(), 2);
    let textus_instance = lowered.monomorphizations[0].function;
    let numerus_instance = lowered.monomorphizations[1].function;
    assert_ne!(textus_instance, numerus_instance);

    let direct_calls = lowered
        .validated
        .program()
        .functions
        .iter()
        .flat_map(|function| function.blocks.iter())
        .flat_map(|block| block.statements.iter())
        .filter_map(|statement| match &statement.kind {
            MirStatementKind::Call { callee: MirCallee::Function(id), .. } => Some(*id),
            MirStatementKind::Call { callee: MirCallee::Definition { .. }, .. } => {
                panic!("generic definition callee leaked after monomorphization")
            }
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(
        direct_calls
            .iter()
            .filter(|id| **id == textus_instance)
            .count(),
        2
    );
    assert_eq!(
        direct_calls
            .iter()
            .filter(|id| **id == numerus_instance)
            .count(),
        1
    );
    let types = lowered.validated.validation().types.as_table();
    assert!(
        lowered
            .validated
            .program()
            .functions
            .iter()
            .all(|function| {
                function
                    .params
                    .iter()
                    .all(|param| !type_contains_param(types, param.ty.semantic_id()))
                    && function
                        .locals
                        .iter()
                        .all(|local| !type_contains_param(types, local.ty.semantic_id()))
                    && function
                        .temps
                        .iter()
                        .all(|temp| !type_contains_param(types, temp.ty.semantic_id()))
                    && !type_contains_param(types, function.return_ty.semantic_id())
            })
    );
}

// ──────────────────────────────────────────────────────────────────────
// 4A — magnitudo shape params monomorphize at concrete call sites.
// ──────────────────────────────────────────────────────────────────────

#[test]
fn monomorphized_device_instance_carries_copied_template_metadata() {
    let unit = analyze(
        r"
@ nucleum
@ publica
functio duplum<T>(T x) → T {
    redde x
}

incipit {
    sit numerus v ← duplum<numerus>(7)
    nota v
}
",
    );
    let lowered =
        lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default()).expect("MIR lowering succeeds");

    assert_eq!(lowered.monomorphizations.len(), 1);
    let instance_id = lowered.monomorphizations[0].function;
    let instance = lowered
        .validated
        .program()
        .functions
        .iter()
        .find(|function| function.id == instance_id)
        .expect("monomorphized instance in program");
    let template = lowered
        .generic_templates
        .iter()
        .find(|template| template.source == instance.source)
        .expect("generic template survives lowering");

    let template_row = lowered
        .device_function_metadata
        .get(&template.id)
        .expect("template device metadata row still present (copy, not move)");
    let instance_row = lowered
        .device_function_metadata
        .get(&instance_id)
        .expect("instance carries the copied device metadata row");

    assert_eq!(
        instance_row.identity.parent, instance_id,
        "instance row parent is the instance id"
    );
    assert_eq!(instance_row.role, template_row.role);
    assert_eq!(instance_row.visibility, template_row.visibility);
    assert_eq!(instance_row.origin, template_row.origin);
    assert_eq!(template_row.identity.parent, template.id, "template row keeps its own parent");
}

/// Every tensor dimension of `ty` (through the type table) is a literal.
fn assert_tensor_dims_literal(types: &TypeTable, ty: MirType) {
    match types.get(ty.semantic_id()) {
        Type::Tensor(_, shape) | Type::Matrix(_, shape) | Type::Sparsa(_, shape) => {
            let IndexExpr::Tuple(dims) = types.get_index(*shape) else {
                panic!("shape is not a rank-n tuple: {shape:?}");
            };
            for dim in dims {
                assert!(
                    matches!(types.get_index(*dim), IndexExpr::Literal(_)),
                    "dimension {dim:?} is not literal after monomorphization"
                );
            }
        }
        _ => {}
    }
}

#[test]
fn mir_generic_shape_params_monomorphize_to_literal_dims() {
    let source = r"
functio add_tensors<magnitudo B, magnitudo D>(tensor<f32, [B, D]> a, tensor<f32, [B, D]> b) → tensor<f32, [B, D]> {
    redde a + b
}

incipit {
    sit tensor<f32, [2, 8]> x ← vacua
    sit tensor<f32, [2, 8]> y ← vacua
    sit tensor<f32, [2, 8]> z ← add_tensors(x, y)
    nota z
}
";
    let unit = analyze(source);
    let lowered =
        lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default()).expect("MIR lowering succeeds");

    assert_eq!(lowered.monomorphizations.len(), 1);
    let instance_id = lowered.monomorphizations[0].function;

    // The call site rewrites to the concrete instance.
    let direct_calls = lowered
        .validated
        .program()
        .functions
        .iter()
        .flat_map(|function| function.blocks.iter())
        .flat_map(|block| block.statements.iter())
        .filter_map(|statement| match &statement.kind {
            MirStatementKind::Call { callee: MirCallee::Function(id), .. } => Some(*id),
            MirStatementKind::Call { callee: MirCallee::Definition { .. }, .. } => {
                panic!("generic definition callee leaked after monomorphization")
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(direct_calls, vec![instance_id]);

    // Instance signature carries concrete shapes: no params remain, and every
    // tensor shape is literal dims.
    let instance = lowered
        .validated
        .program()
        .functions
        .iter()
        .find(|function| function.id == instance_id)
        .expect("monomorphized instance exists");
    let types = lowered.validated.validation().types.as_table();
    assert!(
        instance
            .params
            .iter()
            .all(|param| !type_contains_param(types, param.ty.semantic_id()))
    );
    assert!(!type_contains_param(types, instance.return_ty.semantic_id()));
    for param in &instance.params {
        assert_tensor_dims_literal(types, param.ty);
    }
    assert_tensor_dims_literal(types, instance.return_ty);
    // R6 / Q3: the literal instance's sizes were proven at the typed call
    // site, so it carries no runtime shape check at entry or return.
    let shape_checks = instance
        .blocks
        .iter()
        .flat_map(|block| block.statements.iter())
        .filter(|statement| {
            matches!(
                &statement.kind,
                MirStatementKind::RuntimeCall { call, .. }
                    if matches!(call.intrinsic, MirIntrinsic::TensorShapeCheck(_))
            )
        })
        .count();
    assert_eq!(shape_checks, 0, "a literal instance has no runtime shape check");
}

#[test]
fn mir_generic_shape_params_bind_explicit_natural_type_args() {
    // Size-only generic: explicit NATURAL typeArguments bind magnitudo params.
    // K is declared but absent from argument/return shapes, so D1 witnesses
    // cannot supply it — the call-site list is the only source.
    let source = r"
functio take<magnitudo M, magnitudo K, magnitudo N>(tensor<f32, [M, N]> x) → tensor<f32, [M, N]> {
    redde x
}

incipit {
    sit tensor<f32, [2, 2]> a ← vacua
    sit tensor<f32, [2, 2]> b ← take<magnitudo 2, magnitudo 2, magnitudo 2>(a)
    nota b.longitudo()
}
";
    let unit = analyze(source);
    let lowered = lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default())
        .expect("explicit NATURAL size args must lower");
    assert_eq!(lowered.monomorphizations.len(), 1);
    let shape_args = lowered.monomorphizations[0].key.shape_args.clone();
    assert_eq!(shape_args.len(), 3);
    validate_program(lowered.validated.program(), lowered.validated.validation())
        .expect("explicit NATURAL size-arg monomorphization validates");
    drop(lowered);
    for shape in shape_args {
        assert!(
            matches!(unit.types.get_index(shape), IndexExpr::Literal(_)),
            "explicit NATURAL size arg must bind a literal shape, got {shape:?}"
        );
    }
}

#[test]
fn mir_generic_shape_params_distinct_shapes_produce_distinct_instances() {
    let source = r"
functio add_tensors<magnitudo B, magnitudo D>(tensor<f32, [B, D]> a, tensor<f32, [B, D]> b) → tensor<f32, [B, D]> {
    redde a + b
}

incipit {
    sit tensor<f32, [2, 8]> x ← vacua
    sit tensor<f32, [2, 8]> y ← vacua
    sit tensor<f32, [4, 4]> p ← vacua
    sit tensor<f32, [4, 4]> q ← vacua
    sit tensor<f32, [2, 8]> z ← add_tensors(x, y)
    sit tensor<f32, [4, 4]> r ← add_tensors(p, q)
    nota z
    nota r
}
";
    let unit = analyze(source);
    let lowered =
        lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default()).expect("MIR lowering succeeds");

    assert_eq!(lowered.monomorphizations.len(), 2);
    let first = lowered.monomorphizations[0].function;
    let second = lowered.monomorphizations[1].function;
    assert_ne!(first, second, "distinct shapes must produce distinct instances");

    // Same shape again deduplicates to the existing instance (2 instances for
    // 3 call sites across two shapes is exercised by the key's shape args).
    let param_ty = lowered
        .validated
        .program()
        .functions
        .iter()
        .find(|function| function.id == first)
        .map(|function| function.params[0].ty)
        .expect("instance param");
    assert_tensor_dims_literal(&unit.types, param_ty);
}

#[test]
fn mir_generic_shape_params_same_shape_deduplicates_instances() {
    let source = r"
functio add_tensors<magnitudo B, magnitudo D>(tensor<f32, [B, D]> a, tensor<f32, [B, D]> b) → tensor<f32, [B, D]> {
    redde a + b
}

incipit {
    sit tensor<f32, [2, 8]> x ← vacua
    sit tensor<f32, [2, 8]> y ← vacua
    sit tensor<f32, [2, 8]> p ← vacua
    sit tensor<f32, [2, 8]> q ← vacua
    sit tensor<f32, [2, 8]> z ← add_tensors(x, y)
    sit tensor<f32, [2, 8]> r ← add_tensors(p, q)
    nota z
    nota r
}
";
    let unit = analyze(source);
    let lowered =
        lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default()).expect("MIR lowering succeeds");
    assert_eq!(
        lowered.monomorphizations.len(),
        1,
        "two same-shape call sites share one monomorphized instance"
    );
}

#[test]
fn runtime_shape_origins_share_generic_instance_but_keep_call_witnesses() {
    let source = r#"+++
locale = "en"
+++
fn runtime_extent(int value) → int {
    return value
}

fn require_extent<size N>(ref tf32[N] value) → int {
    return 7
}

main {
    const size short ← runtime_extent(2)
    const size long ← runtime_extent(3)
    let list<f32> short_flat ← [1.0, 2.0]
    let list<f32> long_flat ← [3.0, 4.0, 5.0]
    let tf32[short] short_value ← short_flat ↦ tf32[short]
    let tf32[long] long_value ← long_flat ↦ tf32[long]
    print require_extent(short_value)
    print require_extent(long_value)
}
"#;
    let unit = analyze(source);
    let (monomorphizations, program) = {
        let lowered = lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default())
            .expect("runtime generic shape origins lower");
        (lowered.monomorphizations.clone(), lowered.validated.program().clone())
    };

    assert_eq!(
        monomorphizations.len(),
        1,
        "runtime values must not split the generic instance key"
    );
    let instance = monomorphizations[0].function;
    let [shape_key] = monomorphizations[0].key.shape_args.as_slice() else {
        panic!("expected one stable runtime shape key");
    };
    assert!(
        matches!(
            unit.types.get_index(*shape_key),
            IndexExpr::Runtime(radix_types::RuntimeExtentId::FunctionParameterAxis { parameter: 0, axis: 0 })
        ),
        "specialization key must use the stable formal tensor axis, got {:?}",
        unit.types.get_index(*shape_key)
    );

    let mut calls_to_instance = Vec::new();
    let mut shape_checks = Vec::new();
    for (function_index, function) in program.functions.iter().enumerate() {
        for statement in function.blocks.iter().flat_map(|block| &block.statements) {
            match &statement.kind {
                MirStatementKind::Call { callee: MirCallee::Function(target), .. } if *target == instance => {
                    calls_to_instance.push(statement);
                }
                MirStatementKind::RuntimeCall { call, .. }
                    if matches!(call.intrinsic, MirIntrinsic::TensorShapeCheck(_)) =>
                {
                    shape_checks.push((function_index, call));
                }
                _ => {}
            }
        }
    }
    assert_eq!(
        calls_to_instance.len(),
        2,
        "both runtime origins should call the shared instance"
    );
    // The conversion boundary now emits its own shape checks: each origin
    // keeps three TensorShapeChecks where the from_flat costume folded
    // everything into the call — the callee size-parameter binding, the
    // conversion's extent relations, and the call boundary. Checks group by
    // kind (even index = the short origin, odd = the long origin); the
    // origin identity lives in the checked tensor carriers.
    assert_eq!(
        shape_checks.len(),
        6,
        "each origin keeps its size binding, conversion relations, and boundary check"
    );
    let checks: Vec<_> = shape_checks
        .iter()
        .map(|(_, call)| match &call.intrinsic {
            MirIntrinsic::TensorShapeCheck(check) => check,
            _ => unreachable!(),
        })
        .collect();
    for (short, long) in [(checks[0], checks[1]), (checks[2], checks[3]), (checks[4], checks[5])] {
        assert_eq!(
            short.relations.len(),
            long.relations.len(),
            "both origins check the same extent law"
        );
        for relation in short.relations.iter().chain(long.relations.iter()) {
            // The stable scalar witness guards the physical tensor extent.
            assert!(
                matches!(relation.left, crate::mir::MirShapeExpr::TensorAxis { argument: 0, axis: 0 })
                    && matches!(relation.right, crate::mir::MirShapeExpr::ScalarArgument { .. })
            );
        }
        assert_eq!(
            short.size_bindings.len(),
            long.size_bindings.len(),
            "both origins carry the same binding shape"
        );
        for binding in short.size_bindings.iter().chain(long.size_bindings.iter()) {
            assert_eq!(binding.parameter, 0);
            assert!(matches!(binding.witness, crate::mir::MirShapeExpr::ScalarArgument { .. }));
        }
    }
    assert_ne!(
        shape_checks[0].1.args[0], shape_checks[1].1.args[0],
        "per-call shape guards must read each call's actual tensor carrier"
    );
    assert_ne!(
        shape_checks[2].1.args[1], shape_checks[3].1.args[1],
        "each origin retains its own immutable scalar witness"
    );
    assert_ne!(
        shape_checks[4].1.args[0], shape_checks[5].1.args[0],
        "each origin guards its own destination local at the call boundary"
    );
}

#[test]
fn runtime_return_shape_relations_are_scoped_to_their_result_expression() {
    let mut types = TypeTable::new();
    let formal_axis = types.intern_index(IndexExpr::Runtime(RuntimeExtentId::FunctionParameterAxis {
        parameter: 0,
        axis: 0,
    }));
    let first_result_axis =
        types.intern_index(IndexExpr::Runtime(RuntimeExtentId::ExpressionAxis { expression: 41, axis: 0 }));
    let second_result_axis =
        types.intern_index(IndexExpr::Runtime(RuntimeExtentId::ExpressionAxis { expression: 42, axis: 0 }));
    let first_relation = ShapeRelation {
        left: first_result_axis,
        right: formal_axis,
        span: Span::default(),
        boundary: ShapeRelationBoundary::FunctionReturn,
    };
    let second_relation = ShapeRelation { left: second_result_axis, ..first_relation };

    assert!(super::runtime::shape_relation_applies_to_return(&types, &first_relation, 41,));
    assert!(!super::runtime::shape_relation_applies_to_return(&types, &first_relation, 42,));
    assert!(super::runtime::shape_relation_applies_to_return(&types, &second_relation, 42,));
}

fn assert_literal_shape_arg(types: &TypeTable, shape: crate::semantic::IndexId) {
    match types.get_index(shape) {
        IndexExpr::Literal(_) => {}
        IndexExpr::Tuple(dims) => {
            for dim in dims {
                assert_literal_shape_arg(types, *dim);
            }
        }
        other => panic!("nested monomorphization key kept symbolic shape {other:?}"),
    }
}

#[test]
fn mir_nested_shape_generic_monomorphization_direct_calls_use_concrete_child_keys() {
    let source = r"
functio inner<magnitudo N>(tensor<f32, [N]> x) → tensor<f32, [N]> {
    redde x
}

functio outer<magnitudo N>(tensor<f32, [N]> x) → tensor<f32, [N]> {
    redde inner(x)
}

incipit {
    sit tensor<f32, [4]> a ← vacua
    sit tensor<f32, [4]> b ← vacua
    sit tensor<f32, [8]> c ← vacua
    sit tensor<f32, [4]> first ← outer(a)
    sit tensor<f32, [4]> duplicate ← outer(b)
    sit tensor<f32, [8]> distinct ← outer(c)
    nota first
    nota duplicate
    nota distinct
}
";
    let unit = analyze(source);
    let lowered = lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default())
        .expect("nested generic lowering succeeds");

    assert_eq!(
        lowered.monomorphizations.len(),
        4,
        "outer and inner each form one deduplicated instance per concrete shape"
    );
    let mut instances_per_source = std::collections::HashMap::new();
    for monomorphization in &lowered.monomorphizations {
        assert_eq!(monomorphization.key.shape_args.len(), 1);
        assert_literal_shape_arg(
            lowered.validated.validation().types.as_table(),
            monomorphization.key.shape_args[0],
        );
        *instances_per_source
            .entry(monomorphization.key.source)
            .or_insert(0usize) += 1;
    }
    assert_eq!(instances_per_source.len(), 2);
    assert!(instances_per_source.values().all(|count| *count == 2));
}

#[test]
fn mir_nested_shape_generic_monomorphization_try_calls_use_concrete_child_keys() {
    let source = r"
functio inner<magnitudo N>(tensor<f32, [N]> x) → tensor<f32, [N]> ⇥ textus {
    redde x
}

functio outer<magnitudo N>(tensor<f32, [N]> x) → tensor<f32, [N]> ⇥ textus {
    redde inner(x)
}

incipit {
    sit tensor<f32, [4]> x ← vacua
    fac {
        sit tensor<f32, [4]> y ← outer(x)
        nota y
    } cape err {
        nota err
    }
}
";
    let unit = analyze(source);
    let lowered = lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default())
        .expect("nested failable generic lowering succeeds");

    assert_eq!(lowered.monomorphizations.len(), 2);
    for monomorphization in &lowered.monomorphizations {
        assert_eq!(monomorphization.key.shape_args.len(), 1);
        assert_literal_shape_arg(
            lowered.validated.validation().types.as_table(),
            monomorphization.key.shape_args[0],
        );
    }
    assert!(
        lowered
            .validated
            .program()
            .functions
            .iter()
            .all(|function| {
                function.blocks.iter().all(|block| {
                    !matches!(
                        block.terminator.kind,
                        MirTerminatorKind::TryCall { callee: MirCallee::Definition { .. }, .. }
                    )
                })
            })
    );
}

#[test]
fn mir_nested_shape_generic_monomorphization_still_rejects_truly_unbound_child() {
    let source = r"
functio inner<magnitudo N>() → numerus {
    redde 1
}

functio outer<magnitudo N>(tensor<f32, [N]> x) → numerus {
    redde inner()
}

incipit {
    sit tensor<f32, [4]> x ← vacua
    sit numerus value ← outer(x)
    nota value
}
";
    // GPD-2: `inner<magnitudo N>` declares a size param its own signature
    // shapes never mention. The declared sort is authoritative (no
    // shape-occurrence derivation), so the unbound child fails closed at
    // analysis with SEM014 instead of surfacing an unresolved param at MIR
    // lowering.
    let session = Session::new(la_config().with_target(Target::HirTypeScript));
    let Err(errors) = crate::driver::analyze_source(&session, "test.fab", source, AnalyzeRequest::default()) else {
        panic!("unbound child shape must fail closed")
    };
    assert!(
        errors
            .iter()
            .any(|error| error.issue() == Some("cannot_infer_tensor_shape")
                && error
                    .args
                    .iter()
                    .any(|arg| arg.name == "param" && arg.value == "N")),
        "expected SEM014 cannot_infer_tensor_shape for N, got {errors:?}"
    );
}

// ──────────────────────────────────────────────────────────────────────
// Shader stage identity (MirKernelShaderStage) assigned during HIR→MIR
// lowering. Stage 4 seam: @ nucleum → Compute, @ vertex → Vertex,
// @ fragment → Fragment. Conflicting annotations fail-closed to None.
// MirDeviceRole stays compute-only (Kernel/Device) — shader stage identity
// is a separate axis.
// ──────────────────────────────────────────────────────────────────────

#[test]
// ===========================================================================
// Section M: Shader stage identity — compute, vertex, fragment
// ===========================================================================

fn lowers_nucleum_annotation_to_compute_shader_stage() {
    let unit = analyze("@ nucleum\nfunctio kernel_entry() → vacuum { redde }");
    let program = lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default())
        .map(|lowered| lowered.validated.into_parts().0)
        .expect("MIR lowering succeeds");
    let function = &program.functions[0];
    assert_eq!(
        function.shader_stage,
        Some(MirKernelShaderStage::Compute),
        "@ nucleum must lower to MirKernelShaderStage::Compute"
    );
}

#[test]
fn lowers_vertex_annotation_to_vertex_shader_stage() {
    let unit = analyze("@ vertex\nfunctio vertex_entry() → vacuum { redde }");
    let program = lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default())
        .map(|lowered| lowered.validated.into_parts().0)
        .expect("MIR lowering succeeds");
    let function = &program.functions[0];
    assert_eq!(
        function.shader_stage,
        Some(MirKernelShaderStage::Vertex),
        "@ vertex must lower to MirKernelShaderStage::Vertex"
    );
}

#[test]
fn lowers_fragment_annotation_to_fragment_shader_stage() {
    let unit = analyze("@ fragment\nfunctio fragment_entry() → vacuum { redde }");
    let program = lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default())
        .map(|lowered| lowered.validated.into_parts().0)
        .expect("MIR lowering succeeds");
    let function = &program.functions[0];
    assert_eq!(
        function.shader_stage,
        Some(MirKernelShaderStage::Fragment),
        "@ fragment must lower to MirKernelShaderStage::Fragment"
    );
}

#[test]
fn lowers_plain_function_without_shader_stage() {
    let unit = analyze("functio plain() → vacuum { redde }");
    let program = lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default())
        .map(|lowered| lowered.validated.into_parts().0)
        .expect("MIR lowering succeeds");
    let function = &program.functions[0];
    assert_eq!(function.shader_stage, None, "plain function must have no shader stage");
}

#[test]
fn shader_stage_rejects_conflicting_annotations() {
    // CTO a7c616ed: one function must not simultaneously carry compute and
    // graphics annotations. Fail-closed: conflicting annotations produce no
    // shader stage.
    use crate::hir::HirFunction;
    use crate::lexer::Symbol;

    fn make_func(is_kernel: bool, is_vertex: bool, is_fragment: bool) -> HirFunction {
        HirFunction {
            name: Symbol(0),
            type_params: Vec::new(),
            params: Vec::new(),
            cli_args: None,
            ret_ty: None,
            err_ty: None,
            body: None,
            posture: radix_syntax::CallablePosture::SyncFinite,
            is_async: false,
            is_generator: false,
            is_kernel,
            nucleum_fragment: false,
            is_vertex,
            is_fragment,
            test: None,
        }
    }

    // Kernel-only → Compute.
    assert_eq!(
        shader_stage_for_function(&make_func(true, false, false)),
        Some(MirKernelShaderStage::Compute)
    );

    // Vertex-only → Vertex.
    assert_eq!(
        shader_stage_for_function(&make_func(false, true, false)),
        Some(MirKernelShaderStage::Vertex)
    );

    // Fragment-only → Fragment.
    assert_eq!(
        shader_stage_for_function(&make_func(false, false, true)),
        Some(MirKernelShaderStage::Fragment)
    );

    // Conflicting kernel + vertex: fail-closed, no stage.
    assert_eq!(shader_stage_for_function(&make_func(true, true, false)), None);

    // Conflicting vertex + fragment: fail-closed, no stage.
    assert_eq!(shader_stage_for_function(&make_func(false, true, true)), None);
}

#[test]
fn conflicting_shader_annotations_emit_diagnostic() {
    // @vertex + @fragment on a single function must produce a diagnostic.
    let unit = analyze("@ vertex\n@ fragment\nfunctio conflict() → vacuum { redde }");
    let errors = lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default()).unwrap_err();
    assert_eq!(errors.len(), 1, "expected exactly one error for conflicting annotations");
    assert!(
        errors[0].issue.contains("conflicting"),
        "error issue must mention conflicting: {}",
        errors[0].issue
    );
}

#[test]
fn conflicting_shader_annotations_with_nucleum_emit_diagnostic() {
    // @nucleum + @vertex must also produce a diagnostic.
    let unit = analyze("@ nucleum\n@ vertex\nfunctio conflict() → vacuum { redde }");
    let errors = lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default()).unwrap_err();
    assert_eq!(errors.len(), 1, "expected exactly one error for @nucleum + @vertex");
    assert!(
        errors[0].issue.contains("conflicting"),
        "error issue must mention conflicting: {}",
        errors[0].issue
    );
}

#[test]
fn lowers_vertex_method_inside_genus_to_shader_stage() {
    let unit = analyze(
        "genus Renderer {\n  varia numerus dummy = 0\n  @ vertex\n  functio draw() → vacuum { redde }\n}\n\nincipit {\n  sit _ r ← Renderer {}\n}",
    );
    let program = lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default())
        .map(|lowered| lowered.validated.into_parts().0)
        .expect("MIR lowering succeeds");
    let draw = program
        .functions
        .iter()
        .find(|f| f.name.is_some())
        .expect("genus method must be lowered with a name");
    assert_eq!(
        draw.shader_stage,
        Some(MirKernelShaderStage::Vertex),
        "genus method with @vertex must lower to MirKernelShaderStage::Vertex"
    );
}

#[test]
fn lowers_fragment_method_inside_genus_to_shader_stage() {
    let unit = analyze(
        "genus Renderer {\n  varia numerus dummy = 0\n  @ fragment\n  functio shade() → vacuum { redde }\n}\n\nincipit {\n  sit _ r ← Renderer {}\n}",
    );
    let program = lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default())
        .map(|lowered| lowered.validated.into_parts().0)
        .expect("MIR lowering succeeds");
    let shade = program
        .functions
        .iter()
        .find(|f| f.name.is_some())
        .expect("genus method must be lowered with a name");
    assert_eq!(
        shade.shader_stage,
        Some(MirKernelShaderStage::Fragment),
        "genus method with @fragment must lower to MirKernelShaderStage::Fragment"
    );
}

#[test]
fn vertex_function_injects_out_position_local_for_empty_body() {
    // Even a @vertex function with an empty body must have out_position
    // injected as a synthetic local during MIR lowering.
    let mut unit = analyze("@ vertex\nfunctio v() → vacuum { redde }");
    let out_position_name = unit.interner.intern("out_position");
    let program = lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default())
        .map(|lowered| lowered.validated.into_parts().0)
        .expect("MIR lowering succeeds");
    let function = &program.functions[0];
    assert_eq!(
        function.shader_stage,
        Some(MirKernelShaderStage::Vertex),
        "@ vertex must lower to MirKernelShaderStage::Vertex"
    );

    let out_position = function
        .locals
        .iter()
        .find(|local| local.name == Some(out_position_name))
        .expect("out_position local must be injected for @vertex function");
    assert!(out_position.mutable, "out_position must be writable (mutable=true)");
}

#[test]
fn lowers_vertex_vec3_to_vec4_widen() {
    use crate::hir::{DefId, HirBinOp, HirExpression, HirExpressionKind, HirId, HirLiteral};
    use crate::mir::{
        MirAggregateFields, MirAggregateItem, MirAggregateKind, MirOperand, MirPlaceBase, MirStatementKind,
    };
    use crate::semantic::{IndexExpr, NumericWidth, Primitive, TypeTable};

    let mut types = TypeTable::new();
    let f32_id = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let vec3_id = {
        let width = types.intern_index(IndexExpr::Literal(3));
        types.vector_with_width(f32_id, width)
    };
    let vec4_id = {
        let width = types.intern_index(IndexExpr::Literal(4));
        types.vector_with_width(f32_id, width)
    };

    let vec3_mir = MirType::semantic(vec3_id);
    let vec4_mir = MirType::semantic(vec4_id);
    let span = Span::default();

    let mut builder = FunctionBuilder::new(&types);
    builder.shader_stage = Some(MirKernelShaderStage::Vertex);

    // Set up bindings: position → vec3 (read-only), out_position → vec4 (writable)
    let position_def = DefId(100);
    let out_def = DefId(101);
    builder.add_test_binding(position_def, vec3_mir, false);
    let out_local = builder.add_test_binding(out_def, vec4_mir, true);

    // Create HIR expression: position * 0.5  (type = vec3)
    let position_expr =
        HirExpression { id: HirId(0), kind: HirExpressionKind::Path(position_def), ty: Some(vec3_id), span };
    let half_expr = HirExpression {
        id: HirId(1),
        kind: HirExpressionKind::Literal(HirLiteral::Float(0.5)),
        ty: Some(f32_id),
        span,
    };
    let mul_expr = HirExpression {
        id: HirId(2),
        kind: HirExpressionKind::Binary(HirBinOp::Mul, Box::new(position_expr), Box::new(half_expr)),
        ty: Some(vec3_id),
        span,
    };

    // Lower: out_position ← position * 0.5
    let entry = builder.fresh_block(span);
    builder.switch_to(entry);
    let result = builder.lower_expr_to_destination(&mul_expr, MirPlace::local(out_local), vec4_mir);

    assert!(result.is_some(), "lowering should succeed");
    assert!(builder.errors.is_empty(), "unexpected errors: {:?}", builder.errors);

    // Check that a Construct(Vector) statement was emitted
    let stmts = &builder.blocks[0].statements;
    // Expected: lower_expr_value emits temp = position * 0.5 (Assign),
    // then emit_vec3_to_vec4_construct emits Construct out_position = vec4(temp, 1.0)
    let construct_stmt = stmts
        .iter()
        .find(|s| matches!(s.kind, MirStatementKind::Construct { .. }))
        .expect("must have a Construct statement");
    let MirStatementKind::Construct { destination, aggregate } = &construct_stmt.kind else {
        panic!("unreachable: find already matched Construct");
    };
    assert_eq!(
        destination.base,
        MirPlaceBase::Local(out_local),
        "destination should be out_position local"
    );
    assert_eq!(aggregate.kind, MirAggregateKind::Vector, "aggregate kind must be Vector");
    assert_eq!(aggregate.ty, vec4_mir, "aggregate type must be vec4<f32>");

    let MirAggregateFields::Ordered(items) = &aggregate.fields else {
        panic!("expected Ordered fields, got {:?}", aggregate.fields);
    };
    // Scalar-per-lane widening: four scalar elements, not the old mixed [vec3, scalar].
    assert_eq!(items.len(), 4, "construct should have 4 scalar-lane operands");
    // lanes 0..2 are VectorLane projections of the vec3 source.
    for (lane, item) in items.iter().take(3).enumerate() {
        let MirAggregateItem::Operand(MirOperand::Place(place)) = item else {
            panic!("lane {lane} should be a Place operand, got {item:?}");
        };
        assert!(
            place
                .projections
                .iter()
                .any(|p| matches!(p, MirProjection::VectorLane(n) if *n as usize == lane)),
            "lane {lane} should project VectorLane({lane}), got {place:?}"
        );
    }
    // lane 3 is the materialized 1.0 w-component (assigned to a temp).
    assert!(
        matches!(items[3], MirAggregateItem::Operand(MirOperand::Temp(_))),
        "fourth operand must be the materialized 1.0 w-component (temp), got {:?}",
        items[3]
    );
}

// ──────────────────────────────────────────────────────────────────────
// F-024: variant_payload must report errors for missing metadata or arity
// mismatch instead of silently falling back to Ordered fields.
// ──────────────────────────────────────────────────────────────────────

#[test]
fn variant_payload_errors_on_missing_field_metadata() {
    let types = TypeTable::new();
    let span = Span::default();
    let mut builder = FunctionBuilder::new(&types);

    // No variant field metadata in the empty context — must error, not silently Ordered.
    let fields = builder.variant_payload(DefId(999), vec![MirOperand::Constant(MirConstant::Nil)], span);

    assert!(
        matches!(fields, MirAggregateFields::Ordered(_)),
        "degraded output must still be Ordered"
    );
    assert_eq!(builder.errors.len(), 1, "must report one error, got: {:?}", builder.errors);
    assert!(
        builder.errors[0]
            .issue
            .contains("variant_payload_field_metadata_missing"),
        "error issue must encode missing variant payload metadata, got: {}",
        builder.errors[0].issue
    );
}

#[test]
fn variant_payload_errors_on_arity_mismatch() {
    let types = TypeTable::new();
    let span = Span::default();
    let mut builder = FunctionBuilder::new(&types);

    // Inject a variant with 2 field names into the context.
    let variant = DefId(200);
    let field_a = Symbol(1);
    let field_b = Symbol(2);
    let variant_fields_map = {
        let mut map = FxHashMap::default();
        map.insert(variant, vec![field_a, field_b]);
        Arc::new(map)
    };
    builder.context.variant_fields = variant_fields_map;

    // Call with 1 arg when 2 are expected — must error, not silently Ordered.
    let fields = builder.variant_payload(variant, vec![MirOperand::Constant(MirConstant::Nil)], span);

    assert!(
        matches!(fields, MirAggregateFields::Ordered(_)),
        "degraded output must still be Ordered"
    );
    assert_eq!(builder.errors.len(), 1, "must report one error, got: {:?}", builder.errors);
    assert!(
        builder.errors[0]
            .issue
            .contains("variant_payload_arity_mismatch"),
        "error issue must encode variant payload arity mismatch, got: {}",
        builder.errors[0].issue
    );
}

#[test]
fn variant_payload_produces_named_fields_when_metadata_and_arity_match() {
    let types = TypeTable::new();
    let span = Span::default();
    let mut builder = FunctionBuilder::new(&types);

    // Inject a variant with 2 field names into the context.
    let variant = DefId(300);
    let field_a = Symbol(11);
    let field_b = Symbol(12);
    let variant_fields_map = {
        let mut map = FxHashMap::default();
        map.insert(variant, vec![field_a, field_b]);
        Arc::new(map)
    };
    builder.context.variant_fields = variant_fields_map;

    let fields = builder.variant_payload(
        variant,
        vec![
            MirOperand::Constant(MirConstant::Int(1)),
            MirOperand::Constant(MirConstant::Int(2)),
        ],
        span,
    );

    assert!(
        matches!(fields, MirAggregateFields::Named(_)),
        "must produce Named fields when metadata and arity match, got {fields:?}"
    );
    assert!(builder.errors.is_empty(), "no errors expected, got: {:?}", builder.errors);
}

// ---------------------------------------------------------------------------
// N — Shared MIR CLI-program lowering (wasm-host-parity promotion packet P1)
// ---------------------------------------------------------------------------

fn analyze_exemplum_for_mir(rel: &str) -> AnalyzedModule {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus")
        .join(rel);
    let source = std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
    analyze(&source)
}

/// Collect the field names of every `record` aggregate construct in the
/// lowered program, resolved against the lowering interner.
fn record_aggregate_fields(program: &MirProgram, interner: &Interner) -> Vec<Vec<String>> {
    let mut surfaces = Vec::new();
    for function in &program.functions {
        for block in &function.blocks {
            for statement in &block.statements {
                let MirStatementKind::Construct { aggregate, .. } = &statement.kind else {
                    continue;
                };
                let MirAggregateFields::Named(items) = &aggregate.fields else {
                    continue;
                };
                if !matches!(aggregate.kind, MirAggregateKind::Record) {
                    continue;
                }
                let names = items
                    .iter()
                    .map(|item| interner.resolve(item.name).to_owned())
                    .collect::<Vec<_>>();
                surfaces.push(names);
            }
        }
    }
    surfaces
}

/// Locate the field value for `field` in the first record aggregate that
/// carries it.
fn record_field_value<'a>(function: &'a MirFunction, field: &str, interner: &Interner) -> Option<&'a MirOperand> {
    for block in &function.blocks {
        for statement in &block.statements {
            let MirStatementKind::Construct { aggregate, .. } = &statement.kind else {
                continue;
            };
            let MirAggregateFields::Named(items) = &aggregate.fields else {
                continue;
            };
            if let Some(item) = items
                .iter()
                .find(|item| interner.resolve(item.name) == field)
            {
                return Some(&item.value);
            }
        }
    }
    None
}

/// All ten `shared-mir` CLI-family rows (promotion packet P1 §2) must lower
/// through the default `lower_analyzed_unit` path — the
/// `allow_cli_program` gate is lifted and the shared lowering derives the
/// complete `argumenta` record surface from the CLI descriptor.
#[test]
fn cli_fixture_family_lowers_through_default_path() {
    let fixtures = [
        "cli/cli.fab",
        "argumenta/argumenta.fab",
        "descriptio/descriptio.fab",
        "exitus/exitus.fab",
        "meta/versio.fab",
        "operandus/operandus.fab",
        "optio/optio.fab",
        "ubique/ubique.fab",
        "annotation-sugar/cli-braced.fab",
        "annotation-sugar/optio-braced.fab",
    ];
    for fixture in fixtures {
        let analysis = analyze_exemplum_for_mir(fixture);
        let lowered = lower_analyzed_unit(analysis.application_view(), MirLoweringOptions::default())
            .unwrap_or_else(|errors| panic!("{fixture} must lower through the default path: {errors:?}"));
        let surfaces = record_aggregate_fields(lowered.validated.program(), &lowered.interner);
        assert!(
            !surfaces.is_empty(),
            "{fixture} must construct the CLI `argumenta` record aggregate through the default path, got: {surfaces:?}"
        );
    }
}

/// The shared entry/args contract carries the option/operand binding: the
/// entry `argumenta` record (global surface) and the `greet` command record
/// (global + command-local) are both constructed from the descriptor.
#[test]
fn cli_fab_subcommand_records_carry_option_and_operand_binding() {
    let analysis = analyze_exemplum_for_mir("cli/cli.fab");
    let lowered =
        lower_analyzed_unit(analysis.application_view(), MirLoweringOptions::default()).expect("lower cli/cli.fab");
    let surfaces = record_aggregate_fields(lowered.validated.program(), &lowered.interner);
    assert!(
        surfaces.contains(&vec!["verbose".to_owned()]),
        "entry record must bind the global verbose option, got: {surfaces:?}"
    );
    assert!(
        surfaces.contains(&vec!["verbose".to_owned(), "nomen".to_owned()]),
        "greet record must bind the global verbose option and the nomen operand, got: {surfaces:?}"
    );

    let greet = lowered
        .validated
        .program()
        .functions
        .iter()
        .find(|function| {
            function
                .name
                .is_some_and(|name| lowered.interner.resolve(name) == "greet")
        })
        .expect("greet command function");
    // Global flag default: false. Required operand placeholder: empty textus.
    let verbose = record_field_value(greet, "verbose", &lowered.interner).expect("greet record binds verbose");
    assert!(
        matches!(verbose, MirOperand::Constant(MirConstant::Bool(false))),
        "verbose flag must carry its typed default false, got: {verbose:?}"
    );
    let nomen = record_field_value(greet, "nomen", &lowered.interner).expect("greet record binds nomen");
    let MirOperand::Constant(MirConstant::String(symbol)) = nomen else {
        panic!("nomen must carry a textus placeholder, got: {nomen:?}");
    };
    assert_eq!(lowered.interner.resolve(*symbol), "", "nomen placeholder is the empty textus");
}

/// Option/operand binding semantics through the shared path: flags default to
/// false, undefaulted value options to `nihil`, rest operands to an empty
/// array aggregate.
#[test]
fn cli_option_and_operand_binding_typed_defaults() {
    let optio = analyze_exemplum_for_mir("optio/optio.fab");
    let lowered =
        lower_analyzed_unit(optio.application_view(), MirLoweringOptions::default()).expect("lower optio/optio.fab");
    let surfaces = record_aggregate_fields(lowered.validated.program(), &lowered.interner);
    assert!(
        surfaces.contains(&vec!["verbose".to_owned(), "count".to_owned()]),
        "optio record must bind verbose and count, got: {surfaces:?}"
    );
    let entry = lowered
        .validated
        .program()
        .functions
        .iter()
        .find(|function| function.source.is_none() && function.name.is_none())
        .expect("synthetic entry function");
    let verbose = record_field_value(entry, "verbose", &lowered.interner).expect("verbose binding");
    assert!(
        matches!(verbose, MirOperand::Constant(MirConstant::Bool(false))),
        "verbose flag default false, got: {verbose:?}"
    );
    let count = record_field_value(entry, "count", &lowered.interner).expect("count binding");
    assert!(
        matches!(count, MirOperand::Constant(MirConstant::Nil)),
        "undefaulted value option is nihil, got: {count:?}"
    );

    let operandus = analyze_exemplum_for_mir("operandus/operandus.fab");
    let lowered = lower_analyzed_unit(operandus.application_view(), MirLoweringOptions::default())
        .expect("lower operandus/operandus.fab");
    let entry = lowered
        .validated
        .program()
        .functions
        .iter()
        .find(|function| function.source.is_none() && function.name.is_none())
        .expect("synthetic entry function");
    let files = record_field_value(entry, "files", &lowered.interner).expect("files rest binding");
    assert!(
        matches!(files, MirOperand::Temp(_)),
        "rest operand must bind an empty array aggregate temp, got: {files:?}"
    );
    let input = record_field_value(entry, "input", &lowered.interner).expect("input binding");
    assert!(
        matches!(input, MirOperand::Constant(MirConstant::String(_))),
        "required operand carries a textus placeholder, got: {input:?}"
    );
}

// ---------------------------------------------------------------------------
// O — P3 frontend async/promissum fixtures (wasm-host-parity packet P3)
// ---------------------------------------------------------------------------

/// Shared analysis path: prepare the program-layer import contract and
/// analyze a corpus fixture. Bare `analyze_source` stays fail-closed
/// (empty contracts); on-disk async providers need the prepared path.
fn analyze_exemplum_for_mir_async(rel: &str) -> AnalyzedModule {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus")
        .join(rel);
    let source = std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
    let session = Session::new(
        la_config()
            .with_target(Target::MirLlvm)
            .with_dev_locale_root(),
    );
    crate::program::analyze_prepared_program_path(&session, &path, &source).unwrap_or_else(|diagnostics| {
        let facts: Vec<String> = diagnostics
            .iter()
            .map(|d| format!("{:?} {:?} {:?}", d.code, d.issue(), d.args))
            .collect();
        panic!("{rel} must analyze: {facts:?}")
    })
}

/// The three `frontend_analysis_failed` rows (promotion packet P3 §2) must
/// analyze through the prepared program-layer contract (RTR1b) and lower
/// to non-empty MIR without the await/yield rejection — closing the
/// SEM010/SEM011 analysis gap and the `unsupported_mir_lowering_await_yield_expression`
/// lowering gap in one shared path.
#[test]
fn async_fixture_family_analyzes_and_lowers_through_shared_path() {
    for fixture in [
        "ad/async-solum-leget.fab",
        "ad/async-tempus-dormiet.fab",
        "ad/solum-lege-generic.fab",
    ] {
        let analysis = analyze_exemplum_for_mir_async(fixture);
        assert!(
            !analysis
                .diagnostics
                .iter()
                .any(|d| d.issue() == Some("await_bind_requires_promissum")
                    || d.issue() == Some("cede_requires_promissum")
                    || d.issue() == Some("dynamic_receiver_method_type_args")),
            "{fixture} must not reject await/cede on promissum-returning receivers: {:?}",
            analysis.diagnostics
        );
        let lowered = lower_analyzed_unit(analysis.application_view(), MirLoweringOptions::default())
            .unwrap_or_else(|errors| panic!("{fixture} must lower: {errors:?}"));
        assert!(
            !lowered.validated.program().functions.is_empty(),
            "{fixture} must lower to a non-empty MIR program"
        );
    }
}

/// `await tempus.dormiet(ms)` lowers the awaited provider call (the promissum
/// carrier temp) and discards the payload; the tempus:dormiet route survives.
#[test]
fn async_tempus_tacebit_lowers_provider_call_and_discards_payload() {
    let analysis = analyze_exemplum_for_mir_async("ad/async-tempus-dormiet.fab");
    let lowered =
        lower_analyzed_unit(analysis.application_view(), MirLoweringOptions::default()).expect("lower async tempus");
    let dump = crate::mir::dump_program(lowered.validated.program());
    let provider_calls = dump.matches("runtime provider").count();
    assert!(
        provider_calls >= 2,
        "tempus expectet + dormiet routes must lower through the shared provider intrinsic ({provider_calls} calls):\n{dump}"
    );
    assert!(
        !dump.contains("unsupported"),
        "no unsupported surface in the async tempus unit:\n{dump}"
    );
}

/// `await_const` on a promissum-returning receiver binds the payload carrier
/// into the awaited local (the shared payload-carrier fact).
#[test]
fn async_solum_await_const_binds_payload_carrier() {
    let analysis = analyze_exemplum_for_mir_async("ad/async-solum-leget.fab");
    let lowered = lower_analyzed_unit(analysis.application_view(), MirLoweringOptions::default())
        .expect("lower async solum leget");
    let dump = crate::mir::dump_program(lowered.validated.program());
    assert!(
        dump.matches("runtime provider").count() >= 2,
        "solum scribe + leget routes must lower through the shared provider intrinsic:\n{dump}"
    );
    assert!(
        !dump.contains("unsupported"),
        "no unsupported surface in the async solum leget unit:\n{dump}"
    );
}

/// `reddet future` (await-and-return) materializes the awaited payload into the
/// enclosing function's declared return carrier so the return terminator
/// validates against the declared (payload) return type.
#[test]
fn reddet_awaits_promissum_and_returns_payload() {
    let source = r"
functio fetch() fiet → numerus {
    redde 42
}
functio forward() fiet → numerus {
    reddet fetch()
}
incipiet {
    nota 1
}
";
    let analysis = analyze(source);
    let lowered =
        lower_analyzed_unit(analysis.application_view(), MirLoweringOptions::default()).expect("lower reddet unit");
    assert!(
        lowered
            .validated
            .program()
            .functions
            .iter()
            .any(|function| function
                .name
                .is_some_and(|name| lowered.interner.resolve(name) == "forward")),
        "forward function must lower"
    );
    let dump = crate::mir::dump_program(lowered.validated.program());
    assert!(
        !dump.contains("unsupported"),
        "no unsupported surface in the reddet unit:\n{dump}"
    );
}

/// An awaited `@ future` call produces the success payload, so a void-payload
/// async call lowers to a destination-less `Call` rather than a
/// `promissum<vacuum>` temp. This pins both halves of the async-callee typing
/// change at the statement level (c75db9ed-11).
#[test]
fn await_future_void_call_validates() {
    let source = r"
functio pause() fiet → vacuum {
    nota 0
}

incipiet {
    tacebit pause()
}
";
    let analysis = analyze(source);
    let lowered =
        lower_analyzed_unit(analysis.application_view(), MirLoweringOptions::default()).expect("lower await unit");

    let pause = lowered
        .validated
        .program()
        .functions
        .iter()
        .find(|function| {
            function
                .name
                .is_some_and(|name| lowered.interner.resolve(name) == "pause")
        })
        .and_then(|function| function.source)
        .expect("pause function must lower with a source definition");

    let mut destinations = Vec::new();
    for function in &lowered.validated.program().functions {
        for block in &function.blocks {
            for statement in &block.statements {
                if let MirStatementKind::Call { destination, callee: MirCallee::Definition { source, .. }, .. } =
                    &statement.kind
                    && *source == pause
                {
                    destinations.push(destination.is_some());
                }
            }
        }
    }

    assert!(!destinations.is_empty(), "the await of pause() must lower to a definition call");
    assert!(
        destinations.iter().all(|has_destination| !has_destination),
        "a void-payload async call must have no destination temp: {destinations:?}"
    );
}

/// An awaited in-unit generic `@ future` call instantiates its payload, so the
/// call destination temp carries the concrete `T` (`textus` here) rather than
/// the callee's declared `promissum<T>` handle. Pins the non-void generic case
/// the void-only async tests could not see (c75db9ed-13.1).
#[test]
fn await_generic_future_call_destination_is_instantiated() {
    let source = r#"
functio lege<T>(T x) fiet → T {
    redde x
}

incipiet {
    figendum textus corpus ← lege<textus>("verbum")
    nota 1
}
"#;
    let analysis = analyze(source);
    let lowered = lower_analyzed_unit(analysis.application_view(), MirLoweringOptions::default())
        .expect("lower awaited generic unit");

    let dump = crate::mir::dump_program(lowered.validated.program());
    assert!(
        !dump.contains("unsupported"),
        "no unsupported surface in the awaited generic unit:\n{dump}"
    );

    let lege = lowered
        .validated
        .program()
        .functions
        .iter()
        .find(|function| {
            function
                .name
                .is_some_and(|name| lowered.interner.resolve(name) == "lege")
        })
        .and_then(|function| function.source)
        .expect("lege function must lower with a source definition");

    let types = lowered.validated.validation().types.as_table();

    let mut destination_types = Vec::new();
    for function in &lowered.validated.program().functions {
        for block in &function.blocks {
            for statement in &block.statements {
                let MirStatementKind::Call { destination, callee, .. } = &statement.kind else {
                    continue;
                };
                // The generic callee monomorphizes to a definition-backed
                // `Function`; a non-generic in-unit callee stays `Definition`.
                // Resolve either back to `lege`'s source definition.
                let callee_source = match callee {
                    MirCallee::Definition { source, .. } => Some(*source),
                    MirCallee::Function(id) => lowered
                        .validated
                        .program()
                        .functions
                        .iter()
                        .find(|candidate| candidate.id == *id)
                        .and_then(|candidate| candidate.source),
                    MirCallee::Closure(_) | MirCallee::Value(_) => None,
                };
                if callee_source != Some(lege) {
                    continue;
                }
                let place = destination
                    .as_ref()
                    .expect("an awaited non-void generic call must carry a destination temp");
                let MirPlaceBase::Temp(temp_id) = place.base else {
                    panic!("awaited generic call destination must be an unprojected temp: {place:?}");
                };
                let temp = function
                    .temps
                    .iter()
                    .find(|temp| temp.id == temp_id)
                    .expect("destination temp must exist on the owning function");
                destination_types.push(temp.ty.semantic_id());
            }
        }
    }

    assert!(
        !destination_types.is_empty(),
        "the await of lege<textus>() must lower to a definition call"
    );
    let textus = Type::Primitive(Primitive::Textus);
    assert!(
        destination_types.iter().all(|id| types.get(*id) == &textus),
        "awaited generic call destination must carry the instantiated payload, not promissum<textus>: {:?}",
        destination_types
            .iter()
            .map(|id| types.get(*id))
            .collect::<Vec<_>>()
    );
}

// ===========================================================================
// Section N — CDA-U6: `↤` conversion-directed assignment lowering
// ===========================================================================

fn count_occurrences(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

#[test]
fn conversio_assign_lowers_through_shared_conversion_owner() {
    // `count ↤ "42"` must lower through the shared `↦` conversion owner
    // (`lower_conversio`) exactly once, then store into the destination place
    // exactly once — no codegen-only rewrite duplicating evaluation.
    let dump = dump_source("incipit {\n  varia numerus count ← 0\n  count ↤ \"42\"\n}\n");
    assert!(
        dump.contains("runtime convert runtime"),
        "↤ must lower through the shared conversion owner:\n{dump}"
    );
    assert_eq!(
        count_occurrences(&dump, "runtime convert runtime"),
        1,
        "conversion must happen exactly once:\n{dump}"
    );
    assert_eq!(
        count_occurrences(&dump, " = %"),
        1,
        "the converted value must be stored into the destination exactly once:\n{dump}"
    );
    assert!(!dump.contains("unsupported"), "no unsupported surface:\n{dump}");
}

#[test]
fn conversio_assign_typed_init_lowers_single_convert_then_store() {
    // `sit numerus n ↤ "42"`: the destination is the local being declared;
    // convert the source to the written type once, store once.
    let dump = dump_source("incipit {\n  sit numerus n ↤ \"42\"\n}\n");
    assert_eq!(
        count_occurrences(&dump, "runtime convert runtime"),
        1,
        "typed ↤ init must convert exactly once:\n{dump}"
    );
    assert_eq!(
        count_occurrences(&dump, " = %"),
        1,
        "typed ↤ init must store into the local exactly once:\n{dump}"
    );
    assert!(!dump.contains("unsupported"), "no unsupported surface:\n{dump}");
}

#[test]
fn conversio_assign_recovery_lowers_through_conversion_recovery_slot() {
    // `⊥` default travels through the same conversion intrinsic as `↦`.
    let dump = dump_source("incipit {\n  varia numerus count ← 0\n  count ↤ \"x\" ⊥ 0\n}\n");
    assert!(
        dump.contains("runtime convert runtime"),
        "↤ with recovery must lower through the conversion owner:\n{dump}"
    );
    assert!(
        dump.contains("recovery"),
        "⊥ recovery must be carried by the conversion:\n{dump}"
    );
}

#[test]
fn conversio_assign_call_recovery_is_deferred_out_of_the_main_flow() {
    // CTO-3: a side-effecting `⊥ fb()` recovery CALL is lowered into an
    // orphan block — NOT into the main (bb0) flow — so it cannot run when the
    // conversion succeeds (fallback-not-run-on-success; the runner invokes it
    // lazily at the failure point).
    let dump = dump_source(
        "functio fb() → numerus {\n  redde 0\n}\nincipit {\n  varia numerus count ← 0\n  count ↤ \"42\" ⊥ fb()\n}\n",
    );
    let body = first_block_body(&dump);
    assert!(
        !body.contains("call def#"),
        "the side-effecting recovery call must not be in the main flow (it would run on success):\n{dump}"
    );
    assert!(
        dump.contains("runtime convert runtime") && dump.contains("recovery"),
        "the conversion must still carry the deferred recovery:\n{dump}"
    );
}

#[test]
fn conversio_recovery_with_branching_fallback_validates_as_a_recovery_region() {
    // need: a `⊥` fallback with its own branches (`c ✓ fb() ✗ 0`)
    // lowers into a detached multi-block recovery region; validation must
    // accept the whole region, not only its tail block.
    let source = "functio fb() → numerus {\n  redde 7\n}\nincipit {\n  sit bivalens c ← verum\n  \
                  sit numerus n ← \"x\" ↦ numerus ⊥ (c ✓ fb() ✗ 0)\n  nota n\n}\n";
    let unit = analyze(source);
    let lowered =
        lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default()).expect("MIR lowering succeeds");
    validate_program(lowered.validated.program(), lowered.validated.validation())
        .expect("branching conversion recovery validates");
}

#[test]
fn conversio_assign_chain_lowers_inner_then_outer_conversion() {
    // Right-associated chain `a ↤ b ↤ "42"`: the inner conversion feeds the
    // outer conversion; exactly two conversions, one store per link.
    let dump = dump_source("incipit {\n  varia numerus a ← 0\n  varia numerus b ← 0\n  a ↤ b ↤ \"42\"\n}\n");
    assert_eq!(
        count_occurrences(&dump, "runtime convert runtime"),
        2,
        "chain must convert inner-to-outer exactly once per link:\n{dump}"
    );
    assert_eq!(count_occurrences(&dump, " = %"), 2, "chain must store once per link:\n{dump}");
    assert!(!dump.contains("unsupported"), "no unsupported surface:\n{dump}");
}

// ---------------------------------------------------------------------------
// SFR-4: sequential `reducta via summa ex` MIR fold over the apud tensor-iteration arm
// ---------------------------------------------------------------------------

#[test]
fn sequential_reduction_sum_identity_term_lowers_to_a_fold() {
    // Identity term: every element folds with `+` from a zero accumulator —
    // matching `a.summa()` for this tensor.
    let dump = dump_source(
        r"functio total(tensor<f32, [8]> a) → f32 {
    redde reducta via summa ex a apud [i] sit s { redde s }
}",
    );
    assert!(!dump.contains("unsupported"), "no unsupported surface:\n{dump}");
    assert!(dump.contains("0.0"), "accumulator zero-init must be present:\n{dump}");
    assert!(dump.contains('+'), "the fold combine must be an add:\n{dump}");
}

#[test]
fn sequential_reduction_sum_without_apud_lowers_rank1() {
    let dump = dump_source(
        r"functio total(tensor<f32, [8]> a) → f32 {
    redde reducta via summa ex a sit s { redde s }
}",
    );
    assert!(!dump.contains("unsupported"), "no unsupported surface:\n{dump}");
}

#[test]
fn reduction_max_without_apud_matches_apud_form() {
    let source = r#"+++
locale = "en"
+++

fn maximum_without_apud(tf32[3] values) → f32 {
    return reduce via max from values
}

fn maximum_with_apud(tf32[3] values) → f32 {
    return reduce via max from values at [i]
}

main {
    let list<f32> flat ← [1.0, 4.0, 2.0]
    let tf32[3] values ← flat ↦ tf32[3]
    print maximum_without_apud(values)
    print maximum_with_apud(values)
}
"#;
    let mut host = crate::mir::BufferHost::default();
    crate::mir::run_source(
        &Session::new(la_config().with_target(Target::HirTypeScript)),
        "reduction-max.fab",
        source,
        &mut host,
    )
    .unwrap_or_else(|error| panic!("binderless and apud max reductions must run: {error:?}"));
    assert_eq!(host.stdout_lines, ["4.0", "4.0"]);
}

#[test]
fn sequential_reduction_sum_numerus_term_folds_int_zero() {
    let dump = dump_source(
        r"functio total(tensor<i32, [4]> a) → i32 {
    redde reducta via summa ex a apud [i] sit s { redde s }
}",
    );
    assert!(!dump.contains("unsupported"), "no unsupported surface:\n{dump}");
    assert!(
        dump.contains("const int 0"),
        "numerus accumulator zero-init must be present:\n{dump}"
    );
}

#[test]
fn sequential_reduction_sum_transformed_term_folds_per_element_body() {
    // Non-identity term: the body's local mutation still folds once per
    // element — term `s + 1.0` over 8 lanes.
    let dump = dump_source(
        r"functio total(tensor<f32, [8]> a) → f32 {
    redde reducta via summa ex a apud [i] sit s {
        varia f32 t ← s
        t ← t + 1.0
        redde t
    }
}",
    );
    assert!(!dump.contains("unsupported"), "no unsupported surface:\n{dump}");
    assert!(
        dump.contains("runtime collection tensor_get"),
        "element gather must ride the tensor arm:\n{dump}"
    );
}

#[test]
fn filum_reduction_sum_lowers_to_declared_plan_entry() {
    // SFR-5: the `filum` spelling lowers to the declared `FilumReduce` (the
    // kernel-plan entry), never inline base-lane MIR.
    let unit = analyze(
        r"@ nucleum
functio k(tensor<f32, [256]> a) → f32 {
    redde reducta via summa ex a apud [i] filum f sit s { redde s }
}",
    );
    let dump = dump_analyzed_unit(unit.application_view()).expect("filum summa lowers to the SFR-5 declaration");
    assert!(
        dump.contains("filum_reduce op=sum form=declared"),
        "expected the FilumReduce declaration in the MIR dump:\n{dump}"
    );
}

#[test]
fn lista_elementwise_dynamic_pair_lowers_to_array_mir_op() {
    let dump = dump_source(
        r"functio addita(lista<numerus> a, lista<numerus> b) → lista<numerus> {
    redde a + b
}",
    );
    assert!(
        dump.contains("runtime collection array_add"),
        "dynamic lista arithmetic must use the runner collection op:\n{dump}"
    );
    assert!(!dump.contains("invalid MIR"), "dynamic lista arithmetic must validate:\n{dump}");
}

// A literal capacity is a storage limit, never the live length: expanding the
// operation over `N` cells read past the live length of any list shorter than
// `N` (the audit defect), so a bounded pair takes the runtime op like any other
// pair and the runner pairs the live lengths.
#[test]
fn lista_elementwise_literal_bounds_take_the_runtime_op() {
    let dump = dump_source(
        r"functio addita(lista<numerus, magnitudo 3> a, lista<numerus, magnitudo 3> b) → lista<numerus, magnitudo 3> {
    redde a + b
}",
    );
    assert!(
        dump.contains("runtime collection array_add"),
        "literal-bounded lista arithmetic must pair live lengths at run time:\n{dump}"
    );
    assert!(
        !dump.contains("construct array"),
        "a capacity must not be expanded into cells:\n{dump}"
    );
}

#[test]
fn minmax_infix_clamp_surfaces_route_through_method_twins() {
    let dump = dump_source(
        r"
functio scalar(numerus value, numerus bound) → numerus {
    redde value ⤒ bound
}

functio collection(lista<numerus> values, numerus bound) → lista<numerus> {
    redde values ⤒ bound
}

functio tensorium(tensor<f32, [3]> values, f32 bound) → tensor<f32, [3]> {
    redde values ⤒ bound
}

functio chained(lista<numerus> values, numerus lower, numerus upper) → lista<numerus> {
    redde values ⤒ lower ⤓ upper
}
",
    );
    assert_eq!(dump.matches("runtime collection maximum").count(), 4, "{dump}");
    assert_eq!(dump.matches("runtime collection minimum").count(), 1, "{dump}");
    assert!(!dump.contains("method call before runtime/provider MIR lowering"), "{dump}");
}

#[test]
fn minmax_infix_clamp_runner_matches_method_twins() {
    let source = r"
incipit {
    sit numerus scalar ← 2 ⤒ 5
    nota scalar

    sit lista<numerus> values ← [1, 5, 9]
    sit lista<numerus> clamped ← values ⤒ 4 ⤓ 8
    nota clamped.summa()

    sit lista<f32> flat ← [-1.0, 0.5, 2.0]
    sit tf32[3] values_tensor ← flat ↦ tf32[3]
    sit tf32[3] clamped_tensor ← values_tensor ⤒ 0.0 ⤓ 1.0
    nota clamped_tensor.summa()
}
";
    let mut host = crate::mir::BufferHost::default();
    crate::mir::run_source(
        &Session::new(la_config().with_target(Target::HirTypeScript)),
        "minmax-infix-clamp.fab",
        source,
        &mut host,
    )
    .unwrap_or_else(|error| panic!("minmax infix clamp runner proof should execute: {error:?}"));
    assert_eq!(host.stdout_lines, ["5", "17", "1.5"]);
}

#[test]
fn copia_filtrata_and_mappata_walk_on_the_runner() {
    // GAPS #44 / collections C1: a `copia` receiver walks the collection
    // transform. The set is snapshotted into a `lista<T>` before the loop and
    // the result is rebuilt as a set, which is unordered (D19.2), so each
    // result is sorted before printing — never a raw set or an unsorted
    // set-derived list.
    let lines = run_lines(
        "sit copia<numerus> s ← [1, 2, 3]\n\
         sit copia<numerus> f ← s.filtrata(numerus x ∴ x ≻ 1)\n\
         sit copia<numerus> m ← s.mappata(numerus x → numerus ∴ x % 2)\n\
         nota ordinata via ascendens ex f\n\
         nota ordinata via ascendens ex m",
    )
    .expect("copia filtrata/mappata must walk on the runner");
    assert_eq!(lines, ["[2, 3]", "[0, 1]"]);
}

#[test]
fn itera_ab_walks_an_intervallum_value() {
    // GAPS #44 / collections C3: `faber check` admits `itera ab` over a value
    // whose type is `intervallum`, but lowering must materialize the stored
    // interval into a `lista<T>` and walk it in order rather than decline with
    // `itera ab source before range MIR lowering`. The stored interval is
    // always monotonic (construction rejects reversed bounds), so the walk
    // order matches the literal range it was built from.
    let lines = run_lines("sit intervallum<numerus> r ← 2‥5\nitera ab r sit i { nota i }")
        .expect("itera ab over an exclusive intervallum value must walk on the runner");
    assert_eq!(lines, ["2", "3", "4"]);

    let lines = run_lines("sit intervallum<numerus> s ← 2…5\nitera ab s sit i { nota i }")
        .expect("itera ab over an inclusive intervallum value must walk on the runner");
    assert_eq!(lines, ["2", "3", "4", "5"]);
}

/// Run `body` as an `incipit` block and return its printed lines.
fn run_lines(body: &str) -> Result<Vec<String>, String> {
    let mut host = crate::mir::BufferHost::default();
    let source = format!("incipit {{\n{body}\n}}");
    crate::mir::run_source(
        &Session::new(la_config().with_target(Target::HirTypeScript)),
        "decimal-exact-until-store.fab",
        &source,
        &mut host,
    )
    .map_err(|error| format!("{error:?}"))?;
    Ok(host.stdout_lines)
}

#[test]
fn decimal_sum_is_exact_and_stores_at_scale_eight() {
    // 0.1 + 0.2 = 0.3 (carrier 30_000_000 at scale 8), printed shortest.
    let lines = run_lines("sit d64 a ← 0.1\nsit d64 b ← 0.2\nsit d64 c ← a + b\nnota c").expect("runs");
    assert_eq!(lines, ["0.3"]);
}

#[test]
fn decimal_product_keeps_its_exact_scale_until_a_slot_stores_it() {
    // 19.99 * 0.0725 = 1.449275 exactly (no rounding: 6 fraction digits).
    // 19.99 * 0.0725 * 0.0725 = 0.1050724375 (scale 10) prints exactly and
    // stores half-even at scale 8 as 0.10507244.
    let lines = run_lines(
        "sit d64 p ← 19.99\nsit d64 r ← 0.0725\n\
         sit d64 t ← p * r\nnota t\nnota p * r * r\n\
         sit d64 s ← p * r * r\nnota s",
    )
    .expect("runs");
    assert_eq!(lines, ["1.449275", "0.1050724375", "0.10507244"]);
}

#[test]
fn decimal_expression_rounds_once_at_the_store_not_per_operation() {
    // 1.5 * 0.00000001 = 0.000000015 (a tie at scale 8); * 100.0 gives
    // 0.0000015 exactly. Rounding after `*` would give 0.00000002 * 100.0.
    let lines = run_lines(
        "sit d64 a ← 1.5\nsit d64 r ← 0.00000001\n\
         sit d64 h ← 100.0\nsit d64 t ← a * r * h\nnota t",
    )
    .expect("runs");
    assert_eq!(lines, ["0.0000015"]);
}

#[test]
fn decimal_division_rounds_half_even_to_the_larger_scale() {
    // 1.0 / 3.0 at scale 8 is 0.33333333; 2.0 / 3.0 is 0.66666667.
    let lines = run_lines(
        "sit d64 one ← 1.0\nsit d64 two ← 2.0\nsit d64 three ← 3.0\n\
         sit d64 q ← one / three\nnota q\nsit d64 w ← two / three\nnota w",
    )
    .expect("runs");
    assert_eq!(lines, ["0.33333333", "0.66666667"]);
}

#[test]
fn decimal_intermediate_may_leave_the_range_until_the_store() {
    // big + 1.0 - 1.0 passes through 92233720369.0, past the d64 range, and
    // lands back inside it; big * 2.0 lands outside and traps at the store.
    let lines = run_lines(
        "sit d64 big ← 92233720368.0\nsit d64 one ← 1.0\n\
         sit d64 ok ← big + one - one\nnota ok",
    )
    .expect("runs");
    assert_eq!(lines, ["92233720368"]);
    let trap = run_lines(
        "sit d64 big ← 92233720368.0\nsit d64 two ← 2.0\n\
         sit d64 boom ← big * two\nnota boom",
    )
    .expect_err("out of range store traps");
    assert!(trap.contains("implicit_store_out_of_range"), "{trap}");
    assert!(trap.contains("184467440736"), "{trap}");
}

#[test]
fn decimal_comparison_of_intermediates_is_exact() {
    let lines = run_lines(
        "sit d64 p ← 19.99\nsit d64 r ← 0.0725\n\
         nota p * r * r ≻ p * r\nnota p * r * r ≺ p * r\nnota p * r * r ≅ p * r * r",
    )
    .expect("runs");
    assert_eq!(lines, ["falsum", "verum", "verum"]);
}

#[test]
fn decimal_store_positions_all_round_at_the_slot() {
    // 0.0725^3 = 0.000381078125 (scale 12) stores as 0.00038108 (38108):
    // through a return, an argument, an assignment, an inferred slot and a
    // list element. Unstored, it prints exactly.
    let mut host = crate::mir::BufferHost::default();
    crate::mir::run_source(
        &Session::new(la_config().with_target(Target::HirTypeScript)),
        "decimal-store-positions.fab",
        r"functio cubus(d64 x) → d64 {
    redde x * x * x
}
functio idem(d64 x) → d64 {
    redde x
}
incipit {
    sit d64 r ← 0.0725
    nota cubus(r)
    nota idem(r * r * r)
    varia d64 v ← 1.0
    v ← r * r * r
    nota v
    sit _ w ← r * r * r
    nota w
    sit lista<d64> xs ← [r * r * r]
    nota xs[0]
    nota r * r * r
}",
        &mut host,
    )
    .unwrap_or_else(|error| panic!("store positions must run: {error:?}"));
    assert_eq!(
        host.stdout_lines,
        [
            "0.00038108",
            "0.00038108",
            "0.00038108",
            "0.00038108",
            "0.00038108",
            "0.000381078125"
        ]
    );
}

// ──────────────────────────────────────────────────────────────────────
// VEC-5a: a constant index on a vector is a lane projection, never an Index
// projection (a vector is a register value, not a storage buffer).
// ──────────────────────────────────────────────────────────────────────

const VECTOR_LANE_SOURCE: &str = r"
functio lane(vector<f32, magnitudo 3> v) → f32 {
    redde v[1]
}
functio asserted(vector<f32, magnitudo 3> ∪ nihil v) → f32 {
    redde v![2]
}
functio write(vector<f32, magnitudo 3> v) → vector<f32, magnitudo 3> {
    varia vector<f32, magnitudo 3> u ← v
    u[0] ← 4.0
    redde u
}
";

#[test]
fn vector_constant_index_lowers_to_a_lane_projection() {
    let dump = dump_source(VECTOR_LANE_SOURCE);
    for lane in ["lane1", "lane2", "lane0"] {
        assert!(dump.contains(&format!(".{lane}")), "missing .{lane} in:\n{dump}");
    }
    assert!(
        !dump.contains("[1]") && !dump.contains("[2]"),
        "vector lane lowered as Index:\n{dump}"
    );
}

// ──────────────────────────────────────────────────────────────────────
// VEC-5b: a run-time index on a vector is the checked lane select, an Index
// projection on the vector base (a constant lane stays VectorLane).
// ──────────────────────────────────────────────────────────────────────

const VECTOR_LANE_SELECT_SOURCE: &str = r"
functio pick(vector<f32, magnitudo 3> v, numerus i) → f32 {
    redde v[i]
}
functio store(numerus i) → vector<f32, magnitudo 3> {
    varia vector<f32, magnitudo 3> u ← [0.0, 0.0, 0.0] ↦ vector<f32, magnitudo 3>
    u[i] ← 4.0
    redde u
}
";

#[test]
fn vector_runtime_index_lowers_to_an_index_projection_on_the_vector() {
    let dump = dump_source(VECTOR_LANE_SELECT_SOURCE);
    for projection in ["return _0[_1]", "_1[_0] = "] {
        assert!(
            dump.contains(projection),
            "run-time vector index did not lower to an Index projection ({projection}):\n{dump}"
        );
    }
    assert!(!dump.contains(".lane"), "run-time index became a constant lane:\n{dump}");
}

/// A device function lowers `⇇` to the output-buffer store the recipes already
/// write (one forwarding assign, no copy-into call); a host function keeps the
/// storage-preserving `TensorCopyInto`.
#[test]
fn a_device_function_lowers_copy_into_to_a_forwarding_store() {
    let dump = dump_source(
        "+++\nlocale = \"en\"\n+++\n@ kernel\n@ public\nfn k(tf32[16, 8] a, tf32[8, 16] b, mut tf32[16, 16] out, u32 id) → void {\n    out ⇇ a · b\n}\n",
    );

    assert_eq!(dump.matches("tensor_matmul").count(), 1, "{dump}");
    assert_eq!(
        dump.matches("tensor_copy_into").count(),
        0,
        "a device function has no copy-into call:\n{dump}"
    );
    assert!(dump.contains("_2 = %0"), "the result is stored into the output param:\n{dump}");
}

#[test]
fn a_host_function_keeps_the_storage_preserving_copy_into() {
    let dump = dump_source(
        r"functio copy(tf32[3, 3] a, varia tf32[3, 3] out) → vacuum {
    out ⇇ a
}",
    );

    assert_eq!(dump.matches("tensor_copy_into").count(), 1, "{dump}");
}

#[test]
fn bounded_append_inside_fac_cape_lowers_through_a_failable_helper() {
    // Need: a fixed-capacity `appende` is a bare runtime call with no
    // error edge, so a `fac … cape` directly around it never caught the
    // capacity failure. Inside a handler it must lower to a `try_call` of a
    // `textus`-channel helper whose error edge is the handler.
    let dump = dump_source(
        r"
functio exemplo() → vacuum {
    varia lista<numerus, magnitudo 2> xs ← [1, 2]
    fac { xs.appende(3) } cape err { nota err }
}
",
    );

    assert!(
        dump.contains("try_call"),
        "the append reaches the handler through a try_call: {dump}"
    );
    assert!(
        dump.contains("function f1 -> ty#") && dump.contains("append"),
        "the synthesized helper performs the append: {dump}"
    );
}

#[test]
fn ascribed_json_object_literal_constructs_the_typed_map() {
    let dump = dump_source(
        r#"+++
locale = "en"
+++
main {
    let _ a ← {"a": 1} ∷ map<string, int>
    let _ b ← {"a": null} ∷ map<string, int ∪ none>
    print a
    print b
}
"#,
    );
    assert_eq!(
        dump.matches("construct map:").count(),
        2,
        "each ascribed JSON object literal must build the ascribed map: {dump}"
    );
}

#[test]
fn missing_array_row_is_a_named_lowering_error_not_a_dropped_statement() {
    // Loop and reduction lowering reach `lista<T>` rows through
    // `require_array`; a bare `find_array(..)?` returned `None` with no
    // diagnostic and the enclosing loop vanished from MIR. Lowering
    // pre-interns the rows source programs need, so the missing row is pinned
    // at the helper every loop site shares.
    let types = TypeTable::new();
    let fractus = types.primitive(Primitive::Fractus);
    assert!(types.find_array(fractus).is_none(), "the fixture row must start absent");
    let mut builder = FunctionBuilder::new(&types);
    assert_eq!(
        builder.require_array(fractus, Span::default(), "itera cursor element list"),
        None
    );
    assert_eq!(builder.errors.len(), 1, "the miss must push exactly one error");
    assert_eq!(
        builder.errors[0].issue,
        "missing_type_information_for_mir_lowering_itera_cursor_element_list"
    );

    let mut types = TypeTable::new();
    let fractus = types.primitive(Primitive::Fractus);
    let array = types.array(fractus);
    let mut builder = FunctionBuilder::new(&types);
    assert_eq!(
        builder.require_array(fractus, Span::default(), "itera cursor element list"),
        Some(array)
    );
    assert!(builder.errors.is_empty(), "the happy path pushes nothing");
}

#[test]
fn thread_return_is_refused_at_mir_lowering_by_name_until_lane_lowering_lands() {
    let unit = analyze(
        r"
@ nucleum
@ publica
functio k(in tensor<f32, [4]> out, de tensor<f32, [4]> src) → vacuum {
    itera ex src apud [i] filum lane sit value {
        redde filum
    }
}
",
    );
    let errors = lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default())
        .expect_err("`redde filum` must not be mislowered");
    assert!(
        errors
            .iter()
            .any(|error| error.issue == "return_thread_lowering_pending"),
        "{:?}",
        errors.iter().map(|error| &error.issue).collect::<Vec<_>>()
    );
}
