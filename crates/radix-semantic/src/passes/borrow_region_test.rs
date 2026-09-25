//! BRI-1/BRI-3 drivers for the HIR-hosted borrow-region contract.
//!
//! The delivery artifact settles the host as a semantic CFG derived from
//! typed HIR. Locations retain HIR identity (`HirId`/`DefId`/`Span`), and a
//! borrow region ends at the borrow's last use rather than at HIR block exit.
//! The parity contract is `AcceptedApproximation ⊆ AcceptedRegions`: a
//! region-based checker may accept these cases after last-use inference, but
//! it must not reject anything accepted by the current approximation.
//!
//! The cyclic fixtures below were pinned red against the retired block-scope
//! approximation (BRI-1). Since BRI-3 routed marked-value diagnostics through
//! the region engine, both programs analyze successfully: the marked mutable
//! borrow is last used before the source write, so its region is dead at the
//! write even though the loop back-edge re-enters the condition.

use super::borrow_region;
use super::borrow_region_cfg::{CfgEdgeKind, CfgNodeKind};
use crate::SemanticErrorKind;
use crate::test_support::{analyze_ok, analyze_source, issue};
use radix_hir::visit::{HirVisitor, walk_expr};
use radix_hir::{HirBlock, HirExpression, HirExpressionKind, HirId};

fn assert_region_accepted(source: &str, label: &str) {
    // POST-BRI-3: the production path consults the region facts, so the old
    // false `write_while_borrowed` conflict must be gone.
    let result = analyze_source(source);
    assert!(
        result.is_ok(),
        "{label} must be region-accepted; diagnostics: {:?}",
        result.diagnostics
    );
    assert!(
        !result.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == SemanticErrorKind::MutableBorrowConflict
                && issue(diagnostic) == Some("write_while_borrowed")
        }),
        "{label} must not report write_while_borrowed; diagnostics: {:?}",
        result.diagnostics
    );
}

fn analyzed_region(source: &str) -> borrow_region::RegionFacts {
    let result = analyze_source(source);
    let hir = result
        .hir
        .as_ref()
        .expect("red drivers still produce typed HIR");
    let access_facts = result
        .access_facts
        .as_ref()
        .expect("typed HIR must carry the single access-fact table");
    borrow_region::analyze(hir, &result.types, access_facts).expect("supported HIR CFG")
}

#[derive(Default)]
struct ControlShapes {
    dum: Option<(HirId, Vec<radix_hir::HirId>)>,
    infinite_loop: Option<(HirId, Vec<radix_hir::HirId>)>,
}

impl HirVisitor for ControlShapes {
    fn visit_expr(&mut self, expression: &HirExpression) {
        match &expression.kind {
            HirExpressionKind::Dum(condition, body) => {
                self.dum = Some((condition.id, statement_ids(body)));
            }
            HirExpressionKind::Loop(body) => {
                self.infinite_loop = Some((expression.id, statement_ids(body)));
            }
            _ => {}
        }
        walk_expr(self, expression);
    }
}

fn statement_ids(block: &HirBlock) -> Vec<radix_hir::HirId> {
    block
        .statements
        .iter()
        .map(|statement| statement.id)
        .collect()
}

#[test]
fn dum_back_edge_releases_mutable_borrow_before_source_write() {
    // The body write is after the borrow's last use. The back-edge then
    // re-enters the condition and must not make the released loan loop-wide.
    let source = r"
incipit {
    varia lista<numerus> xs ← [1, 2, 3]
    dum xs.longitudo() < 5 {
        varia in lista<numerus> view ← xs
        nota view.longitudo()
        xs.appende(4)
    }
}
";

    // POST-BRI-3 payoff: last-use region inference accepts this fixture
    // through the production borrow path without the old false conflict.
    assert_region_accepted(source, "dum back-edge release-before-write");
}

#[test]
fn loop_rumpe_releases_mutable_borrow_before_source_write() {
    // The explicit `rumpe` supplies the loop exit while the fall-through body
    // edge returns to the next iteration. The source write follows the borrow
    // last use in the body.
    let source = r"
incipit {
    varia lista<numerus> xs ← [1, 2, 3]
    fac {
        varia in lista<numerus> view ← xs
        nota view.longitudo()
        xs.appende(4)
        rumpe
    } dum verum
}
";

    // POST-BRI-3 payoff: accepted while the explicit `rumpe` exit edge stays.
    assert_region_accepted(source, "loop rumpe release-before-write");
}

#[test]
fn cyclic_release_does_not_weaken_conflict_when_borrow_still_live() {
    // Same cyclic shape, but the source write precedes the borrow's last
    // use: the region is live at the write and SEM052 must still fire.
    let result = analyze_source(
        r"
incipit {
    varia lista<numerus> xs ← [1, 2, 3]
    dum xs.longitudo() < 5 {
        varia in lista<numerus> view ← xs
        xs.appende(4)
        nota view.longitudo()
    }
}
",
    );
    assert!(
        result.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == SemanticErrorKind::MutableBorrowConflict
                && issue(diagnostic) == Some("write_while_borrowed")
        }),
        "live region must keep write_while_borrowed; diagnostics: {:?}",
        result.diagnostics
    );
}

#[test]
fn production_path_consumes_the_handoff_table() {
    // The pipeline result carries the one access-fact table, and the region
    // facts the borrow path consulted were derived from that same table.
    let result = analyze_ok(
        r"
incipit {
    varia lista<numerus> xs ← [1, 2, 3]
    dum xs.longitudo() < 5 {
        varia in lista<numerus> view ← xs
        nota view.longitudo()
        xs.appende(4)
    }
}
",
    );
    let facts = result.access_facts.as_ref().expect("handoff table");
    let hir = result.hir.as_ref().expect("HIR");
    let regions = borrow_region::analyze(hir, &result.types, facts).expect("regions");
    assert!(regions.fact_reads() > 0, "engine must read the table");
}

#[test]
fn dum_cfg_has_condition_body_back_edge_and_exit() {
    let source = r"
incipit {
    varia lista<numerus> xs ← [1, 2, 3]
    dum xs.longitudo() < 5 {
        varia in lista<numerus> view ← xs
        nota view.longitudo()
        xs.appende(4)
    }
}
";
    let result = analyze_source(source);
    let hir = result.hir.as_ref().expect("HIR");
    let mut shapes = ControlShapes::default();
    shapes.visit_program(hir);
    let (condition_id, body_ids) = shapes.dum.expect("dum shape");
    let facts = borrow_region::analyze(
        hir,
        &result.types,
        result.access_facts.as_ref().expect("access facts"),
    )
    .expect("dum CFG");
    let cfg = facts.cfg();
    let condition = cfg.node_for_hir_id(condition_id).expect("condition node");
    let first_body = cfg.node_for_hir_id(body_ids[0]).expect("body entry");
    let last_body = cfg
        .node_for_hir_id(*body_ids.last().expect("body statements"))
        .expect("body exit");

    assert!(
        cfg.successors(condition)
            .any(|edge| edge.to == first_body && edge.kind == CfgEdgeKind::BranchTrue)
    );
    assert!(
        cfg.successors(last_body)
            .any(|edge| edge.to == condition && edge.kind == CfgEdgeKind::LoopBack)
    );
    assert!(
        cfg.successors(condition)
            .any(|edge| edge.kind == CfgEdgeKind::BranchFalse)
    );
}

#[test]
fn loop_cfg_has_back_edge_and_rumpe_exit() {
    let source = r"
incipit {
    varia lista<numerus> xs ← [1, 2, 3]
    fac {
        varia in lista<numerus> view ← xs
        nota view.longitudo()
        xs.appende(4)
        rumpe
    } dum verum
}
";
    let result = analyze_source(source);
    let hir = result.hir.as_ref().expect("HIR");
    let mut shapes = ControlShapes::default();
    shapes.visit_program(hir);
    let (loop_id, body_ids) = shapes.infinite_loop.expect("loop shape");
    let facts = borrow_region::analyze(
        hir,
        &result.types,
        result.access_facts.as_ref().expect("access facts"),
    )
    .expect("loop CFG");
    let cfg = facts.cfg();
    let header = cfg.node_for_hir_id(loop_id).expect("loop header");
    let first_body = cfg.node_for_hir_id(body_ids[0]).expect("body entry");
    assert!(
        cfg.successors(header)
            .any(|edge| edge.to == first_body && edge.kind == CfgEdgeKind::Next)
    );
    assert!(
        cfg.edges()
            .iter()
            .any(|edge| edge.kind == CfgEdgeKind::Break)
    );
    assert!(
        cfg.edges()
            .iter()
            .any(|edge| { edge.kind == CfgEdgeKind::LoopBack && edge.to == header })
    );
    assert!(
        cfg.nodes()
            .iter()
            .any(|node| node.kind == CfgNodeKind::Exit)
    );
}

#[test]
fn region_solver_converges_on_both_cyclic_shapes() {
    let dum = analyzed_region(
        r"
incipit {
    varia lista<numerus> xs ← [1, 2, 3]
    dum xs.longitudo() < 5 {
        varia in lista<numerus> view ← xs
        nota view.longitudo()
        xs.appende(4)
    }
}
",
    );
    let loop_facts = analyzed_region(
        r"
incipit {
    varia lista<numerus> xs ← [1, 2, 3]
    fac {
        varia in lista<numerus> view ← xs
        nota view.longitudo()
        xs.appende(4)
        rumpe
    } dum verum
}
",
    );

    assert!(
        dum.cfg()
            .edges()
            .iter()
            .any(|edge| edge.kind == CfgEdgeKind::LoopBack)
    );
    assert!(
        loop_facts
            .cfg()
            .edges()
            .iter()
            .any(|edge| edge.kind == CfgEdgeKind::LoopBack)
    );
    assert!(dum.fact_reads() > 0);
    assert!(loop_facts.fact_reads() > 0);
}

// ── SCR-04 durable provenance reduction ─────────────────────────────────────

fn reduced(
    source: &str,
) -> (
    borrow_region::RegionFacts,
    crate::capability::FunctionProvenanceTable,
    radix_hir::HirModule,
) {
    let result = analyze_source(source);
    let hir = result
        .hir
        .as_ref()
        .unwrap_or_else(|| panic!("analysis must succeed: {:?}", result.diagnostics))
        .clone();
    let access_facts = result.access_facts.as_ref().expect("access facts");
    let regions = borrow_region::analyze(&hir, &result.types, access_facts).expect("regions");
    let table =
        borrow_region::reduce(&hir, &result.types, Some(&regions)).expect("coherent reduction");
    (regions, table, hir)
}

fn first_function_def(hir: &radix_hir::HirModule) -> radix_hir::DefId {
    hir.items
        .iter()
        .find(|item| matches!(item.kind, radix_hir::HirItemKind::Function(_)))
        .expect("function item")
        .def_id
}

#[test]
fn plain_function_reduces_to_settled_no_borrow_summary() {
    let (_regions, table, hir) = reduced(
        r"
functio plain(numerus n) → numerus {
    redde n
}
incipit { }
",
    );
    let row = table.get(first_function_def(&hir)).expect("row");
    assert_eq!(
        row.returned_borrow,
        radix_types::ReturnedBorrowProvenance::None
    );
    assert!(row.required_regions.is_empty());
    assert!(row.last_uses.is_empty());
}

#[test]
fn borrow_local_last_use_summary_matches_live_regions() {
    let source = r"
functio utitur() → numerus {
    varia lista<numerus> xs ← [1, 2, 3]
    varia in lista<numerus> view ← xs
    redde view.longitudo()
}
incipit { }
";
    let (regions, table, hir) = reduced(source);
    let row = table.get(first_function_def(&hir)).expect("row");

    // Sanity (SCR-04 card): the persisted summary must agree with the live
    // RegionFacts it was reduced from — one last-use fact per body region,
    // keyed by the same borrower, ending at the same last-use site.
    let live: Vec<(radix_hir::DefId, radix_hir::HirId)> = regions
        .regions()
        .iter()
        .filter_map(|region| {
            region
                .last_use()
                .and_then(|location| location.hir_id)
                .map(|site| (region.borrower, site))
        })
        .collect();
    let persisted: Vec<(radix_hir::DefId, radix_hir::HirId)> = row
        .last_uses
        .iter()
        .map(|fact| (fact.borrower, fact.site))
        .collect();
    assert_eq!(persisted, live);
    assert!(
        !persisted.is_empty(),
        "body borrow must carry a last-use fact"
    );
}

#[test]
fn borrow_returning_parameter_carries_parameter_provenance() {
    let (_regions, table, hir) = reduced(
        r"
functio eadem(de lista<numerus> xs) → de lista<numerus> {
    redde xs
}
incipit { }
",
    );
    let row = table.get(first_function_def(&hir)).expect("row");
    let param = match &hir.items[0].kind {
        radix_hir::HirItemKind::Function(function) => function.params[0].def_id,
        _ => panic!("function item"),
    };
    assert_eq!(
        row.returned_borrow,
        radix_types::ReturnedBorrowProvenance::Parameter(param)
    );
    assert!(row.required_regions.is_empty());
}

#[test]
fn absent_regions_fail_closed_for_borrow_returning_function() {
    let result = analyze_source(
        r"
functio mutator(lista<numerus> xs) → in lista<numerus> {
    varia in lista<numerus> view ← xs
    redde view
}
incipit { }
",
    );
    let hir = result.hir.as_ref().expect("typed HIR");
    let error = borrow_region::reduce(hir, &result.types, None).expect_err("fail closed");
    assert!(matches!(
        error,
        borrow_region::ProvenanceError::Unavailable { .. }
    ));
}

#[test]
fn foreign_region_facts_are_incoherent_for_borrow_returning_function() {
    let borrowing = analyze_source(
        r"
functio mutator(lista<numerus> xs) → in lista<numerus> {
    varia in lista<numerus> view ← xs
    redde view
}
incipit { }
",
    );
    let other = analyze_source(
        r"
functio alia(numerus n) → numerus {
    redde n
}
incipit { }
",
    );
    let other_hir = other.hir.as_ref().expect("typed HIR");
    let other_facts = other.access_facts.as_ref().expect("access facts");
    let foreign = borrow_region::analyze(other_hir, &other.types, other_facts).expect("regions");

    let borrowing_hir = borrowing.hir.as_ref().expect("typed HIR");
    let error = borrow_region::reduce(borrowing_hir, &borrowing.types, Some(&foreign))
        .expect_err("cross-run facts must not satisfy a different snapshot");
    assert!(matches!(
        error,
        borrow_region::ProvenanceError::Incoherent { .. }
    ));
}

// ── Ruling R3: `perge` in `fac … dum` lands on the condition test ──────────

/// Collect the `perge` statement ids and the synthesized `dum` test condition
/// of the (single) `fac … dum` loop in the program.
#[derive(Default)]
struct FacDumShape {
    perges: Vec<HirId>,
    test_condition: Option<HirId>,
}

impl HirVisitor for FacDumShape {
    fn visit_expr(&mut self, expression: &HirExpression) {
        if let HirExpressionKind::Loop(body) = &expression.kind {
            let (_, test, _) = body.fac_dum_parts().expect("fac … dum loop parts");
            let radix_hir::HirStatementKind::Expr(test) = &test.kind else {
                unreachable!("fac_dum_parts returns an expression test");
            };
            let HirExpressionKind::Si { cond, .. } = &test.kind else {
                unreachable!("fac_dum_parts returns a si test");
            };
            self.test_condition = Some(cond.id);
        }
        walk_expr(self, expression);
    }

    fn visit_stmt(&mut self, statement: &radix_hir::HirStatement) {
        if matches!(statement.kind, radix_hir::HirStatementKind::Perge) {
            self.perges.push(statement.id);
        }
        radix_hir::visit::walk_stmt(self, statement);
    }
}

fn assert_perge_edges_target_dum_test(source: &str) {
    let result = analyze_source(source);
    let hir = result.hir.as_ref().expect("HIR");
    let mut shape = FacDumShape::default();
    shape.visit_program(hir);
    assert!(!shape.perges.is_empty(), "fixture must contain perge");
    let facts = borrow_region::analyze(
        hir,
        &result.types,
        result.access_facts.as_ref().expect("access facts"),
    )
    .expect("fac … dum CFG");
    let cfg = facts.cfg();
    let test = cfg
        .node_for_hir_id(shape.test_condition.expect("dum test"))
        .expect("dum test condition node");
    for perge in shape.perges {
        let perge = cfg.node_for_hir_id(perge).expect("perge node");
        let targets: Vec<_> = cfg
            .successors(perge)
            .filter(|edge| edge.kind == CfgEdgeKind::Continue)
            .map(|edge| edge.to)
            .collect();
        assert_eq!(
            targets,
            vec![test],
            "perge must continue to the dum condition test (ruling R3)"
        );
    }
}

#[test]
fn fac_dum_perge_edge_targets_condition_test() {
    assert_perge_edges_target_dum_test(
        r"
incipit {
    varia numerus n ← 0
    fac {
        n ← n + 1
        si n < 3 {
            perge
        }
        nota n
    } dum n < 5
}
",
    );
}

#[test]
fn fac_dum_perge_through_bare_fac_block_targets_condition_test() {
    assert_perge_edges_target_dum_test(
        r"
incipit {
    varia numerus n ← 0
    fac {
        n ← n + 1
        fac {
            si n < 3 {
                perge
            }
        }
        nota n
    } dum n < 5
}
",
    );
}

#[test]
fn fac_dum_perge_keeps_borrow_live_into_condition() {
    // The only path from the write to the condition is `perge`. Under ruling
    // R3 the condition runs next and reads `view`, so the loan is live at the
    // write and SEM052 must fire.
    let result = analyze_source(
        r"
incipit {
    varia lista<numerus> xs ← [1, 2, 3]
    varia numerus n ← 0
    varia in lista<numerus> view ← xs
    fac {
        n ← n + 1
        si n < 3 {
            xs.appende(4)
            perge
        }
        rumpe
    } dum view.longitudo() < 10
}
",
    );
    assert!(
        result.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == SemanticErrorKind::MutableBorrowConflict
                && issue(diagnostic) == Some("write_while_borrowed")
        }),
        "borrow read by the dum condition must be live at a write before perge; diagnostics: {:?}",
        result.diagnostics
    );
}
