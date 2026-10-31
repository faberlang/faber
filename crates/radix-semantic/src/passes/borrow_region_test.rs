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
use crate::test_support::{analyze_ok, analyze_source, analyze_source_with_config, issue};
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

/// The tensor-view fixtures are written in the English surface.
fn analyze_en(source: &str) -> crate::test_support::AnalyzedFixture {
    let mut config = crate::PassConfig::for_target(radix_target::Target::HirRust);
    config.locale_pack = Some(
        crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
            .expect("load English locale pack"),
    );
    analyze_source_with_config(source, config)
}

const TENSOR_VIEW_LAST_USE_SOURCE: &str = r"
main {
    let list<f32> data ← [1.0, 2.0, 3.0, 4.0]
    mut tensor<f32, [2, 2]> base ← data ↦ tensor<f32, [2, 2]>
    let tensor<f32, [2, 2]> view ← base.transpose()
    let int dimensions ← view.length()
    base.set([0, 0], 9.0)
    print dimensions
}
";

#[test]
fn tensor_view_base_write_is_allowed_after_last_view_use() {
    let result = analyze_en(TENSOR_VIEW_LAST_USE_SOURCE);
    assert!(
        result.is_ok(),
        "base write after the view's final dimensions query should be accepted: {:?}",
        result.diagnostics
    );
}

#[test]
fn tensor_view_base_write_is_rejected_before_later_view_use() {
    let source = TENSOR_VIEW_LAST_USE_SOURCE.replace(
        "    let int dimensions ← view.length()\n    base.set([0, 0], 9.0)\n    print dimensions",
        "    base.set([0, 0], 9.0)\n    let int dimensions ← view.length()\n    print dimensions",
    );
    let result = analyze_en(&source);
    assert!(
        result.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == SemanticErrorKind::MutableBorrowConflict
                && issue(diagnostic) == Some("write_while_borrowed")
        }),
        "base write while a later view use remains must report write_while_borrowed: {:?}",
        result.diagnostics
    );
}

#[test]
fn tensor_view_ternary_cannot_escape_through_mutable_binding() {
    let source = r"
main {
    mut tf32[2, 2] base ← [[1.0, 2.0], [3.0, 4.0]] ↦ tf32[2, 2]
    mut tf32[2, 2] view ← true ✓ base.transpose() ✗ base.transpose()
}
";
    let result = analyze_en(source);
    assert!(
        result.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == SemanticErrorKind::ModeMismatch
                && issue(diagnostic) == Some("tensor_view_escape")
        }),
        "a tensor-view ternary must retain provenance at a mutable binding: {:?}",
        result.diagnostics
    );
}

#[test]
fn tensor_view_ternary_can_be_consumed_by_materialize() {
    let source = r"
main {
    mut tf32[2, 2] base ← [[1.0, 2.0], [3.0, 4.0]] ↦ tf32[2, 2]
    let tf32[2, 2] owned ← (true ✓ base.transpose() ✗ base.transpose()).materialize()
    base.set([0, 0], 9.0)
    print owned.length()
}
";
    let result = analyze_en(source);
    assert!(
        result.is_ok(),
        "materialize must end ternary view provenance before a later base write: {:?}",
        result.diagnostics
    );
}

#[test]
fn ternary_of_equal_shape_view_calls_joins_by_shape() {
    // Each branch carries its own derived shape index; the join unifies them
    // structurally, so the result has the transposed shape [3, 2].
    let source = r"
main {
    mut tf32[2, 3] base ← [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]] ↦ tf32[2, 3]
    let tf32[3, 2] owned ← (true ✓ base.transpose() ✗ base.transpose()).materialize()
    print owned.length()
}
";
    let result = analyze_en(source);
    assert!(result.is_ok(), "{:?}", result.diagnostics);
}

#[test]
fn ternary_of_unequal_shape_views_is_rejected_at_the_join() {
    // [2, 3] against its transpose [3, 2] share no shape: the join reports
    // the shape mismatch instead of silently picking one side.
    let source = r"
main {
    mut tf32[2, 3] base ← [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]] ↦ tf32[2, 3]
    let tf32[3, 2] owned ← (true ✓ base ✗ base.transpose()).materialize()
    print owned.length()
}
";
    let result = analyze_en(source);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.is_error() && d.issue() == Some("incompatible_tensor_index")),
        "{:?}",
        result.diagnostics
    );
}

#[test]
fn expanded_transpose_chain_cannot_escape_through_mutable_binding() {
    let source = r"
main {
    mut tf32[1, 2] base ← [[1.0, 2.0]] ↦ tf32[1, 2]
    mut tf32[2, 1] escaped ← base.expanded([1, 2]).transpose()
}
";
    let result = analyze_en(source);
    assert!(
        result.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == SemanticErrorKind::ModeMismatch
                && issue(diagnostic) == Some("tensor_view_escape")
        }),
        "expanded and chained transpose provenance must reach mutable sinks: {:?}",
        result.diagnostics
    );
}

const TENSOR_EDGE_POLICY_DECLARATIONS: &str = r"
interface EdgePolicy<C> {
    fn remap(C coord) → C
}
interface EdgeTransform<C, T> {
    fn transform(C coord, T value) → T
}
class ZeroGhost implements EdgePolicy<vector<int, size 1>> ∩ EdgeTransform<vector<int, size 1>, f32> {
    fn remap(vector<int, size 1> coord) → vector<int, size 1> {
        return [0] ↦ vector<int, size 1>
    }
    fn transform(vector<int, size 1> coord, f32 value) → f32 {
        return -value
    }
}
";

#[test]
fn shifted_limes_view_keeps_provenance_through_wrapper_and_policy_type_args() {
    let main = r"
main {
    mut tf32[3] base ← [1.0, 2.0, 3.0] ↦ tf32[3]
    mut tf32[3] escaped ← true ✓ base.shift([1]).edge<ZeroGhost>() ✗ base.shift([1]).edge<ZeroGhost>()
}
";
    let source = format!("{TENSOR_EDGE_POLICY_DECLARATIONS}{main}");
    let result = analyze_en(&source);
    assert!(
        result.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == SemanticErrorKind::ModeMismatch
                && issue(diagnostic) == Some("tensor_view_escape")
        }),
        "shift→limes provenance and a stateless policy type argument must survive the wrapper: {:?}",
        result.diagnostics
    );
}

#[test]
fn custom_limes_view_conflicts_with_base_write_until_last_use() {
    let main = r"
main {
    mut tf32[3] base ← [1.0, 2.0, 3.0] ↦ tf32[3]
    let tf32[3] view ← base.shift([1]).edge<ZeroGhost>()
    base.set([0], 9.0)
    print view.length()
}
";
    let source = format!("{TENSOR_EDGE_POLICY_DECLARATIONS}{main}");
    let result = analyze_en(&source);
    assert!(
        result.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == SemanticErrorKind::MutableBorrowConflict
                && issue(diagnostic) == Some("write_while_borrowed")
        }),
        "the limes view must borrow its tensor receiver, not capture its policy type argument: {:?}",
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
    dum xs.longitudo() ≺ 5 {
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
    dum xs.longitudo() ≺ 5 {
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
    dum xs.longitudo() ≺ 5 {
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
    dum xs.longitudo() ≺ 5 {
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
    dum xs.longitudo() ≺ 5 {
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
    assert_eq!(
        row.required_regions,
        [] as [radix_types::RegionRelationship; 0]
    );
    assert_eq!(row.last_uses, [] as [crate::capability::LastUseFact; 0]);
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
    assert_eq!(
        row.required_regions,
        [] as [radix_types::RegionRelationship; 0]
    );
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
        si n ≺ 3 {
            perge
        }
        nota n
    } dum n ≺ 5
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
            si n ≺ 3 {
                perge
            }
        }
        nota n
    } dum n ≺ 5
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
        si n ≺ 3 {
            xs.appende(4)
            perge
        }
        rumpe
    } dum view.longitudo() ≺ 10
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

fn has_tensor_view_escape(source: &str) -> (bool, Vec<crate::SemanticError>) {
    let result = analyze_source(source);
    let escaped = result.diagnostics.iter().any(|diagnostic| {
        diagnostic.kind == SemanticErrorKind::ModeMismatch
            && issue(diagnostic) == Some("tensor_view_escape")
    });
    (escaped, result.diagnostics.clone())
}

#[test]
fn text_slice_is_not_a_tensor_view() {
    for (label, source) in [
        (
            "returned",
            "functio f(textus s) → textus { redde s.sectio(0, 2) }\nincipit { nota f(\"abcd\") }",
        ),
        (
            "literal receiver",
            "functio f(numerus d) → textus { redde \"0123456789abcdef\".sectio(d, d + 1) }\nincipit { nota f(3) }",
        ),
        (
            "bound then returned",
            "functio f(textus s) → textus {\n    sit textus head ← s.sectio(0, 2)\n    redde head\n}\nincipit { nota f(\"abcd\") }",
        ),
    ] {
        let (escaped, diagnostics) = has_tensor_view_escape(source);
        assert!(
            !escaped,
            "text slice ({label}) is not a view: {diagnostics:?}"
        );
    }
}

#[test]
fn list_slice_is_not_a_tensor_view() {
    for (label, source) in [
        (
            "returned",
            "functio f(lista<numerus> xs) → lista<numerus> { redde xs.sectio(0, 1) }\nincipit { nota f([1, 2]) }",
        ),
        (
            "bound then returned",
            "functio f(lista<numerus> xs) → lista<numerus> {\n    sit lista<numerus> head ← xs.sectio(0, 1)\n    redde head\n}\nincipit { nota f([1, 2]) }",
        ),
    ] {
        let (escaped, diagnostics) = has_tensor_view_escape(source);
        assert!(
            !escaped,
            "list slice ({label}) is not a view: {diagnostics:?}"
        );
    }
}

#[test]
fn matrix_transpose_is_not_a_tensor_view() {
    let source = "functio f(mf32[2, 2] m) → mf32[2, 2] { redde m.transpone() }\nincipit { }";
    let (escaped, diagnostics) = has_tensor_view_escape(source);
    assert!(!escaped, "matrix transpone is a copy: {diagnostics:?}");
}

#[test]
fn tensor_receiver_view_methods_still_escape() {
    for (label, source) in [
        (
            "returned",
            "functio f(tf32[2, 2] x) → tf32[2, 2] { redde x.transpone() }\nincipit { }",
        ),
        (
            "bound then returned",
            "functio f(tf32[2, 2] x) → tf32[2, 2] {\n    sit tf32[2, 2] v ← x.transpone()\n    redde v\n}\nincipit { }",
        ),
    ] {
        let (escaped, diagnostics) = has_tensor_view_escape(source);
        assert!(
            escaped,
            "tensor view ({label}) must still escape-decline: {diagnostics:?}"
        );
    }
}

// ── A-08: `redde` nested in control flow reaches the provenance reduction ───

#[test]
fn borrow_return_only_inside_si_carries_parameter_provenance() {
    let (_regions, table, hir) = reduced(
        r"
functio eadem(bivalens b, de lista<numerus> xs) → de lista<numerus> {
    si b {
        redde xs
    } secus {
        redde xs
    }
}
incipit { }
",
    );
    let row = table.get(first_function_def(&hir)).expect("row");
    let param = match &hir.items[0].kind {
        radix_hir::HirItemKind::Function(function) => function.params[1].def_id,
        _ => panic!("function item"),
    };
    assert_eq!(
        row.returned_borrow,
        radix_types::ReturnedBorrowProvenance::Parameter(param)
    );
}

#[test]
fn nested_borrow_return_with_differing_provenance_is_incoherent() {
    let result = analyze_source(
        r"
functio mixta(bivalens b, lista<numerus> xs, lista<numerus> ys) → in lista<numerus> {
    varia in lista<numerus> a ← xs
    varia in lista<numerus> c ← ys
    si b {
        redde a
    }
    redde c
}
incipit { }
",
    );
    let hir = result
        .hir
        .as_ref()
        .unwrap_or_else(|| panic!("analysis must succeed: {:?}", result.diagnostics));
    let access_facts = result.access_facts.as_ref().expect("access facts");
    let regions = borrow_region::analyze(hir, &result.types, access_facts).expect("regions");
    let error = borrow_region::reduce(hir, &result.types, Some(&regions))
        .expect_err("the nested return disagrees with the top-level return");
    assert!(matches!(
        error,
        borrow_region::ProvenanceError::Incoherent { .. }
    ));
}

#[test]
fn returned_borrow_with_two_regions_of_different_provenance_is_incoherent() {
    let result = analyze_source(
        r"
functio duo(tf32[2, 2] a, tf32[2, 2] b, bivalens cond) → de tf32[2, 2] {
    sit de tf32[2, 2] view ← cond ✓ a.transpone() ✗ b.transpone()
    redde view
}
incipit { }
",
    );
    let hir = result
        .hir
        .as_ref()
        .unwrap_or_else(|| panic!("analysis must succeed: {:?}", result.diagnostics));
    let access_facts = result.access_facts.as_ref().expect("access facts");
    let regions =
        borrow_region::analyze_with_interner(hir, &result.types, access_facts, &result.interner)
            .expect("regions");

    // The conditional binds `view` from both sources, so the returned
    // borrower owns two regions. The reduction must treat that the way it
    // treats a second `redde`: every region contributes a relationship, and
    // provenance that cannot agree is incoherent.
    let mut borrowers: Vec<radix_hir::DefId> = regions
        .regions()
        .iter()
        .map(|region| region.borrower)
        .collect();
    borrowers.sort_by_key(|def_id| def_id.0);
    borrowers.dedup();
    assert_eq!(
        regions.regions().len(),
        2,
        "fixture must bind the returned view from two sources"
    );
    assert_eq!(borrowers.len(), 1, "both regions must share the borrower");

    let error = borrow_region::reduce(hir, &result.types, Some(&regions))
        .expect_err("two provenance sources for one returned borrower must be incoherent");
    assert!(matches!(
        error,
        borrow_region::ProvenanceError::Incoherent { .. }
    ));
}

/// A module whose only executable content is the hand-built `entry` block.
fn module_with_entry(entry: HirBlock) -> radix_hir::HirModule {
    radix_hir::HirModule {
        items: Vec::new(),
        entry: Some(entry),
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    }
}

fn hand_statement(id: u32, kind: radix_hir::HirStatementKind) -> radix_hir::HirStatement {
    radix_hir::HirStatement {
        id: HirId(id),
        kind,
        span: radix_lexer::Span::default(),
    }
}

fn hand_expression(id: u32, kind: HirExpressionKind) -> HirExpression {
    HirExpression {
        id: HirId(id),
        kind,
        ty: None,
        span: radix_lexer::Span::default(),
    }
}

fn cfg_reaches(
    cfg: &super::borrow_region_cfg::HirCfg,
    from: super::borrow_region_cfg::CfgNodeId,
    to: super::borrow_region_cfg::CfgNodeId,
) -> bool {
    let mut seen = vec![from];
    let mut work = vec![from];
    while let Some(node) = work.pop() {
        if node == to {
            return true;
        }
        for edge in cfg.successors(node) {
            if !seen.contains(&edge.to) {
                seen.push(edge.to);
                work.push(edge.to);
            }
        }
    }
    false
}

// A-183: an empty block sequenced after a statement is the identity; it must
// not replace the fall-through the statement already established.
#[test]
fn empty_tail_block_keeps_the_arm_fall_through() {
    let then_block = HirBlock {
        statements: vec![hand_statement(10, radix_hir::HirStatementKind::Tacet)],
        expr: Some(Box::new(hand_expression(
            11,
            HirExpressionKind::Block(HirBlock::default()),
        ))),
        ..HirBlock::default()
    };
    let si = hand_expression(
        30,
        HirExpressionKind::Si {
            cond: Box::new(hand_expression(
                20,
                HirExpressionKind::Literal(radix_hir::HirLiteral::Int(1)),
            )),
            then_block,
            then_catch: None,
            else_block: None,
        },
    );
    let module = module_with_entry(HirBlock {
        statements: vec![hand_statement(31, radix_hir::HirStatementKind::Expr(si))],
        ..HirBlock::default()
    });

    let cfg = super::borrow_region_cfg::build_cfg(&module).expect("cfg");
    let arm = cfg.node_for_hir_id(HirId(10)).expect("arm statement");
    let join = cfg.node_for_hir_id(HirId(30)).expect("si join");
    assert!(
        cfg.successors(arm)
            .any(|edge| edge.to == join && edge.kind == CfgEdgeKind::Next),
        "the arm's fall-through must still reach the join: {:?}",
        cfg.edges()
    );
}

#[test]
fn empty_branches_remain_reachable_from_their_conditions() {
    let si = hand_expression(
        30,
        HirExpressionKind::Si {
            cond: Box::new(hand_expression(
                20,
                HirExpressionKind::Literal(radix_hir::HirLiteral::Int(1)),
            )),
            then_block: HirBlock::default(),
            then_catch: None,
            else_block: None,
        },
    );
    let custodi = hand_statement(
        41,
        radix_hir::HirStatementKind::Custodi(radix_hir::HirCustodi {
            clauses: vec![radix_hir::HirCustodiClause {
                cond: hand_expression(
                    40,
                    HirExpressionKind::Literal(radix_hir::HirLiteral::Int(1)),
                ),
                body: HirBlock::default(),
                span: radix_lexer::Span::default(),
            }],
        }),
    );
    let discerne = hand_expression(
        60,
        HirExpressionKind::Discerne {
            scrutinees: Vec::new(),
            arms: vec![radix_hir::HirCasuArm {
                patterns: Vec::new(),
                body: hand_expression(61, HirExpressionKind::Block(HirBlock::default())),
                span: radix_lexer::Span::default(),
            }],
            exhaustive: false,
        },
    );
    let module = module_with_entry(HirBlock {
        statements: vec![
            hand_statement(31, radix_hir::HirStatementKind::Expr(si)),
            custodi,
            hand_statement(62, radix_hir::HirStatementKind::Expr(discerne)),
        ],
        ..HirBlock::default()
    });

    let cfg = super::borrow_region_cfg::build_cfg(&module).expect("cfg");
    let si_condition = cfg.node_for_hir_id(HirId(20)).expect("si condition");
    let si_join = cfg.node_for_hir_id(HirId(30)).expect("si join");
    assert!(
        cfg.successors(si_condition)
            .any(|edge| { edge.to == si_join && edge.kind == CfgEdgeKind::BranchTrue })
    );

    let guard_condition = cfg.node_for_hir_id(HirId(40)).expect("guard condition");
    let guard_join = cfg.node_for_hir_id(HirId(41)).expect("guard join");
    assert!(
        cfg.successors(guard_condition)
            .any(|edge| { edge.to == guard_join && edge.kind == CfgEdgeKind::BranchTrue })
    );

    let dispatch = cfg
        .nodes()
        .iter()
        .find(|node| node.kind == CfgNodeKind::Condition && node.location.hir_id == Some(HirId(60)))
        .expect("discerne dispatch")
        .id;
    let dispatch_join = cfg
        .nodes()
        .iter()
        .find(|node| node.kind == CfgNodeKind::Join && node.location.hir_id == Some(HirId(60)))
        .expect("discerne join")
        .id;
    assert!(
        cfg.successors(dispatch)
            .any(|edge| { edge.to == dispatch_join && edge.kind == CfgEdgeKind::BranchTrue })
    );
}

#[test]
fn error_edges_cover_only_the_protected_body() {
    let block_with_statement = |id| HirBlock {
        statements: vec![hand_statement(id, radix_hir::HirStatementKind::Tacet)],
        ..HirBlock::default()
    };
    let catch = |binding, statement| radix_hir::HirCape {
        binding_def_id: radix_hir::DefId(binding),
        binding_name: radix_lexer::Symbol(binding),
        binding_ty: None,
        body: block_with_statement(statement),
        span: radix_lexer::Span::default(),
    };

    let handled = hand_expression(
        100,
        HirExpressionKind::Handled {
            body: block_with_statement(101),
            catch: Box::new(catch(1, 102)),
        },
    );
    let si = hand_expression(
        200,
        HirExpressionKind::Si {
            cond: Box::new(hand_expression(
                201,
                HirExpressionKind::Literal(radix_hir::HirLiteral::Int(1)),
            )),
            then_block: block_with_statement(202),
            then_catch: Some(Box::new(catch(2, 203))),
            else_block: None,
        },
    );
    let call_default = hand_expression(
        300,
        HirExpressionKind::CallDefault {
            body: Box::new(block_with_statement(301)),
            default: Box::new(hand_expression(
                302,
                HirExpressionKind::Literal(radix_hir::HirLiteral::Int(0)),
            )),
            err_ty: None,
        },
    );
    let module = module_with_entry(HirBlock {
        statements: vec![
            hand_statement(110, radix_hir::HirStatementKind::Expr(handled)),
            hand_statement(210, radix_hir::HirStatementKind::Expr(si)),
            hand_statement(310, radix_hir::HirStatementKind::Expr(call_default)),
        ],
        ..HirBlock::default()
    });

    let cfg = super::borrow_region_cfg::build_cfg(&module).expect("cfg");
    let error_edges: Vec<_> = cfg
        .edges()
        .iter()
        .filter(|edge| edge.kind == CfgEdgeKind::Error)
        .map(|edge| (edge.from, edge.to))
        .collect();
    assert_eq!(
        error_edges,
        vec![
            (
                cfg.node_for_hir_id(HirId(101)).expect("handled body"),
                cfg.node_for_hir_id(HirId(102)).expect("handled catch"),
            ),
            (
                cfg.node_for_hir_id(HirId(202)).expect("si body"),
                cfg.node_for_hir_id(HirId(203)).expect("si catch"),
            ),
            (
                cfg.node_for_hir_id(HirId(301)).expect("default body"),
                cfg.node_for_hir_id(HirId(302)).expect("default handler"),
            ),
        ],
        "handler nodes and joins must not gain edges back into their own handler"
    );
}

// A-183: an expression statement over an empty block keeps its statement
// node as the fragment entry, so the previous statement flows into it.
#[test]
fn empty_block_statement_stays_on_the_fall_through_path() {
    let module = module_with_entry(HirBlock {
        statements: vec![
            hand_statement(1, radix_hir::HirStatementKind::Tacet),
            hand_statement(
                3,
                radix_hir::HirStatementKind::Expr(hand_expression(
                    2,
                    HirExpressionKind::Block(HirBlock::default()),
                )),
            ),
            hand_statement(4, radix_hir::HirStatementKind::Tacet),
        ],
        ..HirBlock::default()
    });

    let cfg = super::borrow_region_cfg::build_cfg(&module).expect("cfg");
    let before = cfg.node_for_hir_id(HirId(1)).expect("first statement");
    let after = cfg.node_for_hir_id(HirId(4)).expect("last statement");
    assert!(
        cfg_reaches(&cfg, before, after),
        "the empty block statement must not cut the fall-through: {:?}",
        cfg.edges()
    );
}

// ── First-use-in-component anchoring: capture-site regions ──────────────────

fn has_write_conflict(result: &crate::test_support::AnalyzedFixture) -> bool {
    result.diagnostics.iter().any(|diagnostic| {
        diagnostic.kind == SemanticErrorKind::MutableBorrowConflict
            && issue(diagnostic) == Some("write_while_borrowed")
    })
}

/// A borrower whose first recorded use sits inside a separate component (a
/// closure or lane body) must be anchored at the component's creation site:
/// the capture keeps the region live from the bind to that site, so a source
/// write between them stays a `write_while_borrowed` conflict instead of
/// silently passing because the use lives in a disconnected CFG component.
#[test]
fn first_use_inside_closure_keeps_write_conflict() {
    let result = analyze_source(
        r#"
incipit {
    varia lista<numerus> xs ← [1, 2, 3]
    varia in lista<numerus> view ← xs
    xs.appende(4)
    sit _ keeper ← () → numerus ∴ view.longitudo()
    nota keeper()
}
"#,
    );
    assert!(
        has_write_conflict(&result),
        "write between bind and capture must conflict; diagnostics: {:?}",
        result.diagnostics
    );
}

/// The nested-component variant: the first use sits two components deep (a
/// closure whose body is another closure), so both capture sites must anchor
/// the region and the inner anchoring alone would not reach the write.
#[test]
fn first_use_inside_nested_closure_keeps_write_conflict() {
    let result = analyze_source(
        r#"
incipit {
    varia lista<numerus> xs ← [1, 2, 3]
    varia in lista<numerus> view ← xs
    xs.appende(4)
    sit _ outer ← () → () → numerus ∴ () → numerus ∴ view.longitudo()
    nota outer()()
}
"#,
    );
    assert!(
        has_write_conflict(&result),
        "write before a nested capture must conflict; diagnostics: {:?}",
        result.diagnostics
    );
}

/// A reduction lane body is a separate component too: the lane's use of the
/// borrower must anchor at the reduction site.
#[test]
fn first_use_inside_reduction_lane_keeps_write_conflict() {
    let result = analyze_source(
        r#"
incipit {
    varia lista<numerus> xs ← [1, 2, 3]
    varia in lista<numerus> view ← xs
    xs.appende(4)
    sit numerus total ← xs.reducta((numerus a, numerus b) ∴ a + view.longitudo(), 0)
    nota total
}
"#,
    );
    assert!(
        has_write_conflict(&result),
        "write before the lane's use must conflict; diagnostics: {:?}",
        result.diagnostics
    );
}

// ── CFG-builder coverage: value-position control flow, conditions, args ─────

/// A control expression nested inside a value expression must go through
/// the CFG builder: its condition and branch statements carry locations, and
/// the borrow bound inside gets a real region instead of being dropped.  The
/// Faber surface reaches this shape through lowered forms (a guarded `iace
/// … si`, a `⊥` default); the hand-built module pins the structural contract.
#[test]
fn nested_control_expression_in_value_position_gets_cfg_nodes() {
    let borrow_local = radix_hir::HirLocal {
        def_id: radix_hir::DefId(7),
        name: radix_lexer::Symbol(1),
        ty: None,
        init: Some(hand_expression(
            3,
            HirExpressionKind::Ref(
                radix_hir::HirRefKind::Mutable,
                Box::new(hand_expression(
                    9,
                    HirExpressionKind::Path(radix_hir::DefId(8)),
                )),
            ),
        )),
        mutable: false,
        is_await: false,
        runtime_provided: false,
        await_store: None,
        size_decl: false,
        size_static: false,
    };
    let bind = hand_statement(2, radix_hir::HirStatementKind::Local(borrow_local));
    let use_of_borrow = hand_statement(
        6,
        radix_hir::HirStatementKind::Expr(hand_expression(
            10,
            HirExpressionKind::Path(radix_hir::DefId(7)),
        )),
    );
    // `1 + (si cond { bind; use } secus { })`: the `si` is a value operand.
    let nested_si = hand_expression(
        30,
        HirExpressionKind::Si {
            cond: Box::new(hand_expression(
                20,
                HirExpressionKind::Literal(radix_hir::HirLiteral::Bool(true)),
            )),
            then_block: HirBlock {
                statements: vec![bind, use_of_borrow],
                ..HirBlock::default()
            },
            then_catch: None,
            else_block: Some(HirBlock::default()),
        },
    );
    let sum = hand_expression(
        40,
        HirExpressionKind::Binary(
            radix_hir::HirBinOp::Add,
            Box::new(hand_expression(
                41,
                HirExpressionKind::Literal(radix_hir::HirLiteral::Int(1)),
            )),
            Box::new(nested_si),
        ),
    );
    let module = module_with_entry(HirBlock {
        statements: vec![hand_statement(50, radix_hir::HirStatementKind::Expr(sum))],
        ..HirBlock::default()
    });

    let facts = borrow_region::analyze(
        &module,
        &crate::TypeTable::new(),
        &crate::AccessFactTable::default(),
    )
    .expect("nested control expression must be covered");
    let cfg = facts.cfg();
    assert!(
        cfg.node_for_hir_id(HirId(20)).is_some(),
        "the nested condition must carry a node"
    );
    assert!(
        cfg.node_for_hir_id(HirId(2)).is_some(),
        "the branch-local bind must carry a node"
    );
    assert!(
        facts.region_for(radix_hir::DefId(7)).is_some(),
        "the branch-local borrow must get a region"
    );
}

/// A closure written directly in an `si` condition is a component of the
/// condition: the builder must give its body a component, so a borrow bound
/// inside it keeps a real region (with located statements) instead of being
/// silently dropped for the block-scope fallback.
#[test]
fn closure_in_condition_body_gets_real_regions() {
    #[derive(Default)]
    struct ComponentBorrows {
        depth: usize,
        borrowers: Vec<radix_hir::DefId>,
        statements: Vec<HirId>,
    }
    impl HirVisitor for ComponentBorrows {
        fn visit_expr(&mut self, expression: &HirExpression) {
            if matches!(expression.kind, HirExpressionKind::Clausura(..)) {
                self.depth += 1;
                walk_expr(self, expression);
                self.depth -= 1;
                return;
            }
            walk_expr(self, expression);
        }

        fn visit_stmt(&mut self, statement: &radix_hir::HirStatement) {
            if self.depth > 0 {
                self.statements.push(statement.id);
                if let radix_hir::HirStatementKind::Local(local) = &statement.kind {
                    self.borrowers.push(local.def_id);
                }
            }
            radix_hir::visit::walk_stmt(self, statement);
        }
    }

    let result = analyze_source(
        r#"
functio usatur(() → numerus f) → numerus {
    redde f()
}
incipit {
    varia lista<numerus> xs ← [1, 2, 3]
    si usatur(() → numerus ∴ fac {
        varia in lista<numerus> v ← xs
        redde v.longitudo()
    }) ≻ 1 {
        nota 1
    }
}
"#,
    );
    let hir = result.hir.as_ref().expect("HIR");
    let mut shape = ComponentBorrows::default();
    shape.visit_program(hir);
    assert!(
        !shape.borrowers.is_empty(),
        "fixture must bind a borrow inside the condition-hosted closure"
    );
    let facts = borrow_region::analyze(
        hir,
        &result.types,
        result.access_facts.as_ref().expect("access facts"),
    )
    .expect("regions");
    for borrower in shape.borrowers {
        assert!(
            facts.region_for(borrower).is_some(),
            "a borrow bound inside a condition-hosted closure must get a region"
        );
    }
    for statement in shape.statements {
        assert!(
            facts.cfg().node_for_hir_id(statement).is_some(),
            "every statement inside the condition-hosted closure must carry a CFG location"
        );
    }
}

/// The `dum` condition is re-evaluated every iteration, so a component in it
/// belongs on the loop cycle: the capture keeps the region live around the
/// back edge and the body write stays a conflict.
#[test]
fn closure_in_dum_condition_stays_live_around_the_back_edge() {
    let result = analyze_source(
        r#"
functio usatur(() → numerus f) → numerus {
    redde f()
}
incipit {
    varia lista<numerus> xs ← [1, 2, 3]
    varia in lista<numerus> view ← xs
    dum usatur(() → numerus ∴ view.longitudo()) ≺ 5 {
        xs.appende(4)
    }
}
"#,
    );
    assert!(
        has_write_conflict(&result),
        "body write while the condition-position capture is live must conflict; diagnostics: {:?}",
        result.diagnostics
    );
}

/// Chain-call arguments already run through the builder's nested-component
/// walk: pin that a closure written directly in a call argument anchors its
/// capture at the call site.
#[test]
fn chain_call_argument_closure_gets_real_regions() {
    let result = analyze_source(
        r#"
functio usatur(() → numerus f) → numerus {
    redde f()
}
incipit {
    varia lista<numerus> xs ← [1, 2, 3]
    varia in lista<numerus> view ← xs
    xs.appende(4)
    nota usatur(() → numerus ∴ view.longitudo())
}
"#,
    );
    assert!(
        has_write_conflict(&result),
        "write before a call-argument capture must conflict; diagnostics: {:?}",
        result.diagnostics
    );
}

/// The null-safe index payload (`xs?[…]`) evaluates in the enclosing flow:
/// a component written inside it must get its own CFG location instead of
/// silently keying to the ambient statement.
#[test]
fn null_safe_index_payload_gets_cfg_nodes() {
    #[derive(Default)]
    struct PayloadClosures(Vec<HirId>);
    impl HirVisitor for PayloadClosures {
        fn visit_expr(&mut self, expression: &HirExpression) {
            match &expression.kind {
                HirExpressionKind::OptionalChain(
                    _,
                    radix_hir::HirOptionalChainKind::Index(index),
                ) => {
                    let mut nested = PayloadClosures::default();
                    nested.visit_expr(index);
                    self.0.extend(nested.0);
                }
                HirExpressionKind::Clausura(_, _, _, _) => self.0.push(expression.id),
                _ => {}
            }
            walk_expr(self, expression);
        }
    }

    let result = analyze_en(
        r#"
fn usatur(() → int f) → int {
    return f()
}
main {
    mut list<int> ∪ none xs ← [1, 2, 3]
    print xs?[usatur(() → int ∴ 7)]
}
"#,
    );
    let hir = result.hir.as_ref().expect("HIR");
    let mut site = PayloadClosures::default();
    site.visit_program(hir);
    assert!(
        !site.0.is_empty(),
        "fixture must host a closure in the payload"
    );
    let facts = borrow_region::analyze(
        hir,
        &result.types,
        result.access_facts.as_ref().expect("access facts"),
    )
    .expect("regions");
    for closure in site.0 {
        assert!(
            facts.cfg().node_for_hir_id(closure).is_some(),
            "a closure inside the null-safe index payload must run through the CFG builder"
        );
    }
}

// ── Fail-closed: an unlocated borrow start is a named SEM008 gap ────────────

/// An `IncDec` lvalue operand is the one position the CFG builder does not
/// walk. A borrow bound inside it has no start location, so the engine must
/// fail closed with the named `region_facts_not_built` diagnostic instead of
/// silently dropping the candidate and keeping the block-scope approximation.
#[test]
fn unlocated_borrow_start_fails_closed_with_the_named_diagnostic() {
    let borrow_local = radix_hir::HirLocal {
        def_id: radix_hir::DefId(7),
        name: radix_lexer::Symbol(1),
        ty: None,
        init: Some(hand_expression(
            3,
            HirExpressionKind::Ref(
                radix_hir::HirRefKind::Mutable,
                Box::new(hand_expression(
                    9,
                    HirExpressionKind::Path(radix_hir::DefId(8)),
                )),
            ),
        )),
        mutable: false,
        is_await: false,
        runtime_provided: false,
        await_store: None,
        size_decl: false,
        size_static: false,
    };
    let module = module_with_entry(HirBlock {
        statements: vec![hand_statement(
            5,
            radix_hir::HirStatementKind::IncDec(radix_hir::HirIncDec {
                target: hand_expression(
                    4,
                    HirExpressionKind::Block(HirBlock {
                        statements: vec![hand_statement(
                            2,
                            radix_hir::HirStatementKind::Local(borrow_local),
                        )],
                        ..HirBlock::default()
                    }),
                ),
                op: radix_hir::HirIncDecOp::Inc,
            }),
        )],
        ..HirBlock::default()
    });

    let error = borrow_region::analyze(
        &module,
        &crate::TypeTable::new(),
        &crate::AccessFactTable::default(),
    )
    .expect_err("an unlocated borrow start must fail closed");
    assert!(
        matches!(error, borrow_region::RegionError::MissingLocation { .. }),
        "unexpected error: {error}"
    );
    let diagnostic = borrow_region::fail_closed_diagnostic(&error).expect("fail closed");
    let issue_arg = diagnostic
        .args
        .iter()
        .find(|arg| arg.name == "issue")
        .expect("issue arg");
    assert_eq!(issue_arg.value, borrow_region::ISSUE_REGION_FACTS_NOT_BUILT);
    assert_eq!(
        crate::catalog_semantic::semantic_spec(diagnostic.kind).code,
        "SEM008"
    );
}
