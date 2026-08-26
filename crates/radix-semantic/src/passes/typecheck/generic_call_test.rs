//! Explicit type arguments at function and method call sites.

use radix_diagnostics::DiagnosticArg;

fn compile(source: &str) -> crate::test_support::AnalyzedFixture {
    crate::test_support::analyze_source(source)
}

fn accepts(source: &str) {
    let result = compile(source);
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
}

fn rejects_with_issue(source: &str, issue: &str) {
    let result = compile(source);
    assert!(
        !result.is_ok(),
        "expected failure, got: {:?}",
        result.diagnostics
    );
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
    assert!(
        !result.is_ok(),
        "expected failure, got: {:?}",
        result.diagnostics
    );
    assert!(
        result.diagnostics.iter().any(|diag| diag.args == args),
        "expected diagnostic with args {args:?}, got: {:?}",
        result.diagnostics
    );
}

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
fn explicit_generic_call_accepts_infer_hole() {
    accepts(
        r"functio id<T>(T x) → T {
    redde x
}

incipit {
    fixum _ x ← id<_>(42)
    nota x
}",
    );
}

#[test]
fn explicit_magnitudo_natural_args_bind_at_call_site() {
    // gradus nn.fab: `linear_from_raw<2, 2, 2>(...)` — NATURAL typeArguments
    // on magnitudo params.
    accepts(
        r"functio take<magnitudo M, magnitudo N>(tensor<f32, [M, N]> x) → tensor<f32, [M, N]> {
    redde x
}

incipit {
    fixum tensor<f32, [2, 2]> a ← vacua
    fixum tensor<f32, [2, 2]> b ← take<2, 2>(a)
    nota b.longitudo()
}",
    );
}

#[test]
fn generic_call_forwards_symbolic_magnitudo_args_to_generic_callee() {
    // GSF-1: a generic caller must pass its own magnitudo witnesses on to a
    // generic callee (`forward<T, D>` spelling `sink<T, D>(x)`), not report
    // SEM008 unknown_type_name at the symbolic callee argument.
    accepts(
        r"functio sink<magnitudo M, magnitudo N>(tensor<f32, [M, N]> x) → tensor<f32, [M, N]> {
    redde x
}

functio forward<magnitudo T, magnitudo D>(tensor<f32, [T, D]> x) → tensor<f32, [T, D]> {
    redde sink<T, D>(x)
}

incipit {
    fixum tensor<f32, [3, 5]> a ← vacua
    fixum tensor<f32, [3, 5]> b ← forward<3, 5>(a)
    nota b.longitudo()
}",
    );
}

#[test]
fn generic_call_forwards_mixed_symbolic_and_natural_magnitudo_args() {
    // A forwarded witness may sit beside a natural literal in one list.
    accepts(
        r"functio sink<magnitudo M, magnitudo N>(tensor<f32, [M, N]> x) → tensor<f32, [M, N]> {
    redde x
}

functio forward<magnitudo T>(tensor<f32, [T, 4]> x) → tensor<f32, [T, 4]> {
    redde sink<T, 4>(x)
}

incipit {
    fixum tensor<f32, [3, 4]> a ← vacua
    fixum tensor<f32, [3, 4]> b ← forward<3>(a)
    nota b.longitudo()
}",
    );
}

#[test]
fn generic_call_symbolic_forwarding_rejects_typus_param_in_size_slot() {
    // Wrong-kind stays closed: a caller typus param is a type witness, so
    // binding it into a magnitudo slot mismatches the callee signature.
    rejects_with_issue(
        r"functio sink<magnitudo M, magnitudo N>(tensor<f32, [M, N]> x) → tensor<f32, [M, N]> {
    redde x
}

functio wrong<A>(tensor<f32, [2, 4]> x) → tensor<f32, [2, 4]> {
    redde sink<A, 4>(x)
}

incipit { tacet }",
        "argument_type_mismatch",
    );
}

#[test]
fn explicit_magnitudo_natural_args_reject_wrong_arity() {
    rejects_with_issue(
        r"functio take<magnitudo M, magnitudo N>(tensor<f32, [M, N]> x) → tensor<f32, [M, N]> {
    redde x
}

incipit {
    fixum tensor<f32, [2, 2]> a ← vacua
    fixum tensor<f32, [2, 2]> b ← take<2>(a)
}",
        "wrong_number_of_type_arguments",
    );
}

#[test]
fn explicit_magnitudo_natural_args_reject_wrong_kind() {
    rejects_with_issue(
        r"functio take<magnitudo M, magnitudo N>(tensor<f32, [M, N]> x) → tensor<f32, [M, N]> {
    redde x
}

incipit {
    fixum tensor<f32, [2, 2]> a ← vacua
    fixum tensor<f32, [2, 2]> b ← take<textus, numerus>(a)
}",
        "callee_type_args_unsupported",
    );
}

#[test]
fn explicit_generic_call_accepts_mixed_hole_and_concrete() {
    accepts(
        r#"functio both<A, B>(A a, B b) → A {
    redde a
}

incipit {
    fixum _ x ← both<_, textus>(1, "k")
    nota x
}"#,
    );
}

#[test]
fn mixed_hole_first_param_is_bare_numerus() {
    let unit = crate::test_support::analyze_ok(
        r#"functio both<A, B>(A a, B b) → A {
    redde a
}

incipit {
    fixum _ x ← both<_, textus>(1, "k")
    nota x
}"#,
    );
    let tys = crate::test_support::entry_local_tys(&unit);
    assert_eq!(tys.len(), 1);
    assert!(
        matches!(
            unit.types.get(tys[0]),
            crate::Type::Primitive(crate::Primitive::Numerus)
        ),
        "both<_, textus>(1, \"k\") must solve A to bare numerus, got {:?}",
        unit.types.get(tys[0])
    );
}

#[test]
fn mixed_hole_second_param_solves_from_argument() {
    let unit = crate::test_support::analyze_ok(
        r#"functio both<A, B>(A a, B b) → B {
    redde b
}

incipit {
    fixum _ x ← both<numerus, _>(1, "k")
    nota x
}"#,
    );
    let tys = crate::test_support::entry_local_tys(&unit);
    assert_eq!(tys.len(), 1);
    assert!(
        matches!(
            unit.types.get(tys[0]),
            crate::Type::Primitive(crate::Primitive::Textus)
        ),
        "both<numerus, _>(1, \"k\") must solve B to textus, got {:?}",
        unit.types.get(tys[0])
    );
}

#[test]
fn constrained_generic_call_hole_solves_then_checks_domain() {
    accepts(
        r#"@ radix typus T in textus lista<textus> octeti
functio load<T>(T value) → T {
    redde value
}

incipit {
    fixum textus body ← load<_>("body")
    nota body
}"#,
    );
}

#[test]
fn constrained_generic_call_hole_rejects_after_solve() {
    rejects_with_args(
        r"@ radix typus T in textus lista<textus> octeti
functio load<T>(T value) → T {
    redde value
}

incipit {
    fixum numerus n ← load<_>(1)
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
fn explicit_generic_call_rejects_union_hole_in_second_slot() {
    rejects_with_issue(
        r#"functio both<A, B>(A a, B b) → A {
    redde a
}

incipit {
    fixum _ x ← both<textus, ∪>("k", 1)
}"#,
        "explicit_union_type_arg_unsupported",
    );
}

#[test]
fn genus_applied_hole_solves_from_field() {
    accepts(
        r"genus C<T> {
    T value
}

incipit {
    fixum C<_> x ← C { value = 1 }
    nota x
}",
    );
}

#[test]
fn explicit_generic_call_hole_still_requires_exact_arity() {
    rejects_with_issue(
        r#"functio both<A, B>(A a, B b) → A {
    redde a
}

incipit {
    fixum _ x ← both<_>(1, "k")
}"#,
        "wrong_number_of_type_arguments",
    );
}

#[test]
fn explicit_generic_call_unsolved_hole_still_errors() {
    rejects_with_issue(
        r"functio empty<T>() → T {
    tacet
}

incipit {
    fixum _ x ← empty<_>()
}",
        "cannot_infer_expression_type",
    );
}

#[test]
fn explicit_generic_call_rejects_union_hole() {
    rejects_with_issue(
        r#"functio both<A, B>(A a, B b) → A {
    redde a
}

incipit {
    fixum _ x ← both<∪, textus>(1, "k")
}"#,
        "explicit_union_type_arg_unsupported",
    );
}

#[test]
fn generic_applied_arg_accepts_local_size_alias() {
    accepts(
        r"typus Box<magnitudo N> = vector<f32, N>
functio keep<magnitudo N>(Box<N> value) → Box<N> {
    redde value
}
functio concrete(Box<3> value) → Box<3> {
    redde value
}

incipit { tacet }",
    );
}

#[test]
fn generic_applied_arg_accepts_ordered_type_and_size_args() {
    accepts(
        r"typus Box<T, magnitudo N> = vector<T, N>
functio concrete(Box<fractus, 3> value) → Box<fractus, 3> {
    redde value
}

incipit { tacet }",
    );
}

#[test]
fn generic_applied_arg_rejects_kind_swap() {
    rejects_with_issue(
        r"typus Box<magnitudo N> = vector<f32, N>
functio wrong(Box<textus> value) → Box<textus> {
    redde value
}

incipit { tacet }",
        "generic_argument_kind_mismatch",
    );
}

#[test]
fn generic_applied_arg_rejects_wrong_arity() {
    rejects_with_issue(
        r"typus Box<magnitudo N> = vector<f32, N>
functio wrong(Box<3, 4> value) → Box<3, 4> {
    redde value
}

incipit { tacet }",
        "generic_argument_arity_mismatch",
    );
}

#[test]
fn explicit_hole_and_omitted_list_share_return_type_id() {
    let source = r"functio id<T>(T x) → T {
    redde x
}

incipit {
    fixum _ a ← id<_>(42)
    fixum _ b ← id(42)
    nota a
    nota b
}";
    let unit = crate::test_support::analyze_source(source);
    assert!(
        !unit.diagnostics.iter().any(crate::SemanticError::is_error),
        "diagnostics: {:?}",
        unit.diagnostics
    );
    let entry = unit
        .hir
        .as_ref()
        .expect("hir")
        .entry
        .as_ref()
        .expect("incipit");
    let tys: Vec<_> = entry
        .statements
        .iter()
        .filter_map(|stmt| match &stmt.kind {
            radix_hir::HirStatementKind::Local(local) => local.ty,
            _ => None,
        })
        .collect();
    assert_eq!(tys.len(), 2, "expected two locals");
    assert_eq!(
        tys[0],
        tys[1],
        "id<_>(42) and id(42) must share a TypeId, got {:?} vs {:?}",
        unit.types.get(tys[0]),
        unit.types.get(tys[1])
    );
}

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
            DiagnosticArg::new(
                "radix_typus_braced",
                "@ radix { param = T, allowed = textus }",
            ),
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
        &[DiagnosticArg::new(
            "issue",
            "dynamic_receiver_method_type_args",
        )],
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

#[test]
fn sermo_meus_bare_view_typechecks() {
    accepts(
        r#"incipit {
    fixum sermo s ← ad 'runtime:echo' ("salve")
    fixum _ sink ← s.meus()
}"#,
    );
}

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
