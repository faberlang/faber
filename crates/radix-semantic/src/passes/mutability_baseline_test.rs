//! MU-1 unmarked-semantics invariant pins (D-1, D-8).
//!
//! These tests lock today's behavior: shallow const, alias-on-bind,
//! const→mut mutation, and unmarked-param mutation. Every later mutability
//! unit keeps these green.

use crate::test_support::analyze_ok;

#[test]
fn baseline_shallow_const_list_append_is_legal() {
    analyze_ok(
        r"
incipit {
    fixum lista<numerus> xs ← vacua
    xs.appende(4)
}
",
    );
}

#[test]
fn baseline_alias_on_bind_is_legal() {
    analyze_ok(
        r"
incipit {
    fixum lista<numerus> a ← [1, 2, 3]
    fixum lista<numerus> b ← a
    b.appende(4)
    nota a.longitudo()
}
",
    );
}

#[test]
fn baseline_const_into_mut_param_is_legal() {
    analyze_ok(
        r"
functio poke(in lista<numerus> xs) → vacuum {
    xs.appende(4)
}
incipit {
    fixum lista<numerus> a ← [1, 2, 3]
    poke(a)
    nota a.longitudo()
}
",
    );
}

#[test]
fn baseline_unmarked_param_mutation_is_legal() {
    analyze_ok(
        r"
functio poke(lista<numerus> xs) → vacuum {
    xs.appende(4)
}
incipit {
    fixum lista<numerus> a ← [1, 2, 3]
    poke(a)
    nota a.longitudo()
}
",
    );
}

#[test]
fn baseline_summa_unmarked_out_shape_is_legal() {
    analyze_ok(
        r"
functio summa(tf32[256] a, tf32[1] out, u32 id) → vacuum {
    fixum f32 total ← a.summa()
    out[id] ← total
}
",
    );
}
