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
use crate::test_support::{analyze_ok, analyze_source, issue};
use crate::SemanticErrorKind;
use radix_hir::visit::{walk_expr, HirVisitor};
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

    assert!(cfg
        .successors(condition)
        .any(|edge| edge.to == first_body && edge.kind == CfgEdgeKind::BranchTrue));
    assert!(cfg
        .successors(last_body)
        .any(|edge| edge.to == condition && edge.kind == CfgEdgeKind::LoopBack));
    assert!(cfg
        .successors(condition)
        .any(|edge| edge.kind == CfgEdgeKind::BranchFalse));
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
    assert!(cfg
        .successors(header)
        .any(|edge| edge.to == first_body && edge.kind == CfgEdgeKind::Next));
    assert!(cfg
        .edges()
        .iter()
        .any(|edge| edge.kind == CfgEdgeKind::Break));
    assert!(cfg
        .edges()
        .iter()
        .any(|edge| { edge.kind == CfgEdgeKind::LoopBack && edge.to == header }));
    assert!(cfg
        .nodes()
        .iter()
        .any(|node| node.kind == CfgNodeKind::Exit));
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

    assert!(dum
        .cfg()
        .edges()
        .iter()
        .any(|edge| edge.kind == CfgEdgeKind::LoopBack));
    assert!(loop_facts
        .cfg()
        .edges()
        .iter()
        .any(|edge| edge.kind == CfgEdgeKind::LoopBack));
    assert!(dum.fact_reads() > 0);
    assert!(loop_facts.fact_reads() > 0);
}
