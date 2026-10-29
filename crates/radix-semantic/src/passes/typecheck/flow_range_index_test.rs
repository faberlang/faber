//! Index proofs (need, slice 4): which accesses the checker records as
//! proven, and which keep their run-time bounds check.
//!
//! Each fixture is read through [`proofs`]: one flag per `Index` expression in
//! source order, true when the access id is in `proven_indexes`. Fixtures use
//! latin spellings (`itera ab`, `sit`, `varia`).

use radix_hir::visit::{HirVisitor, walk_expr};
use radix_hir::{HirExpression, HirExpressionKind, HirId, HirModule};

use crate::test_support;

#[derive(Default)]
struct IndexIds(Vec<HirId>);

impl HirVisitor for IndexIds {
    fn visit_expr(&mut self, expr: &HirExpression) {
        if matches!(expr.kind, HirExpressionKind::Index(..)) {
            self.0.push(expr.id);
        }
        walk_expr(self, expr);
    }
}

fn index_ids(hir: &HirModule) -> Vec<HirId> {
    let mut ids = IndexIds::default();
    ids.visit_program(hir);
    ids.0
}

/// One flag per `Index` expression, in visit order: proven or checked.
fn proofs(source: &str) -> Vec<bool> {
    let result = test_support::analyze_source(source);
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
    let hir = result.hir.as_ref().expect("hir");
    index_ids(hir)
        .into_iter()
        .map(|id| result.shape_obligations.proven_indexes.contains(&id))
        .collect()
}

/// `(proven, total)` over the `Index` expressions of `source`.
fn tally(source: &str) -> (usize, usize) {
    let seen = proofs(source);
    (seen.iter().filter(|proven| **proven).count(), seen.len())
}

fn list_fn(params: &str, body: &str) -> String {
    format!(
        "functio g(numerus a) → numerus {{ redde a }}\n\
         functio f({params}) → numerus {{\n    varia numerus n ← 0\n{body}\n    redde n\n}}"
    )
}

// ── lists ─────────────────────────────────────────────────────────────────

#[test]
fn a_loop_bounded_list_read_is_proven() {
    let source = list_fn(
        "lista<numerus> s",
        "    itera ab 0‥s.longitudo() sit i { n ← s[i] }",
    );
    assert_eq!(proofs(&source), vec![true]);
}

#[test]
fn an_unbounded_list_read_keeps_its_check() {
    let source = list_fn("lista<numerus> s, numerus x", "    n ← s[x]");
    assert_eq!(proofs(&source), vec![false]);
}

#[test]
fn a_guard_without_a_lower_bound_is_not_a_proof() {
    // `x ≺ len` bounds the top only: a negative `x` would still fault.
    let source = list_fn(
        "lista<numerus> s, numerus x",
        "    si x ≺ s.longitudo() { n ← s[x] }",
    );
    assert_eq!(proofs(&source), vec![false]);
}

#[test]
fn a_clamped_and_guarded_index_is_proven_until_a_call() {
    let source = list_fn(
        "varia lista<numerus> s, numerus x",
        "    sit numerus c ← x ⤒ 0\n    si c ≺ s.longitudo() {\n        n ← s[c]\n        n ← g(1)\n        n ← s[c]\n    }",
    );
    assert_eq!(
        proofs(&source),
        vec![true, false],
        "the call may change the length, so the second read keeps its check"
    );
}

#[test]
fn a_pure_length_read_does_not_drop_the_proof() {
    let source = list_fn(
        "varia lista<numerus> s, numerus x",
        "    sit numerus c ← x ⤒ 0\n    si c ≺ s.longitudo() {\n        n ← s.longitudo()\n        n ← s[c]\n    }",
    );
    assert_eq!(proofs(&source), vec![true]);
}

#[test]
fn a_loop_body_that_can_write_the_list_proves_nothing() {
    // The next iteration runs after the append, and the bound was taken at entry.
    let source = list_fn(
        "varia lista<numerus> s",
        "    itera ab 0‥s.longitudo() sit i {\n        n ← s[i]\n        s.appende(1)\n    }",
    );
    assert_eq!(proofs(&source), vec![false]);
}

#[test]
fn a_store_into_a_mutable_list_keeps_its_check() {
    // The v1 kill rule: a store through a projection kills the facts about
    // every mutable source before the target is typed, the list's live length
    // included, so the target has nothing to be proven by.
    let source = list_fn(
        "varia lista<numerus> s",
        "    itera ab 0‥s.longitudo() sit i { s[i] ← 1 }",
    );
    assert_eq!(proofs(&source), vec![false]);
}

#[test]
fn a_store_is_proved_after_its_right_hand_side() {
    // The call on the right runs before the store and may change the length:
    // the target is proven against what survives it, so it is not proven.
    let source = list_fn(
        "varia lista<numerus> s, numerus x",
        "    sit numerus c ← x ⤒ 0\n    si c ≺ s.longitudo() {\n        s[c] ← g(c)\n    }",
    );
    assert_eq!(proofs(&source), vec![false]);
}

#[test]
fn an_index_read_inside_a_store_target_proves_nothing() {
    // The inner `ys[k]` is evaluated at the store, after the right-hand side.
    let source = "functio g(numerus a) → numerus { redde a }\n\
         functio f(varia lista<numerus> xs, lista<numerus> ys) → numerus {\n    \
         itera ab 0‥ys.longitudo() sit k {\n        xs[ys[k]] ← g(k)\n    }\n    redde 0\n}";
    let seen = proofs(source);
    assert_eq!(seen.len(), 2);
    assert!(
        seen.iter().all(|proven| !proven),
        "neither the target nor the read inside it is proven: {seen:?}"
    );
}

#[test]
fn a_closure_body_does_not_inherit_facts_about_a_mutable_list() {
    let source = list_fn(
        "varia lista<numerus> s",
        "    itera ab 0‥s.longitudo() sit i {\n        sit _ f ← numerus x ∴ s[i] + x\n        n ← f(0)\n    }",
    );
    assert_eq!(
        proofs(&source),
        vec![false],
        "the closure may run after the list shrank"
    );
}

// ── aliased lists ─────────────────────────────────────────────────────────
//
// `sit xs ← ys` shares the list: a write through `ys` changes `xs.longitudo()`
// although `xs` itself is immutable. A fact bounded by a list's live length
// therefore dies at every call and store, whether the list binding is
// `varia` or not. Each fixture reads `xs[i]` after a write through another
// name and must keep its check.

#[test]
fn a_let_alias_of_a_mutable_list_keeps_its_check_across_a_write() {
    let source = list_fn(
        "varia lista<numerus> ys",
        "    sit lista<numerus> xs ← ys
    itera ab 0‥xs.longitudo() sit i {
        ys.detrahe()
        n ← xs[i]
    }",
    );
    assert_eq!(proofs(&source), vec![false]);
}

#[test]
fn a_let_alias_of_a_mutable_list_keeps_its_check_across_a_call() {
    let source = list_fn(
        "varia lista<numerus> ys",
        "    sit lista<numerus> xs ← ys
    itera ab 0‥xs.longitudo() sit i {
        n ← g(i)
        n ← xs[i]
    }",
    );
    assert_eq!(proofs(&source), vec![false]);
}

#[test]
fn a_shared_parameter_keeps_its_check_beside_a_mutable_one() {
    // `f(a, a)` binds both parameters to one list.
    let source = list_fn(
        "lista<numerus> xs, varia lista<numerus> ys",
        "    itera ab 0‥xs.longitudo() sit i {
        ys.detrahe()
        n ← xs[i]
    }",
    );
    assert_eq!(proofs(&source), vec![false]);
}

#[test]
fn a_list_read_from_a_field_keeps_its_check_across_a_write() {
    let source = "genus Capsa { varia lista<numerus> res }
         functio f(varia Capsa b) → numerus {
    varia numerus n ← 0
             sit lista<numerus> xs ← b.res
             itera ab 0‥xs.longitudo() sit i {
        b.res.detrahe()
        n ← xs[i]
    }
    redde n
}";
    assert_eq!(proofs(source), vec![false]);
}

#[test]
fn a_list_read_from_another_collection_keeps_its_check_across_a_write() {
    let source = list_fn(
        "varia lista<numerus> ys",
        "    varia lista<lista<numerus>> outer ← vacua
    outer.appende(ys)
    sit lista<numerus> xs ← outer[0]
    itera ab 0‥xs.longitudo() sit i {
        ys.detrahe()
        n ← xs[i]
    }",
    );
    // `outer[0]` has no named list source and is not proven; neither is `xs[i]`.
    assert_eq!(proofs(&source), vec![false, false]);
}

#[test]
fn a_list_written_by_a_captured_closure_keeps_its_check() {
    let source = list_fn(
        "varia lista<numerus> ys",
        "    sit lista<numerus> xs ← ys
    sit _ shrink ← numerus k ∴ ys.appende(k)
    itera ab 0‥xs.longitudo() sit i {
        shrink(i)
        n ← xs[i]
    }",
    );
    assert_eq!(proofs(&source), vec![false]);
}

#[test]
fn a_text_length_survives_a_call() {
    // A text value is never written in place, so no other name changes the
    // length of a `sit` or parameter text.
    let source = list_fn(
        "textus t",
        "    itera ab 0‥t.longitudo() sit i {
        n ← g(i)
        sit textus c ← t[i]
    }",
    );
    assert_eq!(proofs(&source), vec![true]);
}

// ── tensors (kernel bodies) ───────────────────────────────────────────────

const KERNEL: &str = "@ nucleum\nfunctio k<magnitudo N, magnitudo M>(tensor<f32, [N, M]> src, tensor<f32, [N, M]> other, tensor<f32, [M, N]> flipped, varia tensor<f32, [N, M]> dst, i32 shift, i32 free) → vacuum {\n    itera ex src apud [i, j] filum f sit v {\n";

fn kernel(body: &str) -> String {
    format!("{KERNEL}{body}\n    }}\n}}")
}

#[test]
fn a_coordinate_access_of_its_own_tensor_is_proven() {
    let source = kernel("        dst[[i, j]] ← other[[i, j]]");
    assert_eq!(tally(&source), (2, 2));
}

#[test]
fn a_shifted_coordinate_is_not_proven() {
    // The c4-index-fault shape: `j + shift` has no fact, so the read keeps its
    // check although `i`, `j` and the store are proven.
    let source =
        kernel("        sit i32 column ← j\n        dst[[i, j]] ← other[[i, column + shift]]");
    assert_eq!(tally(&source), (1, 2));
}

#[test]
fn each_component_is_proven_against_its_own_axis() {
    // `flipped` is `[M, N]`: the coordinates of `src` name the other axes.
    let source = kernel("        dst[[i, j]] ← flipped[[i, j]]");
    assert_eq!(tally(&source), (1, 2));
}

#[test]
fn a_partial_proof_proves_nothing() {
    // `i` is proven on axis 0 but `free` is a parameter: the access is checked.
    let source = kernel("        dst[[i, j]] ← other[[i, free]]");
    assert_eq!(tally(&source), (1, 2));
}

#[test]
fn a_swapped_coordinate_pair_is_not_proven() {
    // Both components are in range of *an* axis, not their own.
    let source = kernel("        dst[[i, j]] ← other[[j, i]]");
    assert_eq!(tally(&source), (1, 2));
}

#[test]
fn a_literal_extent_proves_a_range_loop_coordinate() {
    let source = "@ nucleum\nfunctio k(tensor<f32, [4]> src, varia tensor<f32, [4]> dst) → vacuum {\n    itera ab 0‥4 sit i {\n        dst[i] ← src[i]\n    }\n}";
    assert_eq!(tally(source), (2, 2));
}

#[test]
fn a_range_loop_past_the_extent_is_not_proven() {
    let source = "@ nucleum\nfunctio k(tensor<f32, [4]> src, varia tensor<f32, [4]> dst) → vacuum {\n    itera ab 0‥5 sit i {\n        dst[i] ← src[i]\n    }\n}";
    assert_eq!(tally(source), (0, 2));
}

// ── effects between a guard and its use ───────────────────
//
// A length fact dies at every effect that can change the length. The guard's
// facts are rebuilt from syntax after the whole condition was checked, so an
// effect that runs after the fact was read, inside the same condition or in
// a loop around it, must take the fact with it.

const SHRINKER: &str = "functio contrahe(varia lista<numerus> zs) → bivalens {\n    zs.detrahe()\n    redde verum\n}\n";

fn with_shrinker(source: &str) -> String {
    format!("{SHRINKER}{source}")
}

#[test]
fn a_later_conjunct_that_mutates_the_list_kills_the_guard() {
    let source = with_shrinker(
        "functio f(varia lista<numerus> xs, numerus i) → numerus {
    si i ≥ 0 et i ≺ xs.longitudo() et contrahe(varia xs) {
        redde xs[i]
    }
    redde 0
}",
    );
    assert_eq!(proofs(&source), vec![false]);
}

#[test]
fn a_mutating_call_in_the_condition_kills_a_guard_before_it() {
    let source = list_fn(
        "varia lista<numerus> xs, numerus i",
        "    si i ≥ 0 et i ≺ xs.longitudo() et g(1) ≡ 1 {\n        n ← xs[i]\n    }",
    );
    assert_eq!(proofs(&source), vec![false]);
}

#[test]
fn a_mutating_intrinsic_in_a_later_conjunct_kills_the_guard() {
    let source = list_fn(
        "varia lista<numerus> xs, numerus i",
        "    si i ≥ 0 et i ≺ xs.longitudo() et xs.detrahe() non est nihil {\n        n ← xs[i]\n    }",
    );
    assert_eq!(proofs(&source), vec![false]);
}

#[test]
fn a_guard_after_the_last_effect_in_the_condition_still_proves() {
    // The call runs first; the length is read after it, so the fact is fresh.
    let source = with_shrinker(
        "functio f(varia lista<numerus> xs, numerus i) → numerus {
    si contrahe(varia xs) et i ≥ 0 et i ≺ xs.longitudo() {
        redde xs[i]
    }
    redde 0
}",
    );
    assert_eq!(proofs(&source), vec![true]);
}

#[test]
fn a_disjunct_that_mutates_after_the_guard_kills_it() {
    // `si not (A or B)` carries the else facts of both disjuncts.
    let source = with_shrinker(
        "functio f(varia lista<numerus> xs, numerus i) → numerus {
    si non (i ≺ 0 aut i ≥ xs.longitudo() aut contrahe(varia xs)) {
        redde xs[i]
    }
    redde 0
}",
    );
    assert_eq!(proofs(&source), vec![false]);
}

#[test]
fn pure_conjuncts_after_a_guard_keep_the_proof() {
    // Reading the length again, or comparing integers, is not an effect.
    let source = list_fn(
        "varia lista<numerus> xs, numerus i, numerus j",
        "    si i ≥ 0 et i ≺ xs.longitudo() et j ≥ 0 et xs.longitudo() ≻ 0 {\n        n ← xs[i]\n    }",
    );
    assert_eq!(proofs(&source), vec![true]);
}

#[test]
fn a_while_that_empties_the_list_kills_the_proof_made_before_it() {
    let source = list_fn(
        "varia lista<numerus> xs, numerus i",
        "    si i ≥ 0 et i ≺ xs.longitudo() {
        dum xs.longitudo() ≻ 0 {
            xs.detrahe()
        }
        n ← xs[i]
    }",
    );
    assert_eq!(proofs(&source), vec![false]);
}

#[test]
fn a_while_that_calls_out_kills_the_proof_made_before_it() {
    let source = list_fn(
        "varia lista<numerus> xs, numerus i",
        "    si i ≥ 0 et i ≺ xs.longitudo() {
        dum n ≺ 3 {
            n ← g(n)
        }
        n ← xs[i]
    }",
    );
    assert_eq!(proofs(&source), vec![false]);
}

#[test]
fn a_while_that_only_reads_keeps_the_proof_made_before_it() {
    let source = list_fn(
        "varia lista<numerus> xs, numerus i",
        "    si i ≥ 0 et i ≺ xs.longitudo() {
        dum n ≺ xs.longitudo() {
            n ← n + 1
        }
        n ← xs[i]
    }",
    );
    assert_eq!(proofs(&source), vec![true]);
}

#[test]
fn a_range_loop_that_empties_the_list_kills_the_proof_made_before_it() {
    let source = list_fn(
        "varia lista<numerus> xs, numerus i",
        "    si i ≥ 0 et i ≺ xs.longitudo() {
        itera ab 0‥3 sit k {
            xs.detrahe()
        }
        n ← xs[i]
    }",
    );
    assert_eq!(proofs(&source), vec![false]);
}

#[test]
fn a_fact_proven_inside_a_loop_does_not_outlive_a_break() {
    // The loop is left through `rumpe` before the length guard is reached on
    // the other path, so the guard's fact is not true after the loop.
    let source = list_fn(
        "lista<numerus> xs, numerus i",
        "    si i ≥ 0 {
        itera ab 0‥3 sit k {
            si i ≥ xs.longitudo() { rumpe }
        }
        n ← xs[i]
    }",
    );
    assert_eq!(proofs(&source), vec![false]);
}

#[test]
fn a_loop_binding_assigned_in_the_body_loses_its_proof_after_the_loop() {
    let source = list_fn(
        "lista<numerus> xs, numerus i",
        "    varia numerus j ← 0
    si j ≺ xs.longitudo() {
        dum n ≺ 3 {
            j ← j + 5
            n ← n + 1
        }
        n ← xs[j]
    }",
    );
    assert_eq!(proofs(&source), vec![false]);
}
