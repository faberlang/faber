//! DEFER-059 ratchet: generic params stay rigid in bodies; call sites instantiate.

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

#[test]
fn generic_return_widening_rejects_unsound_conversio() {
    rejects_with_issue(
        r"functio bad<T>(T x) → T {
    redde x ↦ textus
}",
        "return_type_mismatch",
    );
}

#[test]
fn generic_union_construction_substitutes_expected_witness() {
    // U-D1: a generic union's variant payload is checked against the union's
    // type param substituted by the enclosing expected type.
    accepts(
        r"discretio Cista<T> {
    Plena { T res }
}

functio facere() → Cista<numerus> {
    redde finge Plena { res = 1 }
}",
    );
}

#[test]
fn generic_class_construction_substitutes_expected_witness() {
    // U-D1: generic genus construction substitutes the expected witness into
    // the field type before the initializer is checked.
    accepts(
        r"genus C<T> {
    varia T value
}

functio facere() → C<numerus> {
    redde C { value = 1 }
}",
    );
}

#[test]
fn generic_class_construction_rejects_uninstantiated_field() {
    // Without an applied context the raw `T` field is still not satisfiable by
    // a concrete initializer: the substitution must come from the construction
    // site, never silently from the field.
    rejects_with_issue(
        r#"genus C<T> {
    varia T value
}

functio facere() → C<numerus> {
    redde C { value = "x" }
}"#,
        "field_initializer_type_mismatch",
    );
}

#[test]
fn generic_call_instantiates_type_params_at_call_site() {
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

#[test]
fn generic_call_shares_one_witness_for_repeated_param_symbol() {
    accepts(
        r"functio pair<T>(T a, T b) → T {
    redde a
}

incipit {
    sit numerus n ← pair(1, 2)
    nota n
}",
    );
}

#[test]
fn generic_call_rejects_mismatched_witnesses_for_shared_param_symbol() {
    rejects_with_issue(
        r#"functio pair<T>(T a, T b) → T {
    redde a
}

incipit {
    sit _ n ← pair(1, "two")
}"#,
        "argument_type_mismatch",
    );
}

#[test]
fn magnitudo_param_lowers_in_vector_signature() {
    accepts(
        r"functio same_width<elem, magnitudo N>(vector<elem, magnitudo N> v) → vector<elem, magnitudo N> {
    redde v
}",
    );
}

#[test]
fn vector_dot_returns_element_type() {
    accepts(
        r"incipit {
    sit vf32[4] a ← [1.0, 2.0, 3.0, 4.0] ↦ vf32[4]
    sit vf32[4] b ← [4.0, 3.0, 2.0, 1.0] ↦ vf32[4]
    sit f32 d ← a.productum(b)
    nota d
}",
    );
}

// ── shape generics (Phase 1A): implicit magnitudo binding ─────────────────

#[test]
fn shape_param_binds_from_argument_shape_at_call_site() {
    // D1: a magnitudo shape param lowercases to a fresh index hole and binds
    // from the argument tensor shapes; the call-site result shape is the
    // concrete witness.
    accepts(
        r"functio sgd_step<magnitudo Figura>(tensor<f32, Figura> param, tensor<f32, Figura> grad, f32 lr) → tensor<f32, Figura> {
    sit tensor<f32, Figura> lr_fill ← param.reple(lr)
    sit tensor<f32, Figura> scaled ← grad ⊙ lr_fill
    redde param - scaled
}

incipit {
    sit tensor<f32, [2, 2]> p ← vacua
    sit tensor<f32, [2, 2]> g ← vacua
    sit tensor<f32, [2, 2]> r ← sgd_step(p, g, 0.1)
    nota r.longitudo()
}",
    );
}

#[test]
fn tuple_shape_params_bind_from_argument_shape() {
    // D1 on the GOAL-CHECK blocker pattern: `tensor<f32, [B, D]>` with two
    // magnitudo params binds both dims from a `[2, 2]` argument.
    accepts(
        r"functio sgd_generic<T, magnitudo B, magnitudo D>(tensor<T, [B, D]> param, tensor<T, [B, D]> grad, T lr) → tensor<T, [B, D]> {
    redde param
}

incipit {
    sit tensor<f32, [2, 2]> p ← vacua
    sit tensor<f32, [2, 2]> g ← vacua
    sit tensor<f32, [2, 2]> r ← sgd_generic(p, g, 0.1)
    nota r.longitudo()
}",
    );
}

#[test]
fn shape_param_binds_rank_agnostic_across_concrete_ranks() {
    // D1: the bare `Figura` shape param is rank-agnostic — one generic
    // signature covers `[2, 2]`, `[4]`, and `[]` witnesses.
    accepts(
        r"functio sgd_step<magnitudo Figura>(tensor<f32, Figura> param, tensor<f32, Figura> grad, f32 lr) → tensor<f32, Figura> {
    sit tensor<f32, Figura> lr_fill ← param.reple(lr)
    sit tensor<f32, Figura> scaled ← grad ⊙ lr_fill
    redde param - scaled
}

incipit {
    sit tensor<f32, [2, 2]> p2 ← vacua
    sit tensor<f32, [2, 2]> g2 ← vacua
    sit tensor<f32, [2, 2]> r2 ← sgd_step(p2, g2, 0.1)
    sit tensor<f32, [4]> p1 ← vacua
    sit tensor<f32, [4]> g1 ← vacua
    sit tensor<f32, [4]> r1 ← sgd_step(p1, g1, 0.1)
    sit tensor<f32, []> p0 ← vacua
    sit tensor<f32, []> g0 ← vacua
    sit tensor<f32, []> r0 ← sgd_step(p0, g0, 0.1)
    nota r0.longitudo()
}",
    );
}

#[test]
fn shape_param_binds_from_expected_type() {
    // D1: a shape param present only in the return type binds from the
    // enclosing expected type (no shape-bearing arguments).
    accepts(
        r"functio make<T, magnitudo Figura>(T x) → tensor<T, Figura> {
    redde vacua
}

incipit {
    sit tensor<numerus, [2, 2]> u ← make(5)
    nota u.longitudo()
}",
    );
}

#[test]
fn shape_param_binds_with_explicit_type_args() {
    // Explicit type args cover type params only; shape params stay implicit
    // (D1), so `sgd_generic<f32>` needs no figura witness.
    accepts(
        r"functio sgd_generic<T, magnitudo Figura>(tensor<T, Figura> param, T lr) → tensor<T, Figura> {
    redde param
}

incipit {
    sit tensor<f32, [2, 2]> p ← vacua
    sit tensor<f32, [2, 2]> r ← sgd_generic<f32>(p, 0.1)
    nota r.longitudo()
}",
    );
}

#[test]
fn shared_shape_param_binds_once_across_repeated_positions() {
    // One hole per distinct shape-param symbol: param and grad share `Figura`,
    // so a `[2, 2]` grad must match a `[2, 2]` param — a rank-2 grad against
    // a rank-1 param rejects instead of binding two independent shapes.
    rejects_with_issue(
        r"functio pair_shaped<T, magnitudo Figura>(tensor<T, Figura> a, tensor<T, Figura> b) → tensor<T, Figura> {
    redde a
}

incipit {
    sit tensor<f32, [2, 2]> x ← vacua
    sit tensor<f32, [2]> y ← vacua
    sit _ r ← pair_shaped(x, y)
}",
        "incompatible_tensor_index",
    );
}

#[test]
fn unbound_shape_param_rejects_with_sem014() {
    // D6: a shape param bound by neither arguments nor the expected type is
    // an explicit SEM014.cannot_infer_tensor_shape — never a silent guess.
    rejects_with_issue(
        r"functio make<T, magnitudo Figura>(T x) → tensor<T, Figura> {
    redde vacua
}

incipit {
    make(5)
}",
        "cannot_infer_tensor_shape",
    );
}

#[test]
fn param_bound_shape_does_not_fire_sem014_in_library_context() {
    // The goal's SEM014 relaxation: a shape held rigid by a magnitudo param is
    // not an open metavariable, so a library-context declaration finalizes
    // without `cannot_infer_tensor_shape`.
    accepts(
        r"functio sgd_step<T, magnitudo Figura>(tensor<T, Figura> param, tensor<T, Figura> grad, T lr) → tensor<T, Figura> {
    redde param
}",
    );
}

#[test]
fn open_shape_hole_in_library_context_still_fires_sem014() {
    // The pinned remaining SEM014 trigger: a literal `_` hole in a tensor shape
    // that no witness resolves is still a missing annotation.
    rejects_with_issue(
        r"functio k() → vacuum {
    sit tensor<f32, [_, 3]> x ← vacua
}",
        "cannot_infer_tensor_shape",
    );
}

// ── shape-changing contracts (Phase 2A): symbolic methods in generic bodies ─

#[test]
fn generic_scaled_dot_product_method_body_typechecks() {
    // The Phase-2 payoff shape: method-spelled scaled_dot_product with
    // symbolic transpose, matmul, elementwise, and softmax steps.
    accepts(
        r"functio sdpa_full<magnitudo B, magnitudo D>(
    tensor<f32, [B, D]> qb, tensor<f32, [B, D]> kb, tensor<f32, [B, D]> vb,
    tensor<f32, [B, B]> scale
) → tensor<f32, [B, D]> {
    sit tensor<f32, [D, B]> kt ← kb.transpone()
    sit tensor<f32, [B, B]> scores ← qb.matmul(kt)
    sit tensor<f32, [B, B]> scaled ← scores ⊙ scale
    sit tensor<f32, [B, B]> attn ← scaled.activatio_softmax()
    sit tensor<f32, [B, D]> context ← attn.matmul(vb)
    redde context
}",
    );
}

#[test]
fn shape_changing_call_site_binds_witnesses() {
    // Concrete [2, 8] arguments bind B and D through a shape-changing body.
    accepts(
        r#"functio sdpa<magnitudo B, magnitudo D>(tensor<f32, [B, D]> qb, tensor<f32, [B, D]> kb, tensor<f32, [B, D]> vb) → tensor<f32, [B, D]> {
    sit tensor<f32, [D, B]> kt ← kb.transpone()
    sit tensor<f32, [B, B]> scores ← qb.matmul(kt)
    sit tensor<f32, [B, D]> context ← scores.matmul(vb)
    redde context
}

incipit {
    sit tensor<f32, [2, 8]> qb ← vacua
    sit tensor<f32, [2, 8]> kb ← vacua
    sit tensor<f32, [2, 8]> vb ← vacua
    sit tensor<f32, [2, 8]> ctx ← sdpa(qb, kb, vb)
    nota "call ok"
}"#,
    );
}

#[test]
fn shape_changing_call_site_shape_mismatch_rejects() {
    // A [3, 4] argument cannot witness [B, D] after B=2/D=8 bound from the
    // first argument; the inner matmul dims also mismatch. Reject, never guess.
    rejects_with_issue(
        r#"functio sdpa_bad<magnitudo B, magnitudo D>(tensor<f32, [B, D]> qb, tensor<f32, [B, D]> kb, tensor<f32, [B, D]> vb) → tensor<f32, [B, D]> {
    redde qb.matmul(kb)
}

incipit {
    sit tensor<f32, [2, 8]> qb ← vacua
    sit tensor<f32, [3, 4]> kb ← vacua
    sit tensor<f32, [2, 8]> vb ← vacua
    sit tensor<f32, [2, 8]> ctx ← sdpa_bad(qb, kb, vb)
    nota "bad call ok"
}"#,
        "incompatible_tensor_index",
    );
}

// ── method-level type params on generic classes ───────────

#[test]
fn generic_method_type_params_mix_with_class_params_accepts() {
    // Class param A is visible in the method signature and body; method param
    // C is visible in the method signature and body.
    accepts(
        r"genus Something<A> {
    functio run<C>(A b, C c) → A {
        sit C local ← c
        sit A a2 ← b
        redde a2
    }
}",
    );
}

#[test]
fn generic_method_type_param_shadows_class_param_name() {
    // Method param A reuses the class param name and wins in the method
    // signature. Box<numerus> would reject "x" for the class witness, so
    // set<textus>("x") accepting proves the method's A bound the call.
    accepts(
        r#"genus Box<A> {
    varia A hidden
    functio set<A>(A x) → A { redde x }
}

functio make() → Box<numerus> {
    redde Box { hidden = 1 }
}

incipit {
    sit Box<numerus> s ← make()
    sit textus t ← s.set<textus>("x")
    sit numerus n ← s.set<numerus>(5)
    nota t
    nota n
}"#,
    );
}

#[test]
fn generic_sibling_method_cannot_see_method_type_param() {
    // Method type params are method-local: a sibling cannot name another's C.
    rejects_with_issue(
        r"genus Something<A> {
    varia A field
    functio run<C>(A b, C c) → A {
        redde b
    }
    functio sibling(C x) → C {
        redde x
    }
}",
        "unknown_type",
    );
}

#[test]
fn generic_class_type_param_in_field_type_accepts() {
    accepts(
        r"genus Something<A> {
    varia A field
}",
    );
}

#[test]
fn generic_method_on_generic_class_dual_witness_call_accepts() {
    // Class witness A=numerus and method witness C=textus at the same call.
    accepts(
        r#"genus Something<A> {
    functio run<C>(A b, C c) → A {
        sit C local ← c
        sit A a2 ← b
        redde a2
    }
}

functio make() → Something<numerus> {
    redde Something { }
}

incipit {
    sit Something<numerus> s ← make()
    sit numerus result ← s.run<textus>(1, "x")
    nota result
}"#,
    );
}

#[test]
fn generic_method_on_generic_class_swapped_args_rejects() {
    rejects_with_issue(
        r#"genus Something<A> {
    functio run<C>(A b, C c) → A {
        sit C local ← c
        sit A a2 ← b
        redde a2
    }
}

functio make() → Something<numerus> {
    redde Something { }
}

incipit {
    sit Something<numerus> s ← make()
    sit _ result ← s.run<textus>("x", 1)
}"#,
        "argument_type_mismatch",
    );
}

// --- S0-G8: rigid shape params on intrinsic-method calls ---

#[test]
fn parametric_vector_intrinsic_method_keeps_rigid_width() {
    // SameAsReceiver methods on a size-generic receiver: the body's rigid
    // `N` is not a call-site shape param to solve (no SEM014).
    accepts(
        r"functio normata<magnitudo N>(vector<f32, magnitudo N> v) → vector<f32, magnitudo N> {
    redde v.normalizata()
}",
    );
}

#[test]
fn parametric_vector_scalar_multiplica_splats() {
    // Scalar-operand elementwise multiply: the scalar broadcasts across the
    // lanes; the result keeps the receiver shape at any width. The copy-out
    // elementwise method is the perfect form `multiplicata`.
    accepts(
        r"functio scala<magnitudo N>(vector<f32, magnitudo N> v, f32 s) → vector<f32, magnitudo N> {
    redde v.multiplicata(s)
}",
    );
}

#[test]
fn vector_scalar_multiplica_rejects_foreign_element() {
    rejects_with_issue(
        r"functio scala(vector<f32, magnitudo 3> v, numerus n) → vector<f32, magnitudo 3> {
    redde v.multiplicata(n)
}",
        "argument_type_mismatch",
    );
}

#[test]
fn whole_shape_generic_accepts_a_callers_own_size_params() {
    // K14a: `size Shape` is a root hole and must bind from the argument.
    // K14b: the argument's `[M, N]` names the caller's own rigid size
    // parameters, which must survive the call boundary so the caller's
    // `[M, N]` return annotation still matches.
    accepts(
        r"functio keep<magnitudo Shape>(tensor<f32, Shape> value) → tensor<f32, Shape> {
    redde value
}

functio caller<magnitudo M, magnitudo N>(tensor<f32, [M, N]> value) → tensor<f32, [M, N]> {
    redde keep(value)
}

incipit {
    sit tensor<f32, [2, 3]> value ← vacua
    nota caller(value).longitudo()
}",
    );
}

#[test]
fn per_axis_generic_binds_to_a_callers_own_size_params() {
    // K14b: the callee's per-axis holes bind to the caller's `M`/`N`, not to
    // the argument's runtime axes.
    accepts(
        r"functio pass<magnitudo A, magnitudo B>(tensor<f32, [A, B]> value) → tensor<f32, [A, B]> {
    redde value
}

functio caller<magnitudo M, magnitudo N>(tensor<f32, [M, N]> value) → tensor<f32, [M, N]> {
    redde pass(value)
}

incipit {
    sit tensor<f32, [2, 3]> value ← vacua
    nota caller(value).longitudo()
}",
    );
}

// --- A ranged list keeps its range while the element parameter is bound ---

#[test]
fn generic_element_param_binds_a_ranged_list_argument() {
    accepts(
        r"functio numera<T>(lista<T, magnitudo 1…∞> xs) → numerus {
    redde xs.longitudo()
}

incipit {
    sit lista<numerus, magnitudo 1…∞> xs ← [7]
    nota numera<numerus>(xs)
}",
    );
}

// --- Size parameters of a generic class instantiate at the receiver ---

#[test]
fn applied_class_method_return_type_instantiates_size_param() {
    // `Box<3>.mensura()` returns `vector<f32, size 3>`: the class's `N` is the
    // receiver's size argument, not a rigid parameter.
    accepts(
        r"genus Capsa<magnitudo N> {
    varia vector<f32, magnitudo N> minima

    functio mensura() → vector<f32, magnitudo N> {
        redde ego.minima
    }
}

functio a(Capsa<3> b) → vector<f32, magnitudo 3> {
    redde b.mensura()
}",
    );
}

#[test]
fn applied_class_method_parameter_type_instantiates_size_param() {
    accepts(
        r"genus Capsa<magnitudo N> {
    varia vector<f32, magnitudo N> minima

    functio muta(vector<f32, magnitudo N> offset) → vector<f32, magnitudo N> {
        redde offset
    }
}

functio a(Capsa<3> b, vector<f32, magnitudo 3> o) → vector<f32, magnitudo 3> {
    redde b.muta(o)
}",
    );
}

#[test]
fn applied_class_method_parameter_rejects_foreign_width() {
    rejects_with_issue(
        r"genus Capsa<magnitudo N> {
    varia vector<f32, magnitudo N> minima

    functio muta(vector<f32, magnitudo N> offset) → vector<f32, magnitudo N> {
        redde offset
    }
}

functio a(Capsa<3> b, vector<f32, magnitudo 2> o) → vector<f32, magnitudo 3> {
    redde b.muta(o)
}",
        "incompatible_tensor_index",
    );
}

#[test]
fn applied_class_method_return_rejects_foreign_width() {
    rejects_with_issue(
        r"genus Capsa<magnitudo N> {
    varia vector<f32, magnitudo N> minima

    functio mensura() → vector<f32, magnitudo N> {
        redde ego.minima
    }
}

functio a(Capsa<3> b) → vector<f32, magnitudo 2> {
    redde b.mensura()
}",
        "incompatible_tensor_index",
    );
}

#[test]
fn applied_class_field_type_instantiates_size_param() {
    accepts(
        r"genus Capsa<magnitudo N> {
    varia vector<f32, magnitudo N> minima
}

functio a(Capsa<3> b) → vector<f32, magnitudo 3> {
    redde b.minima
}",
    );
}

// --- Type parameters of a generic class instantiate at the receiver ---

#[test]
fn applied_class_method_return_type_instantiates_type_param() {
    accepts(
        r"genus Cista<T> {
    varia T res

    functio accipe() → T {
        redde ego.res
    }
}

functio a(Cista<numerus> b) → numerus {
    redde b.accipe()
}",
    );
}

#[test]
fn applied_class_method_return_rejects_foreign_type() {
    rejects_with_issue(
        r"genus Cista<T> {
    varia T res

    functio accipe() → T {
        redde ego.res
    }
}

functio a(Cista<numerus> b) → textus {
    redde b.accipe()
}",
        "return_type_mismatch",
    );
}

#[test]
fn applied_class_method_argument_type_is_checked() {
    rejects_with_issue(
        r#"genus Cista<T> {
    varia T res

    functio idem(T v) → T {
        redde v
    }
}

functio a(Cista<numerus> b) → textus {
    redde b.idem("x")
}"#,
        "argument_type_mismatch",
    );
}

#[test]
fn applied_class_method_argument_of_the_instantiated_type_checks() {
    accepts(
        r"genus Cista<T> {
    varia T res

    functio idem(T v) → T {
        redde v
    }
}

functio a(Cista<numerus> b) → numerus {
    redde b.idem(1)
}",
    );
}

#[test]
fn applied_nested_generic_class_method_instantiates() {
    accepts(
        r"genus Cista<T> {
    varia T res

    functio accipe() → T {
        redde ego.res
    }
}

functio a(Cista<Cista<numerus>> b) → Cista<numerus> {
    redde b.accipe()
}",
    );
    rejects_with_issue(
        r"genus Cista<T> {
    varia T res

    functio accipe() → T {
        redde ego.res
    }
}

functio a(Cista<Cista<numerus>> b) → numerus {
    redde b.accipe()
}",
        "return_type_mismatch",
    );
}

#[test]
fn applied_two_parameter_class_method_instantiates_each() {
    accepts(
        r"genus Par<K, V> {
    varia K clavis
    varia V valor

    functio accipeValor() → V {
        redde ego.valor
    }
}

functio a(Par<textus, numerus> p) → numerus {
    redde p.accipeValor()
}",
    );
    rejects_with_issue(
        r"genus Par<K, V> {
    varia K clavis
    varia V valor

    functio accipeValor() → V {
        redde ego.valor
    }
}

functio a(Par<textus, numerus> p) → textus {
    redde p.accipeValor()
}",
        "return_type_mismatch",
    );
}

#[test]
fn applied_class_method_without_type_param_is_unchanged() {
    accepts(
        r"genus Cista<T> {
    varia T res

    functio numerare() → numerus {
        redde 1
    }
}

functio a(Cista<textus> b) → numerus {
    redde b.numerare()
}",
    );
}

#[test]
fn applied_class_method_call_through_ego_checks() {
    accepts(
        r"genus Cista<T> {
    varia T res

    functio accipe() → T {
        redde ego.res
    }

    functio itera() → T {
        redde ego.accipe()
    }
}",
    );
}
