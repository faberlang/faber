//! Coverage for check-time kernel legality (gpu-reset rule 12).
//!
//! Every rejection asserts the structured `issue` and its named args, never
//! rendered prose. Acceptance cases assert that the whole analysis is clean,
//! so a legal kernel is proven legal rather than merely un-flagged.

use super::kernel_legality::check;
use crate::test_support::{analyze_source, assert_accepts};
use crate::{SemanticError, SemanticErrorKind};
use radix_hir::HirItemKind;

const HOST_TYPE: &str = "nucleum_host_type";

fn kernel_errors(source: &str) -> Vec<SemanticError> {
    analyze_source(source)
        .diagnostics
        .iter()
        .filter(|err| {
            err.is_error()
                && err
                    .issue()
                    .is_some_and(|issue| issue.starts_with("nucleum_"))
        })
        .cloned()
        .collect()
}

fn arg<'a>(err: &'a SemanticError, name: &str) -> Option<&'a str> {
    err.args
        .iter()
        .find(|arg| arg.name == name)
        .map(|arg| arg.value.as_str())
}

/// Find the kernel diagnostic for `issue` whose args include every pair.
fn expect_kernel_error(source: &str, issue: &str, args: &[(&str, &str)]) -> SemanticError {
    let errors = kernel_errors(source);
    errors
        .iter()
        .find(|err| {
            err.issue() == Some(issue)
                && args
                    .iter()
                    .all(|(name, value)| arg(err, name) == Some(*value))
        })
        .cloned()
        .unwrap_or_else(|| panic!("missing {issue} {args:?} in {errors:?}"))
}

// ---------------------------------------------------------------------------
// Legal kernels
// ---------------------------------------------------------------------------

#[test]
fn structured_control_flow_in_a_kernel_loop_is_legal() {
    // The q11 shape: an `si` inside a kernel loop is a structured form.
    assert_accepts(
        r"
@ nucleum
@ publica
functio relu(tensor<f32, [8]> x, varia tensor<f32, [8]> out) → vacuum {
    itera ex x apud [i] sit v {
        si v ≻ 0.0 {
            out[i] ← v
        } secus {
            out[i] ← 0.0
        }
    }
}
",
    );
}

#[test]
fn kernels_may_call_kernels() {
    assert_accepts(
        r"
@ nucleum
functio bump(f32 x) → f32 {
    redde x + 1.0
}

@ nucleum
@ publica
functio caller(tensor<f32, [8]> x, varia tensor<f32, [8]> out) → vacuum {
    itera ex x apud [i] sit v {
        out[i] ← bump(v)
    }
}
",
    );
}

#[test]
fn bounded_static_scratch_and_value_types_are_legal() {
    assert_accepts(
        r"
genus Pair {
    varia f32 a
    varia f32 b
}

@ nucleum
@ publica
functio scratch(f32 x) → f32 {
    sit Pair p ← Pair { a = x, b = x }
    sit f32 y ← p.a + p.b
    redde y
}
",
    );
}

#[test]
fn a_bounded_kernel_local_collection_is_legal() {
    assert_accepts(
        r"
@ nucleum
@ publica
functio scratch(f32 x) → f32 {
    sit lista<f32, magnitudo 4> s ← [x, x, x, x]
    redde s[0]
}
",
    );
}

#[test]
fn a_text_literal_selector_on_a_device_intrinsic_is_legal() {
    assert_accepts(
        r#"
@ nucleum
@ publica
functio swizzled(f32 x) → f32 {
    sit vector<f32, magnitudo 4> a ← [x, 2.0, 3.0, 4.0] ↦ vector<f32, magnitudo 4>
    sit vector<f32, magnitudo 4> b ← a.swizzle("wzyx")
    redde x
}
"#,
    );
}

#[test]
fn a_host_function_may_call_a_public_kernel() {
    // Rule 9 amendment: a host call to a public kernel is an ordinary call.
    assert_accepts(
        r"
@ nucleum
@ publica
functio bump(f32 x) → f32 {
    redde x + 1.0
}

functio host() → f32 {
    redde bump(1.0)
}
",
    );
}

#[test]
fn recursion_is_legal_and_reported_as_a_fact() {
    let fixture = analyze_source(
        r"
@ nucleum
functio down(i32 n) → i32 {
    si n ≤ 0 {
        redde 0
    }
    secus {
        redde down(n - 1)
    }
}

@ nucleum
functio flat(i32 n) → i32 {
    redde n
}
",
    );
    assert!(
        fixture.is_ok(),
        "recursion must not be a legality error: {:?}",
        fixture.diagnostics
    );
    let hir = fixture.hir.as_ref().expect("hir");
    let report = check(
        hir,
        &fixture.resolver,
        &fixture.types,
        &fixture.interner,
        &fixture.tensor_edge_policies,
    );
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let names: Vec<&str> = hir
        .items
        .iter()
        .filter(|item| report.recursive_kernels.contains(&item.def_id))
        .filter_map(|item| match &item.kind {
            HirItemKind::Function(function) => Some(fixture.interner.resolve(function.name)),
            _ => None,
        })
        .collect();
    assert_eq!(names, ["down"]);
}

// ---------------------------------------------------------------------------
// Host effects
// ---------------------------------------------------------------------------

#[test]
fn console_output_in_a_kernel_is_rejected() {
    let err = expect_kernel_error(
        r"
@ nucleum
@ publica
functio noisy(f32 x) → vacuum {
    nota x
}
",
        "nucleum_host_effect",
        &[("operation", "scribe")],
    );
    assert_eq!(err.kind, SemanticErrorKind::LoweringError);
}

#[test]
fn a_conversation_call_in_a_kernel_is_rejected() {
    expect_kernel_error(
        r"
@ ad 'numeri:bis'
functio bis(numerus n) → numerus {
    redde n * 2
}

@ nucleum
@ publica
functio talk(numerus n) → vacuum {
    sit sermo s ← ad 'numeri:bis' (n)
}
",
        "nucleum_host_effect",
        &[("operation", "ad")],
    );
}

// ---------------------------------------------------------------------------
// Async and generator postures
// ---------------------------------------------------------------------------

#[test]
fn an_async_kernel_is_rejected() {
    expect_kernel_error(
        r"
@ nucleum
@ publica
functio later(f32 x) fiet → f32 {
    redde x
}
",
        "nucleum_async",
        &[("construct", "async_function")],
    );
}

#[test]
fn a_generator_kernel_is_rejected() {
    expect_kernel_error(
        r"
@ nucleum
@ publica
functio stream(f32 x) fiunt → f32 {
    cede x
}
",
        "nucleum_async",
        &[("construct", "generator_function")],
    );
}

// ---------------------------------------------------------------------------
// Host-only types
// ---------------------------------------------------------------------------

#[test]
fn unbounded_collections_and_text_are_rejected_as_parameters() {
    for (ty, family) in [
        ("lista<f32>", "lista"),
        ("textus", "textus"),
        ("tabula<textus, f32>", "tabula"),
        ("copia<numerus>", "copia"),
    ] {
        let source =
            format!("@ nucleum\n@ publica\nfunctio k({ty} v) → vacuum {{\n    tacet\n}}\n");
        expect_kernel_error(
            &source,
            HOST_TYPE,
            &[("family", family), ("site", "parameter")],
        );
    }
}

#[test]
fn a_u32_index_vector_parameter_feeding_gather_is_rejected() {
    // Rule 12.3 has no carve-out: an unbounded `lista<u32>` is a host
    // collection even when it feeds `tensor.gather`.
    // `gather` takes `tensor<u32, [n]>`, so the `lista<u32>` parameter is a
    // type error at the call (the legality pass never sees an exemption to
    // grant); the same parameter unused is SEM008 in the test below.
    let result = analyze_source(
        r"
@ nucleum
@ publica
functio pick(tf32[8, 4] table, lista<u32> ids) → tf32[1, 4] {
    redde table.gather(ids)
}
",
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|err| err.is_error() && err.issue() == Some("argument_type_mismatch")),
        "expected argument_type_mismatch on the lista gather ids, got {:?}",
        result.diagnostics
    );
}

#[test]
fn a_tensor_u32_ids_parameter_feeding_gather_is_a_legal_buffer() {
    assert_accepts(
        r"
@ nucleum
@ publica
functio pick(tf32[8, 4] table, tensor<u32, [1]> ids) → tf32[1, 4] {
    redde table.gather(ids)
}
",
    );
}

#[test]
fn an_index_vector_parameter_not_feeding_gather_is_still_rejected() {
    expect_kernel_error(
        r"
@ nucleum
@ publica
functio pick(tf32[8, 4] table, lista<u32> ids) → tf32[8, 4] {
    redde table
}
",
        HOST_TYPE,
        &[("family", "lista"), ("site", "parameter")],
    );
}

#[test]
fn a_host_typed_local_is_rejected() {
    expect_kernel_error(
        r"
@ nucleum
@ publica
functio k(f32 x) → f32 {
    sit lista<f32> xs ← [x, x]
    redde x
}
",
        HOST_TYPE,
        &[("family", "lista"), ("site", "local")],
    );
}

#[test]
fn a_host_typed_return_is_rejected() {
    expect_kernel_error(
        r#"
@ nucleum
@ publica
functio k() → textus {
    redde "x"
}
"#,
        HOST_TYPE,
        &[("family", "textus"), ("site", "return")],
    );
}

#[test]
fn a_genus_with_an_illegal_member_is_rejected() {
    expect_kernel_error(
        r"
genus Label {
    varia textus name
}

@ nucleum
@ publica
functio k(Label l) → vacuum {
    tacet
}
",
        HOST_TYPE,
        &[("family", "textus"), ("site", "parameter")],
    );
}

#[test]
fn dynamic_value_types_are_rejected() {
    for (ty, family) in [("valor", "valor"), ("json", "json")] {
        let source =
            format!("@ nucleum\n@ publica\nfunctio k({ty} v) → vacuum {{\n    tacet\n}}\n");
        expect_kernel_error(
            &source,
            HOST_TYPE,
            &[("family", family), ("site", "parameter")],
        );
    }
}

#[test]
fn user_visible_atomics_are_rejected() {
    expect_kernel_error(
        r"
@ nucleum
@ publica
functio k(atomic<i32> cell) → vacuum {
    tacet
}
",
        "nucleum_workgroup",
        &[("construct", "atomic")],
    );
}

// ---------------------------------------------------------------------------
// Calls
// ---------------------------------------------------------------------------

#[test]
fn a_call_to_an_ordinary_function_is_rejected() {
    expect_kernel_error(
        r"
functio host(f32 x) → f32 {
    redde x * 2.0
}

@ nucleum
@ publica
functio k(f32 x) → f32 {
    redde host(x)
}
",
        "nucleum_host_call",
        &[("callee", "host"), ("reason", "not_a_kernel")],
    );
}

#[test]
fn a_call_through_an_interface_value_is_rejected() {
    expect_kernel_error(
        r"
implendum Numerabile {
    functio valor() → f32
}

genus Unus implet Numerabile {
    functio valor() → f32 {
        redde 1.0
    }
}

@ nucleum
@ publica
functio k(Numerabile n) → f32 {
    redde n.valor()
}
",
        "nucleum_host_call",
        &[("callee", "pactum"), ("reason", "dynamic")],
    );
}

#[test]
fn a_private_kernel_cannot_be_called_from_host_code() {
    expect_kernel_error(
        r"
@ nucleum
functio helper(f32 x) → f32 {
    redde x + 1.0
}

functio host() → f32 {
    redde helper(1.0)
}
",
        "nucleum_private_host_call",
        &[("callee", "helper")],
    );
}

// ---------------------------------------------------------------------------
// Local recovery
// ---------------------------------------------------------------------------

#[test]
fn a_local_handler_in_a_kernel_is_rejected() {
    expect_kernel_error(
        r"
@ nucleum
@ publica
functio k(f32 x) → f32 {
    fac {
        redde x
    } cape err {
        redde 0.0
    }
    redde x
}
",
        "nucleum_recovery",
        &[("construct", "cape")],
    );
}

#[test]
fn a_call_default_in_a_kernel_selects_a_value_and_is_legal() {
    // Operator ruling 2026-10-08: `⊥` is compare plus select, never a handler.
    assert_accepts(
        r"
@ nucleum
@ publica
functio k(tensor<f32, [8]> x, varia tensor<f32, [8]> out) → vacuum {
    itera ex x apud [i] filum f sit v {
        out[i] ← x.accipe([i + 1]) ⊥ 0.0
    }
}
",
    );
}

#[test]
fn a_conversion_default_in_a_kernel_selects_a_value_and_is_legal() {
    // Operator ruling 2026-10-08 (KCONVDEF): `n ↦ i16 ⊥ 0` is compare plus
    // select, never a handler.
    assert_accepts(
        r"
@ nucleum
@ publica
functio k(i32 n) → i16 {
    redde n ↦ i16 ⊥ 0
}
",
    );
}

#[test]
fn a_local_do_catch_is_still_a_recovery_next_to_a_call_default() {
    expect_kernel_error(
        r"
@ nucleum
@ publica
functio k(tensor<f32, [8]> x, numerus n) → f32 {
    sit f32 d ← x.accipe([n]) ⊥ 0.0
    fac {
        redde d
    } cape err {
        redde 0.0
    }
    redde d
}
",
        "nucleum_recovery",
        &[("construct", "cape")],
    );
}

// ---------------------------------------------------------------------------
// Error channel (operator ruling 2026-10-08: `⇥ text` and nothing else)
// ---------------------------------------------------------------------------

#[test]
fn a_kernel_may_declare_a_text_error_channel() {
    assert_accepts(
        r"
@ nucleum
@ publica
functio k(f32 x) → f32 ⇥ textus {
    si x ≺ 0.0 {
        iace 'negative'
    }
    redde x
}
",
    );
}

#[test]
fn a_typed_error_channel_on_a_kernel_is_a_named_check_error() {
    expect_kernel_error(
        r"
@ nucleum
@ publica
functio k(f32 x) → f32 ⇥ numerus {
    si x ≺ 0.0 {
        iace 1
    }
    redde x
}
",
        "nucleum_typed_error_channel",
        &[("kernel", "k")],
    );
}

// ---------------------------------------------------------------------------
// Raw lane ids (gpu-reset rule 12.5)
// ---------------------------------------------------------------------------

const RAW_LANE_ID: &str = "nucleum_raw_lane_id";

/// The local declaration that binds one builtin call with its natural type.
fn lane_id_local(builtin: radix_types::MirKernelBuiltin) -> String {
    let ty = if builtin.returns_vector() {
        "vector<u32, magnitudo 3>"
    } else {
        "u32"
    };
    format!("sit {ty} lane ← {}()", builtin.spelling())
}

#[test]
fn every_thread_id_builtin_is_rejected_in_a_kernel_body() {
    for builtin in radix_types::MirKernelBuiltin::ALL {
        let source = format!(
            "@ nucleum\n@ publica\nfunctio k(f32 x) → f32 {{\n    {}\n    redde x\n}}\n",
            lane_id_local(builtin)
        );
        expect_kernel_error(&source, RAW_LANE_ID, &[("builtin", builtin.spelling())]);
    }
}

#[test]
fn every_thread_id_builtin_is_rejected_in_a_fragment_body() {
    for builtin in radix_types::MirKernelBuiltin::ALL {
        let source = format!(
            "@ nucleum fragment\nfunctio helper(f32 x) → f32 {{\n    {}\n    redde x\n}}\n",
            lane_id_local(builtin)
        );
        expect_kernel_error(&source, RAW_LANE_ID, &[("builtin", builtin.spelling())]);
    }
}

#[test]
fn a_thread_id_builtin_is_rejected_inside_a_kernel_loop() {
    expect_kernel_error(
        r"
@ nucleum
@ publica
functio k(tensor<u32, [8]> x, tensor<u32, [8]> out) → vacuum {
    itera ex x apud [i] sit v {
        out[global_invocation_x()] ← v
    }
}
",
        RAW_LANE_ID,
        &[("builtin", "global_invocation_x")],
    );
}

#[test]
fn no_user_code_accepts_a_thread_id_builtin() {
    for builtin in radix_types::MirKernelBuiltin::ALL {
        let local = lane_id_local(builtin);
        // Host code: the typechecker's device-only rule rejects it.
        let host = analyze_source(&format!(
            "functio host() → f32 {{\n    {local}\n    redde 1.0\n}}\n"
        ));
        assert!(
            host.diagnostics
                .iter()
                .any(|err| err.is_error()
                    && err.issue() == Some("gpu_builtin_device_function_required")),
            "{}: {:?}",
            builtin.spelling(),
            host.diagnostics
        );
        // Kernel and fragment bodies: the legality rule rejects it.
        for header in ["@ nucleum\n@ publica", "@ nucleum fragment"] {
            let source =
                format!("{header}\nfunctio k(f32 x) → f32 {{\n    {local}\n    redde x\n}}\n");
            assert!(
                !analyze_source(&source).is_ok(),
                "{} accepted under `{header}`",
                builtin.spelling()
            );
        }
    }
}

const RAW_LANE_ID_PARAM: &str = "nucleum_raw_lane_id_param";
const POSITIONAL_LANE_PARAM: &str = "positional_lane_param";

fn lane_warnings(source: &str) -> Vec<SemanticError> {
    analyze_source(source)
        .diagnostics
        .iter()
        .filter(|err| !err.is_error() && err.issue() == Some(POSITIONAL_LANE_PARAM))
        .cloned()
        .collect()
}

#[test]
fn a_kernel_parameter_named_after_every_thread_id_builtin_is_rejected() {
    for builtin in radix_types::MirKernelBuiltin::ALL {
        let ty = if builtin.returns_vector() {
            "vector<u32, magnitudo 3>"
        } else {
            "u32"
        };
        let source = format!(
            "@ nucleum\n@ publica\nfunctio k(tensor<u32, [4]> input, tensor<u32, [4]> output, {ty} {}) → vacuum {{\n}}\n",
            builtin.spelling()
        );
        let err = expect_kernel_error(
            &source,
            RAW_LANE_ID_PARAM,
            &[
                ("builtin", builtin.spelling()),
                ("parameter", builtin.spelling()),
            ],
        );
        assert_eq!(err.kind, SemanticErrorKind::LoweringError);
    }
}

#[test]
fn a_builtin_named_parameter_anywhere_in_the_list_is_rejected() {
    expect_kernel_error(
        r"
@ nucleum
@ publica
functio k(tensor<u32, [4]> input, u32 workgroup_id_y, tensor<u32, [4]> output) → vacuum {
}
",
        RAW_LANE_ID_PARAM,
        &[
            ("builtin", "workgroup_id_y"),
            ("parameter", "workgroup_id_y"),
        ],
    );
}

#[test]
fn a_builtin_named_parameter_is_not_also_warned_as_positional() {
    let source = r"
@ nucleum
@ publica
functio k(tensor<u32, [4]> input, tensor<u32, [4]> output, u32 global_invocation_x) → vacuum {
}
";
    assert!(lane_warnings(source).is_empty());
}

#[test]
fn a_positional_trailing_u32_lane_parameter_of_any_name_warns() {
    for name in ["id", "i", "lane", "anything_at_all"] {
        let source = format!(
            "@ nucleum\n@ publica\nfunctio copy(tensor<u32, [4]> input, varia tensor<u32, [4]> output, u32 {name}) → vacuum {{\n    output[{name}] ← input[{name}]\n}}\n"
        );
        let warnings = lane_warnings(&source);
        assert_eq!(warnings.len(), 1, "{name}: {warnings:?}");
        assert_eq!(arg(&warnings[0], "parameter"), Some(name));
        assert_eq!(arg(&warnings[0], "kernel"), Some("copy"));
        assert_eq!(
            warnings[0].kind,
            SemanticErrorKind::Warning(crate::WarningKind::PositionalLaneParam)
        );
        // A warning never fails the analysis.
        assert!(analyze_source(&source).is_ok(), "{name}");
    }
}

#[test]
fn a_trailing_u32_that_is_not_a_lane_does_not_warn() {
    // A value-returning kernel takes its trailing `u32` as an ordinary input.
    let returning = r"
@ nucleum
@ publica
functio scale(tensor<u32, [4]> input, tensor<u32, [4]> other, u32 n) → u32 {
    redde n
}
";
    assert!(lane_warnings(returning).is_empty());
    // Two parameters cannot be `inputs.., output, index`.
    let short = r"
@ nucleum
@ publica
functio bump(tensor<u32, [4]> output, u32 n) → vacuum {
}
";
    assert!(lane_warnings(short).is_empty());
    // A non-`u32` trailing parameter is not a lane.
    let wide = r"
@ nucleum
@ publica
functio k(tensor<u32, [4]> input, tensor<u32, [4]> output, i32 n) → vacuum {
}
";
    assert!(lane_warnings(wide).is_empty());
}

#[test]
fn a_compiler_bound_lane_parameter_is_unaffected() {
    // The lane parameter is how the compiler itself binds a builtin input: no
    // user call is written, so the raw-lane-id rule has nothing to reject.
    let source = r"
@ nucleum
@ publica
functio copy(tensor<u32, [4]> input, varia tensor<u32, [4]> output, u32 i) → vacuum {
    output[i] ← input[i]
}
";
    assert!(
        kernel_errors(source).is_empty(),
        "{:?}",
        kernel_errors(source)
    );
    assert_accepts(source);
}

// ---------------------------------------------------------------------------
// Diagnostic identity
// ---------------------------------------------------------------------------

#[test]
fn kernel_diagnostics_carry_the_offending_span() {
    let source = "@ nucleum\n@ publica\nfunctio noisy(f32 x) → vacuum {\n    nota x\n}\n";
    let err = expect_kernel_error(source, "nucleum_host_effect", &[("operation", "scribe")]);
    let line_start = source.find("    nota x").expect("offending line");
    assert!(
        (err.span.start as usize) >= line_start
            && (err.span.start as usize) < line_start + "    nota x".len(),
        "span {:?} must point at the offending line",
        err.span
    );
}

// N7e D6: `trapping<float>` is a language rule through the host-type family
// mechanism; `saturating<float>` is a temporary named refusal.

#[test]
fn policy_float_trapping_cell_is_a_host_only_family_in_a_kernel() {
    for width in ["f16", "bf16", "f32", "f64"] {
        expect_kernel_error(
            &format!(
                r"
@ nucleum
@ publica
functio k(exactus<{width}> x) → vacuum {{
    tacet
}}
"
            ),
            HOST_TYPE,
            &[("family", "trapping<float>"), ("site", "parameter")],
        );
    }
}

#[test]
fn policy_float_saturating_cell_in_a_kernel_names_the_temporary_issue() {
    for site in ["parameter", "return", "local"] {
        let source = match site {
            "parameter" => {
                r"
@ nucleum
@ publica
functio k(saturatus<f32> x) → vacuum {
    tacet
}
"
            }
            "return" => {
                r"
@ nucleum
@ publica
functio k() → saturatus<f32> {
    redde 1.0
}
"
            }
            _ => {
                r"
@ nucleum
@ publica
functio k(f32 x) → f32 {
    sit saturatus<f32> y ← 1.0
    redde x
}
"
            }
        };
        expect_kernel_error(
            source,
            "nucleum_saturating_float_not_built",
            &[("site", site)],
        );
    }
}
