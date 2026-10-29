//! Explicit type arguments at function and method call sites.

use radix_diagnostics::DiagnosticArg;

fn compile(source: &str) -> crate::test_support::AnalyzedFixture {
    crate::test_support::analyze_source_la(source)
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
    sit textus x ← id<textus>("salve")
    nota x
}"#,
    );
}

#[test]
fn explicit_generic_call_binds_ranged_lista_argument() {
    accepts(
        r#"functio count<T>(lista<T, magnitudo 1…∞> xs) → numerus {
    redde xs.longitudo()
}

incipit {
    sit lista<numerus, magnitudo 1…∞> xs ← [7]
    nota count<numerus>(xs)
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
    sit _ x ← id<numerus>("salve")
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
    sit _ x ← plain<textus>("salve")
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
    sit _ x ← both<textus>(1, 2)
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
    sit _ x ← id<_>(42)
    nota x
}",
    );
}

#[test]
fn explicit_magnitudo_natural_args_bind_at_call_site() {
    // gradus nn.fab: `linear_from_raw<size 2, size 2, size 2>(...)` — `size`
    // typeArguments on magnitudo params.
    accepts(
        r"functio take<magnitudo M, magnitudo N>(tensor<f32, [M, N]> x) → tensor<f32, [M, N]> {
    redde x
}

incipit {
    sit tensor<f32, [2, 2]> a ← vacua
    sit tensor<f32, [2, 2]> b ← take<magnitudo 2, magnitudo 2>(a)
    nota b.longitudo()
}",
    );
}

#[test]
fn generic_call_forwards_symbolic_magnitudo_args_to_generic_callee() {
    // GSF-1: a generic caller must pass its own magnitudo witnesses on to a
    // generic callee (`forward<T, D>` spelling `sink<size T, size D>(x)`), not report
    // SEM008 unknown_type_name at the symbolic callee argument.
    accepts(
        r"functio sink<magnitudo M, magnitudo N>(tensor<f32, [M, N]> x) → tensor<f32, [M, N]> {
    redde x
}

functio forward<magnitudo T, magnitudo D>(tensor<f32, [T, D]> x) → tensor<f32, [T, D]> {
    redde sink<magnitudo T, magnitudo D>(x)
}

incipit {
    sit tensor<f32, [3, 5]> a ← vacua
    sit tensor<f32, [3, 5]> b ← forward<magnitudo 3, magnitudo 5>(a)
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
    redde sink<magnitudo T, magnitudo 4>(x)
}

incipit {
    sit tensor<f32, [3, 4]> a ← vacua
    sit tensor<f32, [3, 4]> b ← forward<magnitudo 3>(a)
    nota b.longitudo()
}",
    );
}

#[test]
fn affine_and_exact_division_generic_calls_preserve_the_matched_shape() {
    accepts(
        r"functio drop_one<magnitudo N>(tensor<f32, [N - 1]> value) → tensor<f32, [N - 1]> {
    redde value
}

functio half_width<magnitudo D>(tensor<f32, [D / 2]> value) → tensor<f32, [D / 2]> {
    redde value
}

functio cancel_offset<magnitudo N>(tensor<f32, [N - 1 + 1]> value) → tensor<f32, [N - 1 + 1]> {
    redde value
}

incipit {
    sit tensor<f32, [3]> three ← vacua
    sit tensor<f32, [3]> tail ← drop_one(three)
    sit tensor<f32, [4]> four ← vacua
    sit tensor<f32, [4]> half ← half_width(four)
    sit tensor<f32, [0]> zero ← vacua
    sit tensor<f32, [0]> canceled ← cancel_offset(zero)
    nota tail
    nota half
    nota canceled
}",
    );
}

#[test]
fn odd_generic_extent_rejects_an_inexact_exact_division_argument() {
    rejects_with_issue(
        r"functio pair_widths<magnitudo D>(tensor<f32, [D]> full, tensor<f32, [D / 2]> half) → vacuum {
}

incipit {
    sit tensor<f32, [5]> full ← vacua
    sit tensor<f32, [2]> half ← vacua
    pair_widths(full, half)
}",
        "incompatible_tensor_index",
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
    redde sink<A, magnitudo 4>(x)
}

incipit { tacet }",
        "generic_type_arg_sort",
    );
}

#[test]
fn explicit_magnitudo_natural_args_reject_wrong_arity() {
    rejects_with_issue(
        r"functio take<magnitudo M, magnitudo N>(tensor<f32, [M, N]> x) → tensor<f32, [M, N]> {
    redde x
}

incipit {
    sit tensor<f32, [2, 2]> a ← vacua
    sit tensor<f32, [2, 2]> b ← take<magnitudo 2>(a)
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
    sit tensor<f32, [2, 2]> a ← vacua
    sit tensor<f32, [2, 2]> b ← take<textus, numerus>(a)
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
    sit _ x ← both<_, textus>(1, "k")
    nota x
}"#,
    );
}

#[test]
fn mixed_hole_first_param_is_bare_numerus() {
    let unit = crate::test_support::analyze_ok_la(
        r#"functio both<A, B>(A a, B b) → A {
    redde a
}

incipit {
    sit _ x ← both<_, textus>(1, "k")
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
    let unit = crate::test_support::analyze_ok_la(
        r#"functio both<A, B>(A a, B b) → B {
    redde b
}

incipit {
    sit _ x ← both<numerus, _>(1, "k")
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
        r#"@ radix typus T in textus lista<textus> lista<b8>
functio load<T>(T value) → T {
    redde value
}

incipit {
    sit textus body ← load<_>("body")
    nota body
}"#,
    );
}

#[test]
fn constrained_generic_call_hole_rejects_after_solve() {
    rejects_with_args(
        r"@ radix typus T in textus lista<textus> lista<b8>
functio load<T>(T value) → T {
    redde value
}

incipit {
    sit numerus n ← load<_>(1)
    nota n
}",
        &[
            DiagnosticArg::new("issue", "generic_type_arg_domain"),
            DiagnosticArg::new("rejected", "numerus"),
            DiagnosticArg::new("param", "T"),
            DiagnosticArg::new("accepted", "textus, lista<b8>, lista<textus>"),
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
    sit _ x ← both<textus, ∪>("k", 1)
}"#,
        "explicit_union_type_arg_unsupported",
    );
}

#[test]
fn genus_applied_hole_solves_from_field() {
    accepts(
        r"genus C<T> {
    varia T value
}

incipit {
    sit C<_> x ← C { value = 1 }
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
    sit _ x ← both<_>(1, "k")
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
    sit _ x ← empty<_>()
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
    sit _ x ← both<∪, textus>(1, "k")
}"#,
        "explicit_union_type_arg_unsupported",
    );
}

#[test]
fn generic_applied_arg_accepts_local_size_alias() {
    accepts(
        r"typus Box<magnitudo N> = vector<f32, magnitudo N>
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
        r"typus Box<T, magnitudo N> = vector<T, magnitudo N>
functio concrete(Box<fractus, 3> value) → Box<fractus, 3> {
    redde value
}

incipit { tacet }",
    );
}

#[test]
fn generic_applied_arg_rejects_kind_swap() {
    rejects_with_issue(
        r"typus Box<magnitudo N> = vector<f32, magnitudo N>
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
        r"typus Box<magnitudo N> = vector<f32, magnitudo N>
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
    sit _ a ← id<_>(42)
    sit _ b ← id(42)
    nota a
    nota b
}";
    let unit = crate::test_support::analyze_source_la(source);
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
        r#"@ radix typus T in textus lista<textus> lista<b8>
functio load<T>(T value) → T {
    redde value
}

incipit {
    sit textus body ← load<textus>("body")
    sit lista<textus> lines ← load<lista<textus>>(["a", "b"])
    sit lista<b8> bytes ← load<lista<b8>>(|ca fe|)
    nota body
    nota lines
    nota bytes
}"#,
    );
}

#[test]
fn constrained_generic_call_rejects_unsupported_explicit_type_arg() {
    rejects_with_args(
        r"@ radix typus T in textus lista<textus> lista<b8>
functio load<T>(T value) → T {
    redde value
}

incipit {
    sit numerus n ← load<numerus>(1)
    nota n
}",
        &[
            DiagnosticArg::new("issue", "generic_type_arg_domain"),
            DiagnosticArg::new("rejected", "numerus"),
            DiagnosticArg::new("param", "T"),
            DiagnosticArg::new("accepted", "textus, lista<b8>, lista<textus>"),
        ],
    );
}

#[test]
fn constrained_generic_call_rejects_unsupported_inferred_type_arg() {
    rejects_with_args(
        r"@ radix typus T in textus lista<textus> lista<b8>
functio load<T>(T value) → T {
    redde value
}

incipit {
    sit numerus n ← load(1)
    nota n
}",
        &[
            DiagnosticArg::new("issue", "generic_type_arg_domain"),
            DiagnosticArg::new("rejected", "numerus"),
            DiagnosticArg::new("param", "T"),
            DiagnosticArg::new("accepted", "textus, lista<b8>, lista<textus>"),
        ],
    );
}

#[test]
fn constrained_generic_call_accepts_repeated_and_sugar_domains_structurally() {
    accepts(
        r"@ radix typus T in i32 i32
@ radix typus T in f32 f32
functio id<T>(T x) → T {
    redde x
}

incipit {
    sit i32 n ← id<i32>(1 ∷ i32)
    sit f32 f ← id<f32>(1.0 ∷ f32)
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
    sit _ s ← "§"<textus>("x")
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
    sit ignotum callee ← box(1)
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
    sit ignotum receiver ← box(1)
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
    sit lista<numerus> xs ← [1, 2, 3]
    sit _ n ← xs.longitudo<textus>()
}",
        &[DiagnosticArg::new("issue", "intrinsic_method_type_args")],
    );
}

#[test]
fn empty_generic_catalog_row_rejects_type_args_without_a_method_name_branch() {
    rejects_with_issue(
        r"incipit {
    sit lista<numerus> xs ← [1, 2, 3]
    varia _ ok ← xs.continet<textus>(1)
}",
        "intrinsic_method_type_args",
    );
}

#[test]
fn sermo_tuus_typed_view_typechecks_in_iteration() {
    accepts(
        r#"incipit {
    sit sermo s ← ad 'runtime:echo' ("salve")
    sit tuus<textus> t ← s.tuus<textus>()
    itera ex t.cursor() sit frame {
        adfirma frame.data ≡ "salve"
    }
}"#,
    );
}

#[test]
fn sermo_tuus_bare_view_defaults_to_valor_frames() {
    accepts(
        r#"incipit {
    sit sermo s ← ad 'runtime:echo' ("salve")
    sit tuus<valor> t ← s.tuus()
    itera ex t.cursor() sit frame {
        sit valor data ← frame.data
        nota data
    }
}"#,
    );
}

#[test]
fn sermo_directional_live_methods_typecheck() {
    accepts(
        r#"incipit {
    sit sermo s ← ad 'runtime:echo' ("salve")
    sit meus<textus> m ← s.meus<textus>()
    sit tuus<textus> t ← s.tuus<textus>()
    m.sink("salve")
    sit scrinium<textus> ∪ nihil reply ← t.accipe()
    sit textus drained ← t.exhauri()
    sit status outbound ← m.fini()
    sit status inbound ← t.fini()
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
    sit sermo s ← ad 'runtime:echo' ("salve")
    sit _ frames ← s.tuus<textus, numerus>()
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
    sit sermo s ← ad 'runtime:echo' ("salve")
    sit _ sink ← s.meus()
}"#,
    );
}

#[test]
fn sermo_meus_typed_view_typechecks() {
    accepts(
        r#"incipit {
    sit sermo s ← ad 'runtime:echo' ("salve")
    sit _ sink ← s.meus<lista<b8>>()
}"#,
    );
}

#[test]
fn sermo_meus_rejects_more_than_one_payload_type_arg() {
    rejects_with_args(
        r#"incipit {
    sit sermo s ← ad 'runtime:echo' ("salve")
    sit _ sink ← s.meus<textus, numerus>()
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
    sit numerus n ← id(42)
    nota n
}",
    );
}

// ---------------------------------------------------------------------------
// KPC-SEM — imported namespace generic calls (spike1 shape).
//
// `gradus:math`'s `add<size M, size N>` exports through a file interface whose
// callable carries the DFV2-3 device contract (`@ kernel @ publica`). The
// consumer's `math.add(a, b)` must resolve to that canonical imported `FuncSig`
// and solve concrete type/shape witnesses from the arguments and the expected
// result — never fall through the dynamic/`ignotum` route.

use std::collections::BTreeMap;

use crate::file_interface::{
    FileExport, FileExportKind, InterfaceBodyChannel, InterfaceCallable, InterfaceDeviceContract,
    InterfaceDeviceRole, InterfaceDeviceVisibility, InterfaceIndexSnapshot, InterfaceParam,
    InterfaceTypeSnapshot,
};
use crate::passes::visibility_annotation::VisibilityTier;

fn math_namespace_interface() -> crate::file_interface::FileInterface {
    let f32_elem = Box::new(InterfaceTypeSnapshot::SizedNumeric(
        crate::Primitive::Fractus,
        radix_types::NumericWidth::F32,
    ));
    let shape_mn = InterfaceIndexSnapshot::Tuple(vec![
        InterfaceIndexSnapshot::Param("M".to_owned()),
        InterfaceIndexSnapshot::Param("N".to_owned()),
    ]);
    let tensor_mn = InterfaceTypeSnapshot::Tensor(f32_elem.clone(), shape_mn.clone());
    let mut interface = crate::file_interface::FileInterface::new();
    interface.insert(FileExport {
        name: "add".to_owned(),
        kind: FileExportKind::Function(InterfaceCallable {
            type_params: vec![
                crate::file_interface::InterfaceCallableParam {
                    name: "M".to_owned(),
                    kind: radix_syntax::GenericParamKind::Magnitudo,
                    constraint: crate::file_interface::InterfaceTypeParamConstraint::Any,
                    default: None,
                },
                crate::file_interface::InterfaceCallableParam {
                    name: "N".to_owned(),
                    kind: radix_syntax::GenericParamKind::Magnitudo,
                    constraint: crate::file_interface::InterfaceTypeParamConstraint::Any,
                    default: None,
                },
            ],
            params: vec![
                InterfaceParam {
                    ty: tensor_mn.clone(),
                    mode: crate::SemanticParamMode::Owned,
                    optional: false,
                    sponte: false,
                    axis: false,
                },
                InterfaceParam {
                    ty: tensor_mn.clone(),
                    mode: crate::SemanticParamMode::Owned,
                    optional: false,
                    sponte: false,
                    axis: false,
                },
            ],
            ret: Box::new(tensor_mn),
            err: None,
            is_async: false,
            is_generator: false,
            // DFV2-3: an exported `@ kernel @ publica` declaration carries its
            // device contract on the callable. Typing must survive the fact.
            tensor_edge_policy_requirements: Vec::new(),
            device: Some(InterfaceDeviceContract {
                role: InterfaceDeviceRole::Kernel,
                visibility: InterfaceDeviceVisibility::Public,
                body: InterfaceBodyChannel::Source,
            }),
        }),
    });
    interface
}

fn analyze_math_consumer(source: &str) -> crate::test_support::AnalyzedFixture {
    crate::test_support::analyze_source_with_import_contract_la(
        source,
        BTreeMap::from([(
            "math".to_owned(),
            BTreeMap::from([("add".to_owned(), VisibilityTier::Publica)]),
        )]),
        BTreeMap::from([("math".to_owned(), math_namespace_interface())]),
    )
}

#[test]
fn imported_namespace_generic_call_types_concrete_result() {
    struct CallTypeCollector {
        tys: Vec<Option<crate::TypeId>>,
    }
    impl radix_hir::visit::HirVisitor for CallTypeCollector {
        fn visit_expr(&mut self, expr: &radix_hir::HirExpression) {
            if matches!(
                expr.kind,
                radix_hir::HirExpressionKind::MethodCall(..)
                    | radix_hir::HirExpressionKind::Call(..)
            ) {
                self.tys.push(expr.ty);
            }
            radix_hir::visit::walk_expr(self, expr);
        }
    }
    // spike1 shape: concrete tensor params and expected result around the
    // imported size-generic `math.add`. Must typecheck the concrete
    // `tensor<f32, [4, 8]>` result instead of SEM010/IGNOTUM.
    let mut unit = analyze_math_consumer(
        r#"importa ex "./math" math

functio _probe(tensor<f32, [4, 8]> a, tensor<f32, [4, 8]> b) → tensor<f32, [4, 8]> {
    redde math.add(a, b)
}"#,
    );
    assert!(
        unit.is_ok(),
        "imported generic wrapper should analyze green: {:?}",
        unit.diagnostics
    );

    // The resolved canonical signature is the imported one, not a method on
    // the receiver value or an ignotum escape.
    let math = unit.interner.intern("math");
    let add = unit.interner.intern("add");
    let sig = unit
        .resolver
        .namespace_file_function_canonical(math, add)
        .expect("canonical imported add signature");
    assert_eq!(sig.type_params.len(), 2, "add stays size-generic");

    // The call's recorded type is the concrete solved result.
    let mut collector = CallTypeCollector { tys: Vec::new() };
    radix_hir::visit::walk_program(&mut collector, unit.hir.as_ref().expect("hir"));
    let call_ty = collector
        .tys
        .last()
        .copied()
        .flatten()
        .expect("math.add call type recorded");
    let name_of = |def_id| {
        unit.resolver.get_symbol(def_id).map_or_else(
            || format!("{def_id:?}"),
            |symbol| unit.interner.resolve(symbol.name).to_owned(),
        )
    };
    let shown = crate::format_faber_type(call_ty, &unit.types, &unit.interner, &name_of);
    assert_eq!(
        shown, "tensor<f32, [4, 8]>",
        "imported generic call must solve the concrete result shape"
    );
    assert!(
        !matches!(
            unit.types.get(call_ty),
            crate::Type::Primitive(crate::Primitive::Ignotum)
        ),
        "imported generic call must not fall through the ignotum route"
    );
}

#[test]
fn imported_namespace_generic_call_host_expected_control() {
    // Control E shape: the same imported call inside a host entry with a
    // concrete annotation — proves the repair is not device-context-only.
    let unit = analyze_math_consumer(
        r#"importa ex "./math" math

incipit {
    sit tensor<f32, [4, 8]> a ← vacua
    sit tensor<f32, [4, 8]> b ← vacua
    sit tensor<f32, [4, 8]> r ← math.add(a, b)
    nota r
}"#,
    );
    assert!(
        unit.is_ok(),
        "host-context imported generic call should analyze green: {:?}",
        unit.diagnostics
    );
    let tys = crate::test_support::entry_local_tys(&unit);
    assert_eq!(tys.len(), 3, "a, b, r locals");
    let name_of = |def_id| {
        unit.resolver.get_symbol(def_id).map_or_else(
            || format!("{def_id:?}"),
            |symbol| unit.interner.resolve(symbol.name).to_owned(),
        )
    };
    let shown = crate::format_faber_type(tys[2], &unit.types, &unit.interner, &name_of);
    assert_eq!(shown, "tensor<f32, [4, 8]>");
}

#[test]
fn imported_namespace_generic_call_binds_explicit_magnitudo_args() {
    // Control F repaired: explicit size witnesses on the imported generic
    // callee instantiate instead of tripping the dynamic-receiver rejection
    // the ignotum route produced.
    let unit = analyze_math_consumer(
        r#"importa ex "./math" math

functio _probe(tensor<f32, [4, 8]> a, tensor<f32, [4, 8]> b) → tensor<f32, [4, 8]> {
    redde math.add<magnitudo 4, magnitudo 8>(a, b)
}"#,
    );
    assert!(
        unit.is_ok(),
        "explicit magnitudo witnesses on the imported generic call should analyze green: {:?}",
        unit.diagnostics
    );
}

#[test]
fn imported_namespace_generic_call_rejects_incompatible_shape() {
    // Genuinely incompatible witnesses stay fail-closed with the existing
    // argument mismatch diagnostic.
    let unit = analyze_math_consumer(
        r#"importa ex "./math" math

functio _bad(tensor<f32, [4, 8]> a, tensor<f32, [2, 2]> b) → tensor<f32, [4, 8]> {
    redde math.add(a, b)
}"#,
    );
    assert!(
        !unit.is_ok(),
        "mismatched shape witness must fail: {:?}",
        unit.diagnostics
    );
    assert!(
        unit.diagnostics.iter().any(|diag| diag
            .args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == "argument_type_mismatch")),
        "expected argument_type_mismatch, got: {:?}",
        unit.diagnostics
    );
}

#[test]
fn imported_namespace_unknown_member_stays_fail_closed() {
    // An unresolved namespace callee keeps its existing diagnostic; the
    // generic repair must not make unknown exports permissive.
    let unit = analyze_math_consumer(
        r#"importa ex "./math" math

functio _probe(tensor<f32, [4, 8]> a, tensor<f32, [4, 8]> b) → tensor<f32, [4, 8]> {
    redde math.sub(a, b)
}"#,
    );
    assert!(
        !unit.is_ok(),
        "unknown namespace member must fail: {:?}",
        unit.diagnostics
    );
    assert!(
        unit.diagnostics.iter().any(|diag| diag
            .args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == "namespace_missing_export")),
        "expected namespace_missing_export, got: {:?}",
        unit.diagnostics
    );
}

#[test]
fn namespace_call_without_import_surface_keeps_bare_driver_leniency() {
    // KPC-SEM boundary control: with no import contract at all (bare driver
    // mode, `AnalyzeRequest::default()`), an import binding has no surface and
    // its member calls keep the pinned permissive typing — the empty-contract
    // leniency `module_analysis_does_not_derive_contracts_from_path_name`
    // pins. The fail-closed repair for a resolvable-but-unusable import is
    // driver-owned (the import graph must propagate the target failure), not a
    // semantic guess from an absent contract. This test pins that the generic
    // repair does not turn contract absence into hard errors.
    let unit = crate::test_support::analyze_source_with_import_contract_la(
        r#"importa ex "./math" math

functio _probe(tensor<f32, [4, 8]> a, tensor<f32, [4, 8]> b) → tensor<f32, [4, 8]> {
    sit ignotum r ← math.add(a, b)
    redde a
}"#,
        BTreeMap::new(),
        BTreeMap::new(),
    );
    assert!(
        unit.is_ok(),
        "contract absence must stay permissive (bare-driver leniency): {:?}",
        unit.diagnostics
    );
}

#[test]
fn ignotum_value_receiver_method_keeps_dynamic_route() {
    // Ordinary unknown callees stay on their existing diagnostics: an
    // ignotum-typed VALUE receiver (not an import namespace) keeps the
    // documented escape, including its explicit-type-args rejection.
    rejects_with_args(
        r"functio box(ignotum v) → ignotum {
    redde v
}

incipit {
    sit ignotum receiver ← box(1)
    receiver.meth<textus>()
}",
        &[DiagnosticArg::new(
            "issue",
            "dynamic_receiver_method_type_args",
        )],
    );
}

// --- spread argument checked once ---

#[test]
fn spread_argument_diagnostic_is_reported_once() {
    let count = |source: &str| {
        compile(source)
            .diagnostics
            .iter()
            .filter(|diag| diag.issue() == Some("string_operands_required"))
            .count()
    };
    let spread = count(
        r#"functio duo(numerus a, numerus b) → numerus {
    redde a + b
}

incipit {
    sit numerus n ← duo(sparge (1 + "a"))
    nota n
}"#,
    );
    let plain = count(
        r#"functio duo(numerus a, numerus b) → numerus {
    redde a + b
}

incipit {
    sit numerus n ← duo((1 + "a"), 2)
    nota n
}"#,
    );
    assert_eq!(plain, 1, "control: one diagnostic for the plain call");
    assert_eq!(spread, 1, "spread argument must be checked once");
}

// ---------------------------------------------------------------------------
// A-03 residual: the inferred call form (no explicit type arguments) must bind
// the type parameter through every container shape the explicit form already
// binds. `substitute_type_params` rewrites `T` to a fresh inference hole
// inside the declared container; `unify_with_policy` then has to walk the
// element (and capacity) slots, exactly as the `lista<T>` arm does.

#[test]
fn inferred_generic_call_binds_through_queue_and_stack() {
    accepts(
        r"functio peek_lista<T>(lista<T> xs) → numerus {
    redde 0
}

functio peek_queue<T>(queue<T> q) → numerus {
    redde 0
}

functio peek_stack<T>(stack<T> s) → numerus {
    redde 0
}

functio caller(lista<numerus> l, queue<numerus> q, stack<numerus> s) → numerus {
    redde peek_lista(l) + peek_queue(q) + peek_stack(s)
}",
    );
}

#[test]
fn inferred_generic_call_binds_through_bounded_queue_and_stack() {
    accepts(
        r"functio peek_bounded_queue<T>(queue<T, magnitudo 4> q) → numerus {
    redde 0
}

functio peek_bounded_stack<T>(stack<T, magnitudo 4> s) → numerus {
    redde 0
}

functio caller(queue<numerus, magnitudo 4> bq, stack<numerus, magnitudo 4> bs) → numerus {
    redde peek_bounded_queue(bq) + peek_bounded_stack(bs)
}",
    );
}

#[test]
fn inferred_generic_call_binds_through_ledger() {
    accepts(
        r"functio peek_ledger<T>(tabulae<textus, T> m) → numerus {
    redde 0
}

functio caller(tabulae<textus, numerus> m) → numerus {
    redde peek_ledger(m)
}",
    );
}

#[test]
fn inferred_generic_call_binds_through_intersection() {
    accepts(
        r#"implendum Nomen {
    functio nomen() → textus
}

implendum Salutans {
    functio saluta() → textus
}

genus Civis implet Nomen ∩ Salutans {
    functio nomen() → textus {
        redde "civis"
    }

    functio saluta() → textus {
        redde "salve"
    }
}

functio peek<T>(T ∩ Nomen x) → numerus {
    redde 0
}

functio caller_inter(Nomen ∩ Salutans ns) → numerus {
    redde peek(ns)
}

functio caller_genus(Civis c) → numerus {
    redde peek(c)
}"#,
    );
}

#[test]
fn inferred_generic_intersection_with_prior_concrete_use() {
    accepts(
        r"implendum Nomen {
    functio nomen() → textus
}

implendum Salutans {
    functio saluta() → textus
}

functio early(Salutans x) → numerus {
    redde 1
}

functio peek<T>(T ∩ Nomen x) → numerus {
    redde 0
}

functio caller(Nomen ∩ Salutans ns) → numerus {
    redde peek(ns)
}",
    );
}

#[test]
fn inferred_generic_intersection_prior_use_of_other_member() {
    accepts(
        r"implendum Nomen {
    functio nomen() → textus
}

implendum Salutans {
    functio saluta() → textus
}

functio early(Nomen x) → numerus {
    redde 1
}

functio peek<T>(T ∩ Nomen x) → numerus {
    redde 0
}

functio caller(Nomen ∩ Salutans ns) → numerus {
    redde peek(ns)
}",
    );
}

#[test]
fn inferred_generic_intersection_constraint_position() {
    accepts(
        r"implendum Nomen {
    functio nomen() → textus
}

implendum Salutans {
    functio saluta() → textus
}

functio early(Salutans x) → numerus {
    redde 1
}

functio peek<T>(Nomen ∩ T x) → numerus {
    redde 0
}

functio caller(Salutans ∩ Nomen ns) → numerus {
    redde peek(ns)
}",
    );
}

#[test]
fn inferred_generic_intersection_rejects_missing_member() {
    rejects_with_issue(
        r"implendum Nomen {
    functio nomen() → textus
}

implendum Salutans {
    functio saluta() → textus
}

implendum Alius {
    functio alius() → textus
}

functio early(Salutans x) → numerus {
    redde 1
}

functio peek<T>(T ∩ Nomen x) → numerus {
    redde 0
}

functio caller(Salutans ∩ Alius sa) → numerus {
    redde peek(sa)
}",
        "argument_type_mismatch",
    );
}

#[test]
fn inferred_generic_call_rejects_container_element_mismatch() {
    rejects_with_issue(
        r"functio wants(queue<numerus> q) → numerus {
    redde 0
}

functio caller(queue<textus> q) → numerus {
    redde wants(q)
}",
        "argument_type_mismatch",
    );
}

#[test]
fn inferred_generic_call_rejects_bounded_capacity_mismatch() {
    rejects_with_issue(
        r"functio wants(queue<numerus, magnitudo 4> q) → numerus {
    redde 0
}

functio caller(queue<numerus, magnitudo 8> q) → numerus {
    redde wants(q)
}",
        "incompatible_tensor_index",
    );
}
