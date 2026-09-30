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
functio relu(tensor<f32, [8]> x, tensor<f32, [8]> out) → vacuum {
    itera ex x apud [i] fixum v {
        si v > 0.0 {
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
functio caller(tensor<f32, [8]> x, tensor<f32, [8]> out) → vacuum {
    itera ex x apud [i] fixum v {
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
    fixum Pair p ← Pair { a = x, b = x }
    fixum f32 y ← p.a + p.b
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
    fixum lista<f32, 4> s ← [x, x, x, x]
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
    fixum vector<f32, 4> a ← [x, 2.0, 3.0, 4.0] ↦ vector<f32, 4>
    fixum vector<f32, 4> b ← a.swizzle("wzyx")
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
functio down(numerus n) → numerus {
    si n ≤ 0 {
        redde 0
    }
    redde down(n - 1)
}

@ nucleum
functio flat(numerus n) → numerus {
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
    let report = check(hir, &fixture.resolver, &fixture.types, &fixture.interner);
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
    fixum sermo s ← ad 'numeri:bis' (n)
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
fn a_host_typed_local_is_rejected() {
    expect_kernel_error(
        r"
@ nucleum
@ publica
functio k(f32 x) → f32 {
    fixum lista<f32> xs ← [x, x]
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
