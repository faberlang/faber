//! N3 red-first pins for cross-statement flow facts and the ruled join.
//!
//! These fixtures exercise the public typechecker seam rather than the
//! assignment funnel: facts are expected to be selected before ordinary
//! binding lookup and then consumed by the existing access/type checks.

use crate::test_support;

fn nominal_union() -> &'static str {
    "genus Errata {\n    textus nuntius\n}\n\ngenus Bonum {\n    textus fructus\n}\n\n"
}

fn three_nominal_union() -> &'static str {
    "genus Errata {\n    textus nuntius\n}\n\ngenus Medius {\n    textus proprium\n}\n\ngenus Tertius {\n    textus clausum\n}\n\n"
}

#[test]
fn early_return_eliminates_the_tested_member_for_later_statements() {
    let source = format!(
        "{decls}functio f(Errata ∪ Bonum r) → textus {{\n\
             si r est Errata {{ redde r.nuntius }}\n\
             redde r.fructus\n\
         }}",
        decls = nominal_union(),
    );
    test_support::assert_accepts(&source);
}

#[test]
fn diverging_edge_does_not_block_the_surviving_else_fact() {
    let source = format!(
        "{decls}functio f(Errata ∪ Bonum r) → textus {{\n\
             si r est Errata {{ redde r.nuntius }} secus {{ nota \"bonum\" }}\n\
             redde r.fructus\n\
         }}",
        decls = nominal_union(),
    );
    test_support::assert_accepts(&source);
}

#[test]
fn branch_specific_facts_drop_at_a_normal_join() {
    let source = format!(
        "{decls}functio f(Errata ∪ Bonum r, bivalens c) → textus {{\n\
             si c {{ adfirma r est Errata }} secus {{ adfirma r est Bonum }}\n\
             redde r.nuntius\n\
         }}",
        decls = nominal_union(),
    );
    test_support::assert_rejects_with_issue(&source, "union_member_not_common");
}

#[test]
fn member_subset_intersection_keeps_a_fact_common_to_both_edges() {
    let source = format!(
        "{decls}functio f(Errata ∪ Medius ∪ Tertius r, bivalens c) → textus {{\n\
             si c {{ adfirma r non est Errata }} secus {{ adfirma r non est Tertius }}\n\
             redde r.proprium\n\
         }}",
        decls = three_nominal_union(),
    );
    test_support::assert_accepts(&source);
}

#[test]
fn assignment_drops_a_null_fact_before_the_next_use() {
    test_support::assert_rejects_with_issue(
        "functio f(textus ∪ nihil x) → numerus {\n\
             si x non est nihil {\n\
                 x ← nihil\n\
                 redde x.longitudo()\n\
             }\n\
             redde 0\n\
         }",
        "optional_receiver_requires_chain",
    );
}

#[test]
fn assignment_on_one_join_edge_drops_the_incoming_fact() {
    test_support::assert_rejects_with_issue(
        "functio f(textus ∪ nihil x, bivalens c) → numerus {\n\
             adfirma x non est nihil\n\
             si c { x ← nihil } secus { nota x.longitudo() }\n\
             redde x.longitudo()\n\
         }",
        "optional_receiver_requires_chain",
    );
}

#[test]
fn loop_entry_drops_facts_for_bindings_touched_by_the_body() {
    test_support::assert_rejects_with_issue(
        "functio f(textus ∪ nihil x) → numerus {\n\
             adfirma x non est nihil\n\
             dum falsum {\n\
                 nota x.longitudo()\n\
                 x ← nihil\n\
             }\n\
             redde 0\n\
         }",
        "optional_receiver_requires_chain",
    );
}

#[test]
fn immutable_loop_invariant_facts_may_enter_the_body() {
    test_support::assert_accepts(
        "functio f(textus ∪ nihil x) → numerus {\n\
             adfirma x non est nihil\n\
             dum falsum {\n\
                 redde x.longitudo()\n\
             }\n\
             redde 0\n\
         }",
    );
}
