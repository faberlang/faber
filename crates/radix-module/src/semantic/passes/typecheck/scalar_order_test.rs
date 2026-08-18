//! Typecheck acceptance and rejection probes for scalar `textus`/`ascii` ordering (Seam B-lite).

use crate::{diagnostics::DiagnosticArg, Compiler, Config, Target};

fn compile(source: &str) -> crate::CompileResult {
    Compiler::new(Config::default().with_target(Target::HirRust)).compile_str("test.fab", source)
}

#[cfg(feature = "hir-rust")]
fn accepts(result: &crate::CompileResult) {
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
}

fn rejects_with_issue(result: &crate::CompileResult, issue: &str) {
    assert!(!result.success(), "expected failure, got: {:?}", result.diagnostics);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diag| diag.args == vec![DiagnosticArg::new("issue", issue)]),
        "expected diagnostic issue {issue:?}, got: {:?}",
        result.diagnostics
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn scalar_ascii_literals_order_typechecks() {
    accepts(&compile("incipit { si 'a' ≤ 'z' { nota \"ok\" } }"));
}

#[test]
fn full_cord_text_order_rejects() {
    rejects_with_issue(
        &compile("incipit { si \"ab\" < \"ac\" { nota \"bad\" } }"),
        "numeric_operands_required",
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn sectio_binding_scalar_order_typechecks() {
    accepts(&compile(
        r#"
incipit {
    fixum textus s ← "az"
    fixum textus c ← s.sectio(0, 1)
    si c ≥ 'a' { nota "ok" }
}
"#,
    ));
}

#[test]
fn unary_textus_predicate_closure_scalar_order_rejects_without_interval_context() {
    // WHY: scalar closure marking is limited to retenta/expurgata interval helpers
    // (DEFER-047), not every `(textus) → bivalens` parameter.
    rejects_with_issue(
        &compile(
            r#"
functio keep(textus s, (textus) → bivalens pred) → textus {
    redde s
}

incipit {
    fixum textus out ← keep("abc", textus c ∴ c ≥ 'a' aut c ≤ 'z')
}
"#,
        ),
        "numeric_operands_required",
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn interval_helper_native_body_scalar_order_typechecks() {
    accepts(&compile(
        r#"
    functio retenta(textus s, textus infimum sponte, textus supremum sponte) → textus {
        varia lista<textus> kept ← vacua
        varia numerus i ← 0
        dum i < s.longitudo() {
            fixum textus c ← s.sectio(i, i + 1)
            si c ≥ infimum et c ≤ supremum ergo kept.appende(c)
            i ← i + 1
        }
        redde ""
    }
"#,
    ));
}

#[test]
fn interval_helper_rejects_multi_scalar_bounds() {
    let result = compile(
        r#"
functio retenta(textus s, textus infimum sponte, textus supremum sponte) → textus {
    redde s
}

incipit {
    nota retenta("hello", "ab", "z")
}
"#,
    );
    rejects_with_issue(&result, "interval_bound_single_scalar");
    assert!(result
        .diagnostics
        .iter()
        .any(|diag| { diag.args == vec![DiagnosticArg::new("issue", "interval_bound_single_scalar")] }));
}

#[cfg(feature = "hir-rust")]
#[test]
fn interval_helper_accepts_ascii_literal_bounds() {
    accepts(&compile(
        r#"
functio retenta(textus s, textus infimum sponte, textus supremum sponte) → textus {
    redde s
}

incipit {
    nota retenta("hello", 'a', 'z')
}
"#,
    ));
}

// English-canonical chorda names (keep_if/keep_between/drop_if/drop_between)
// must receive the same single-scalar text markings as the Latin names.

#[cfg(feature = "hir-rust")]
#[test]
fn en_interval_helper_native_body_scalar_order_typechecks() {
    accepts(&compile(
        r#"
    functio keep_between(textus s, textus infimum sponte, textus supremum sponte) → textus {
        varia lista<textus> kept ← vacua
        varia numerus i ← 0
        dum i < s.longitudo() {
            fixum textus c ← s.sectio(i, i + 1)
            si c ≥ infimum et c ≤ supremum ergo kept.appende(c)
            i ← i + 1
        }
        redde ""
    }
"#,
    ));
}

#[cfg(feature = "hir-rust")]
#[test]
fn en_interval_helper_accepts_ascii_literal_bounds() {
    accepts(&compile(
        r#"
functio drop_between(textus s, textus infimum sponte, textus supremum sponte) → textus {
    redde s
}

incipit {
    nota drop_between("hello", 'a', 'z')
}
"#,
    ));
}

#[test]
fn en_interval_helper_rejects_multi_scalar_bounds() {
    rejects_with_issue(
        &compile(
            r#"
functio keep_between(textus s, textus infimum sponte, textus supremum sponte) → textus {
    redde s
}

incipit {
    nota keep_between("hello", "ab", "z")
}
"#,
        ),
        "interval_bound_single_scalar",
    );
}

#[test]
fn interval_helper_rejects_multi_char_bounded_ascii_bound() {
    rejects_with_issue(
        &compile(
            r#"
functio retenta(textus s, textus infimum sponte, textus supremum sponte) → textus {
    redde s
}

functio go(ascii<8> bound) {
    nota retenta("hello", bound, 'z')
}
"#,
        ),
        "argument_type_mismatch",
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn char_literal_assigns_to_textus_slot() {
    accepts(&compile("incipit { fixum textus t ← 'a' }"));
}

#[cfg(feature = "hir-rust")]
#[test]
fn en_predicate_helper_closure_scalar_order_typechecks() {
    accepts(&compile(
        r#"
functio keep_if(textus s, (textus) → bivalens pred) → textus {
    redde s
}

incipit {
    fixum textus out ← keep_if("abc", textus c ∴ c ≥ 'a' aut c ≤ 'z')
}
"#,
    ));
}

// SEM011 / BB-2: char literals are BoundedAscii{1}. Relational compare is
// code-point order for char pairs; mixed char/string stays rejected unless the
// live single-scalar convention already accepts it.

#[cfg(feature = "hir-rust")]
#[test]
fn bounded_ascii_one_literals_ge_typechecks() {
    accepts(&compile("incipit { si 'c' ≥ 'a' { nota \"ok\" } }"));
}

#[cfg(feature = "hir-rust")]
#[test]
fn ascii_one_params_relational_typechecks() {
    accepts(&compile(
        r#"
functio after(ascii<1> left, ascii<1> right) → bivalens {
    redde left ≥ right
}
"#,
    ));
}

#[cfg(feature = "hir-rust")]
#[test]
fn bare_ascii_params_relational_typechecks() {
    accepts(&compile(
        r#"
functio after(ascii left, ascii right) → bivalens {
    redde left ≥ right
}
"#,
    ));
}

#[cfg(feature = "hir-rust")]
#[test]
fn mixed_ascii_and_ascii_one_relational_typechecks() {
    accepts(&compile(
        r#"
functio after(ascii left, ascii<1> right) → bivalens {
    redde left ≥ right
}
"#,
    ));
}

#[test]
fn char_to_multi_char_string_relational_rejects() {
    rejects_with_issue(
        &compile("incipit { si 'a' ≥ \"hello\" { nota \"bad\" } }"),
        "numeric_operands_required",
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn char_to_single_char_string_follows_scalar_convention() {
    accepts(&compile("incipit { si 'a' ≥ \"x\" { nota \"ok\" } }"));
}

#[test]
fn numerus_to_char_relational_rejects() {
    rejects_with_issue(&compile("incipit { si 1 ≥ 'a' { nota \"bad\" } }"), "numeric_operands_required");
}

#[test]
fn char_to_numerus_relational_rejects() {
    rejects_with_issue(&compile("incipit { si 'a' ≥ 1 { nota \"bad\" } }"), "numeric_operands_required");
}

#[test]
fn multi_char_ascii_literal_relational_stays_rejected() {
    rejects_with_issue(
        &compile("incipit { si 'ab' ≥ 'cd' { nota \"bad\" } }"),
        "numeric_operands_required",
    );
}
