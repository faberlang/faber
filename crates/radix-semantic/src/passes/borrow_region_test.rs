//! BRI-1 red drivers for the HIR-hosted borrow-region contract.
//!
//! The delivery artifact settles the host as a semantic CFG derived from
//! typed HIR. Locations retain HIR identity (`HirId`/`DefId`/`Span`), and a
//! borrow region ends at the borrow's last use rather than at HIR block exit.
//! The parity contract is `AcceptedApproximation ⊆ AcceptedRegions`: a
//! region-based checker may accept these cases after last-use inference, but
//! it must not reject anything accepted by the current approximation.
//!
//! These fixtures intentionally pin the current scope checker's red result.
//! Once the region authority lands, both programs are expected to analyze
//! successfully because the marked mutable borrow is last used before the
//! source write. Do not weaken the red expectation in this unit.

use super::borrow_region;
use super::borrow_region_cfg::{CfgEdgeKind, CfgNodeKind};
use crate::test_support::{analyze_source, issue};
use crate::SemanticErrorKind;
use radix_hir::visit::{walk_expr, HirVisitor};
use radix_hir::{HirBlock, HirExpression, HirExpressionKind, HirId};

fn assert_current_scope_red(source: &str, label: &str) {
    let result = analyze_source(source);
    assert!(
        result.is_err(),
        "{label} must remain a current-scope red driver; diagnostics: {:?}",
        result.diagnostics
    );
    assert!(
        result.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == SemanticErrorKind::MutableBorrowConflict
                && issue(diagnostic) == Some("write_while_borrowed")
        }),
        "{label} must expose MutableBorrowConflict/write_while_borrowed; diagnostics: {:?}",
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
    let source = r#"
incipit {
    varia lista<numerus> xs ← [1, 2, 3]
    dum xs.longitudo() < 5 {
        varia in lista<numerus> view ← xs
        nota view.longitudo()
        xs.appende(4)
    }
}
"#;

    // PRE-BRI-3: block-scope tracking keeps `view` live until the body exits,
    // so the current authority reports write_while_borrowed. POST-BRI-3, the
    // last-use region should accept this fixture.
    assert_current_scope_red(source, "dum back-edge release-before-write");
}

#[test]
fn loop_rumpe_releases_mutable_borrow_before_source_write() {
    // The explicit `rumpe` supplies the loop exit while the fall-through body
    // edge returns to the next iteration. The source write follows the borrow
    // last use in the body.
    let source = r#"
incipit {
    varia lista<numerus> xs ← [1, 2, 3]
    fac {
        varia in lista<numerus> view ← xs
        nota view.longitudo()
        xs.appende(4)
        rumpe
    } dum verum
}
"#;

    // PRE-BRI-3: the current block-scope authority reports the same red.
    // POST-BRI-3: last-use region inference should accept the fixture while
    // preserving the explicit `rumpe` exit edge.
    assert_current_scope_red(source, "loop rumpe release-before-write");
}

#[test]
fn dum_cfg_has_condition_body_back_edge_and_exit() {
    let source = r#"
incipit {
    varia lista<numerus> xs ← [1, 2, 3]
    dum xs.longitudo() < 5 {
        varia in lista<numerus> view ← xs
        nota view.longitudo()
        xs.appende(4)
    }
}
"#;
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
    let source = r#"
incipit {
    varia lista<numerus> xs ← [1, 2, 3]
    fac {
        varia in lista<numerus> view ← xs
        nota view.longitudo()
        xs.appende(4)
        rumpe
    } dum verum
}
"#;
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
        r#"
incipit {
    varia lista<numerus> xs ← [1, 2, 3]
    dum xs.longitudo() < 5 {
        varia in lista<numerus> view ← xs
        nota view.longitudo()
        xs.appende(4)
    }
}
"#,
    );
    let loop_facts = analyzed_region(
        r#"
incipit {
    varia lista<numerus> xs ← [1, 2, 3]
    fac {
        varia in lista<numerus> view ← xs
        nota view.longitudo()
        xs.appende(4)
        rumpe
    } dum verum
}
"#,
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
