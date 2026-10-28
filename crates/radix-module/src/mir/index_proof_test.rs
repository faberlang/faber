//! The checker's index proofs reach MIR (need, slice 4): an access
//! whose every index component the checker proved in range lowers to a
//! `ProvenIndex` projection, and an access nothing proved lowers to `Index`.
//! No MIR stage derives a proof of its own.

use crate::codegen::Target;
use crate::driver::{AnalyzedModule, Config, Session};
use crate::mir::{MirLoweringOptions, lower_analyzed_unit};
use crate::program::analyze_prepared_program_name;
use radix_mir::visit::MirVisitor;
use radix_mir::{MirPlace, MirProjection};

fn analyze(source: &str) -> AnalyzedModule {
    analyze_prepared_program_name(
        &Session::new(Config::default().with_target(Target::HirTypeScript)),
        "index-proof.fab",
        &format!("+++\nlocale = \"en\"\n+++\n{source}"),
    )
    .unwrap_or_else(|errors| panic!("source must analyze: {errors:?}"))
}

#[derive(Default)]
struct Projections {
    checked: usize,
    proven: usize,
}

impl MirVisitor for Projections {
    fn visit_place(&mut self, place: &MirPlace) {
        for projection in &place.projections {
            match projection {
                MirProjection::Index(_) => self.checked += 1,
                MirProjection::ProvenIndex(_) => self.proven += 1,
                _ => {}
            }
        }
        radix_mir::visit::walk_place(self, place);
    }
}

/// `(checked, proven)` index projections in the lowered program of `source`.
fn index_projections(source: &str) -> (usize, usize) {
    let unit = analyze(source);
    let lowered =
        lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default()).expect("MIR lowering validates");
    let mut seen = Projections::default();
    for function in &lowered.validated.program().functions {
        seen.visit_function(function);
    }
    (seen.checked, seen.proven)
}

#[test]
fn a_loop_bounded_list_read_lowers_as_a_proven_projection() {
    let source = r#"
fn total(list<int> xs) → int {
    mut int sum ← 0
    for range 0‥xs.length() let i {
        sum ← sum + xs[i]
    }
    return sum
}

main {
    print total([1, 2, 3])
}
"#;
    assert_eq!(index_projections(source), (0, 1));
}

#[test]
fn an_unbounded_list_read_lowers_as_a_checked_projection() {
    let source = r#"
fn pick(list<int> xs, int at) → int {
    return xs[at]
}

main {
    print pick([1, 2, 3], 1)
}
"#;
    assert_eq!(index_projections(source), (1, 0));
}

#[test]
fn only_the_proven_access_of_two_is_proven() {
    let source = r#"
fn mixed(list<int> xs, int at) → int {
    mut int sum ← 0
    for range 0‥xs.length() let i {
        sum ← sum + xs[i]
    }
    return sum + xs[at]
}

main {
    print mixed([1, 2, 3], 1)
}
"#;
    assert_eq!(index_projections(source), (1, 1));
}

#[test]
fn a_call_after_the_proof_leaves_the_access_checked() {
    let source = r#"
fn grow(mut list<int> xs, int at) → int {
    let int c ← at ⤒ 0
    if c ≺ xs.length() {
        let int first ← xs[c]
        xs.append(first)
        return xs[c]
    }
    return 0
}

main {
    mut list<int> xs ← [1, 2, 3]
    print grow(mut xs, 1)
}
"#;
    assert_eq!(index_projections(source), (1, 1));
}
