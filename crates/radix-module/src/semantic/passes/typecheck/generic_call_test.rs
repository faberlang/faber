//! Explicit type arguments at function and method call sites.

use crate::diagnostics::DiagnosticArg;
use crate::{Compiler, Config, Target};

fn compile(source: &str) -> crate::CompileResult {
    Compiler::new(Config::default().with_target(Target::HirRust)).compile_str("generic_call.fab", source)
}

#[cfg(feature = "hir-rust")]
fn accepts(source: &str) {
    let result = compile(source);
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
}

fn rejects_with_issue(source: &str, issue: &str) {
    let result = compile(source);
    assert!(!result.success(), "expected failure, got: {:?}", result.diagnostics);
    assert!(
        result.diagnostics.iter().any(|diag| diag
            .args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == issue)),
        "expected diagnostic issue {issue:?}, got: {:?}",
        result.diagnostics
    );
}

fn rejects_with_args(source: &str, args: &[DiagnosticArg]) {
    let result = compile(source);
    assert!(!result.success(), "expected failure, got: {:?}", result.diagnostics);
    assert!(
        result.diagnostics.iter().any(|diag| diag.args == args),
        "expected diagnostic with args {args:?}, got: {:?}",
        result.diagnostics
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn explicit_generic_call_binds_type_param() {
    accepts(
        r#"functio id<T>(T x) → T {
    redde x
}

incipit {
    fixum textus x ← id<textus>("salve")
    nota x
}"#,
    );
}

#[test]
fn explicit_generic_call_rejects_argument_mismatch() {
    rejects_with_issue(
        r#"functio id<T>(T x) → T {
    redde x
}

incipit {
    fixum _ x ← id<numerus>("salve")
}"#,
        "argument_type_mismatch",
    );
}

#[test]
fn explicit_generic_call_rejects_non_generic_callee() {
    rejects_with_issue(
        r#"functio plain(textus x) → textus {
    redde x
}

incipit {
    fixum _ x ← plain<textus>("salve")
}"#,
        "callee_type_args_unsupported",
    );
}

#[test]
fn explicit_generic_call_rejects_wrong_type_arg_arity() {
    rejects_with_issue(
        r"functio both<A, B>(A a, B b) → A {
    redde a
}

incipit {
    fixum _ x ← both<textus>(1, 2)
}",
        "wrong_number_of_type_arguments",
    );
}

#[test]
fn explicit_generic_call_rejects_infer_hole() {
    rejects_with_issue(
        r"functio id<T>(T x) → T {
    redde x
}

incipit {
    fixum _ x ← id<_>(42)
}",
        "explicit_type_arg_inference_unsupported",
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn constrained_generic_call_accepts_allowed_explicit_type_arg() {
    accepts(
        r#"@ radix typus T in textus lista<textus> octeti
functio load<T>(T value) → T {
    redde value
}

incipit {
    fixum textus body ← load<textus>("body")
    fixum lista<textus> lines ← load<lista<textus>>(["a", "b"])
    fixum octeti bytes ← load<octeti>(|ca fe|)
    nota body
    nota lines
    nota bytes
}"#,
    );
}

#[test]
fn constrained_generic_call_rejects_unsupported_explicit_type_arg() {
    rejects_with_args(
        r"@ radix typus T in textus lista<textus> octeti
functio load<T>(T value) → T {
    redde value
}

incipit {
    fixum numerus n ← load<numerus>(1)
    nota n
}",
        &[
            DiagnosticArg::new("issue", "generic_type_arg_domain"),
            DiagnosticArg::new("rejected", "numerus"),
            DiagnosticArg::new("param", "T"),
            DiagnosticArg::new("accepted", "textus, octeti, lista<textus>"),
        ],
    );
}

#[test]
fn constrained_generic_call_rejects_unsupported_inferred_type_arg() {
    rejects_with_args(
        r"@ radix typus T in textus lista<textus> octeti
functio load<T>(T value) → T {
    redde value
}

incipit {
    fixum numerus n ← load(1)
    nota n
}",
        &[
            DiagnosticArg::new("issue", "generic_type_arg_domain"),
            DiagnosticArg::new("rejected", "numerus"),
            DiagnosticArg::new("param", "T"),
            DiagnosticArg::new("accepted", "textus, octeti, lista<textus>"),
        ],
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn constrained_generic_call_accepts_repeated_and_sugar_domains_structurally() {
    accepts(
        r"@ radix typus T in i32 numerus<i32>
@ radix typus T in f32 fractus<f32>
functio id<T>(T x) → T {
    redde x
}

incipit {
    fixum numerus<i32> n ← id<numerus<i32>>(1 ∷ numerus<i32>)
    fixum fractus<f32> f ← id<fractus<f32>>(1.0 ∷ fractus<f32>)
    nota n
    nota f
}",
    );
}

#[test]
fn braced_radix_typus_rejects_empty_allowed_domain_even_when_unused() {
    rejects_with_args(
        r#"@ radix { param = T }
functio load<T>(T value) → T {
    redde value
}

incipit {
    nota "unused"
}"#,
        &[
            DiagnosticArg::new("issue", "empty_radix_typus_domain"),
            DiagnosticArg::new("annotation", "@ radix typus"),
            DiagnosticArg::new("radix_typus_braced", "@ radix { param = T, allowed = textus }"),
        ],
    );
}

#[test]
fn explicit_generic_call_rejects_template_application() {
    rejects_with_issue(
        r#"incipit {
    fixum _ s ← "§"<textus>("x")
}"#,
        "template_type_args",
    );
}

#[test]
fn explicit_generic_call_rejects_ignotum_callee() {
    rejects_with_args(
        r#"functio box(ignotum v) → ignotum {
    redde v
}

incipit {
    fixum ignotum callee ← box(1)
    callee<textus>("a")
}"#,
        &[DiagnosticArg::new("issue", "dynamic_callee_type_args")],
    );
}

#[test]
fn explicit_generic_call_rejects_inferred_callee() {
    rejects_with_args(
        r"incipit {
    varia _ callee
    callee<textus>(1)
}",
        &[DiagnosticArg::new("issue", "inferred_callee_type_args")],
    );
}

#[test]
fn explicit_generic_call_rejects_ignotum_receiver_method() {
    rejects_with_args(
        r"functio box(ignotum v) → ignotum {
    redde v
}

incipit {
    fixum ignotum receiver ← box(1)
    receiver.meth<textus>()
}",
        &[DiagnosticArg::new("issue", "dynamic_receiver_method_type_args")],
    );
}

#[test]
fn explicit_generic_call_rejects_inferred_receiver_method() {
    rejects_with_args(
        r"incipit {
    varia _ receiver
    receiver.meth<textus>()
}",
        &[DiagnosticArg::new("issue", "method_call_type_args")],
    );
}

#[test]
fn explicit_generic_call_rejects_intrinsic_method_type_args() {
    rejects_with_args(
        r"incipit {
    fixum lista<numerus> xs ← [1, 2, 3]
    fixum _ n ← xs.longitudo<textus>()
}",
        &[DiagnosticArg::new("issue", "intrinsic_method_type_args")],
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn sermo_tuus_typed_view_typechecks_in_iteration() {
    accepts(
        r#"incipit {
    fixum sermo s ← ad 'runtime:echo' ("salve")
    fixum tuus<textus> t ← s.tuus<textus>()
    itera ex t.cursor() fixum frame {
        adfirma frame.data ≡ "salve"
    }
}"#,
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn sermo_tuus_bare_view_defaults_to_valor_frames() {
    accepts(
        r#"incipit {
    fixum sermo s ← ad 'runtime:echo' ("salve")
    fixum tuus<valor> t ← s.tuus()
    itera ex t.cursor() fixum frame {
        fixum valor data ← frame.data
        nota data
    }
}"#,
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn sermo_directional_live_methods_typecheck() {
    accepts(
        r#"incipit {
    fixum sermo s ← ad 'runtime:echo' ("salve")
    fixum meus<textus> m ← s.meus<textus>()
    fixum tuus<textus> t ← s.tuus<textus>()
    m.da("salve")
    fixum scrinium<textus> ∪ nihil reply ← t.accipe()
    fixum textus drained ← t.exhauri()
    fixum status outbound ← m.fini()
    fixum status inbound ← t.fini()
    nota reply
    nota drained
    nota outbound
    nota inbound
}"#,
    );
}

#[test]
fn sermo_tuus_rejects_more_than_one_payload_type_arg() {
    rejects_with_args(
        r#"incipit {
    fixum sermo s ← ad 'runtime:echo' ("salve")
    fixum _ frames ← s.tuus<textus, numerus>()
}"#,
        &[
            DiagnosticArg::new("issue", "sermo_view_payload_type_arg_count"),
            DiagnosticArg::new("method", "sermo.tuus"),
        ],
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn sermo_meus_bare_view_typechecks() {
    accepts(
        r#"incipit {
    fixum sermo s ← ad 'runtime:echo' ("salve")
    fixum _ sink ← s.meus()
}"#,
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn sermo_meus_typed_view_typechecks() {
    accepts(
        r#"incipit {
    fixum sermo s ← ad 'runtime:echo' ("salve")
    fixum _ sink ← s.meus<octeti>()
}"#,
    );
}

#[test]
fn sermo_meus_rejects_more_than_one_payload_type_arg() {
    rejects_with_args(
        r#"incipit {
    fixum sermo s ← ad 'runtime:echo' ("salve")
    fixum _ sink ← s.meus<textus, numerus>()
}"#,
        &[
            DiagnosticArg::new("issue", "sermo_view_payload_type_arg_count"),
            DiagnosticArg::new("method", "sermo.meus"),
        ],
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn inferred_generic_call_still_works() {
    accepts(
        r"functio id<T>(T x) → T {
    redde x
}

incipit {
    fixum numerus n ← id(42)
    nota n
}",
    );
}
