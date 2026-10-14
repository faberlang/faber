//! MU-1 unmarked-semantics invariant pins (D-1, D-8).
//!
//! These tests lock the unmarked laws: alias-on-bind, var→mut mutation (a const argument is rejected), and
//! param mutation through `in`. The tcs1 language decision narrows D-1's
//! shallow law for the binding form: an in-place mutating intrinsic on an
//! immutable binding is a reassignment-class error, so the old shallow-const
//! pin now asserts the rejection.

use crate::test_support::{analyze_ok_la, assert_rejects_with_issue_la};

#[test]
fn baseline_const_list_in_place_append_requires_varia() {
    assert_rejects_with_issue_la(
        r"
incipit {
    fixum lista<numerus> xs ← vacua
    xs.appende(4)
}
",
        "imperativus_operator_method_requires_varia_receiver",
    );
}

#[test]
fn baseline_alias_on_bind_is_legal() {
    let _ = analyze_ok_la(
        r"
incipit {
    varia lista<numerus> a ← [1, 2, 3]
    varia lista<numerus> b ← a
    b.appende(4)
    nota a.longitudo()
}
",
    );
}

#[test]
fn baseline_const_into_mut_param_is_rejected() {
    // The argument of a `mut` parameter must be a writable place: a `const`
    // binding is not (a `varia` binding is, see the twin below).
    assert_rejects_with_issue_la(
        r"
functio poke(in lista<numerus> xs) → vacuum {
    xs.appende(4)
}
incipit {
    fixum lista<numerus> a ← [1, 2, 3]
    poke(in a)
    nota a.longitudo()
}
",
        "mut_argument_not_writable",
    );
}

#[test]
fn baseline_var_into_mut_param_is_legal() {
    let _ = analyze_ok_la(
        r"
functio poke(in lista<numerus> xs) → vacuum {
    xs.appende(4)
}
incipit {
    varia lista<numerus> a ← [1, 2, 3]
    poke(in a)
    nota a.longitudo()
}
",
    );
}

#[test]
fn baseline_unmarked_param_in_place_mutation_requires_in_mode() {
    // An unmarked parameter is a write-once binding, so its in-place mutation
    // is rejected like any immutable binding; `in` mode alone carries the
    // mutation right (no access marker needed).
    assert_rejects_with_issue_la(
        r"
functio poke(lista<numerus> xs) → vacuum {
    xs.appende(4)
}
",
        "imperativus_operator_method_requires_varia_receiver",
    );
}

#[test]
fn baseline_summa_unmarked_out_shape_is_rejected() {
    // Reading A: an unmarked parameter is shared read-only, so writing `out`
    // without `in` is SEM056.
    assert_rejects_with_issue_la(
        r"
functio summa(tf32[256] a, tf32[1] out, u32 id) → vacuum {
    fixum f32 total ← a.summa()
    out[id] ← total
}
",
        "assign_to_nonmut_tensor_param",
    );
}

#[test]
fn baseline_summa_mut_out_shape_is_legal() {
    let _ = analyze_ok_la(
        r"
functio summa(tf32[256] a, in tf32[1] out, u32 id) → vacuum {
    fixum f32 total ← a.summa()
    out[id] ← total
}
",
    );
}
