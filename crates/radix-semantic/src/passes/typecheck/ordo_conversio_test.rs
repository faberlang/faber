//! D9.4: built-in `ordo` conversion rows.
//!
//! `Ordo ↦ numerus` (declared discriminant; infallible), `numerus ↦ Ordo`
//! (failable: no matching member) and `Ordo ↦ textus` (member name;
//! infallible). Typecheck records the row on the `Conversio` node so every
//! backend lowers the same decision.

use crate::test_support::{analyze_ok_la, assert_rejects_with_issue_la};
use radix_hir::visit::{HirVisitor, walk_expr};
use radix_hir::{HirConversioRoute, HirExpression, HirExpressionKind, HirOrdoRow};

#[derive(Default)]
struct RouteCollector {
    routes: Vec<HirConversioRoute>,
}

impl HirVisitor for RouteCollector {
    fn visit_expr(&mut self, expr: &HirExpression) {
        if let HirExpressionKind::Conversio {
            route: Some(route), ..
        } = &expr.kind
        {
            self.routes.push(route.clone());
        }
        walk_expr(self, expr);
    }
}

fn routes(source: &str) -> Vec<HirConversioRoute> {
    let result = analyze_ok_la(source);
    let mut collector = RouteCollector::default();
    collector.visit_program(result.hir.as_ref().expect("hir"));
    collector.routes
}

fn only_ordo_route(source: &str) -> (HirOrdoRow, Vec<Option<i64>>) {
    let routes = routes(source);
    assert_eq!(routes.len(), 1, "routes: {routes:?}");
    let HirConversioRoute::Ordo { row, members } = &routes[0] else {
        panic!("expected ordo route, got {routes:?}");
    };
    (*row, members.iter().map(|member| member.value).collect())
}

const COLOR: &str = "ordo Color { rubrum, viridis, caeruleum }\n";
const STATUS: &str = "ordo Status { pendens = 10, actum, finitum = -3, deletum }\n";

#[test]
fn ordo_to_numerus_records_implicit_discriminants() {
    let (row, values) = only_ordo_route(&format!(
        "{COLOR}incipit {{ sit numerus n ← Color.viridis ↦ numerus\n nota n }}"
    ));
    assert_eq!(row, HirOrdoRow::ToNumerus);
    assert_eq!(values, vec![Some(0), Some(1), Some(2)]);
}

#[test]
fn ordo_to_numerus_continues_from_declared_discriminants() {
    let (row, values) = only_ordo_route(&format!(
        "{STATUS}incipit {{ sit numerus n ← Status.actum ↦ numerus\n nota n }}"
    ));
    assert_eq!(row, HirOrdoRow::ToNumerus);
    assert_eq!(values, vec![Some(10), Some(11), Some(-3), Some(-2)]);
}

#[test]
fn ordo_to_textus_records_member_names() {
    let routes = routes(&format!(
        "{COLOR}incipit {{ sit textus s ← Color.caeruleum ↦ textus\n nota s }}"
    ));
    let [HirConversioRoute::Ordo { row, members }] = routes.as_slice() else {
        panic!("expected one ordo route, got {routes:?}");
    };
    assert_eq!(*row, HirOrdoRow::ToTextus);
    assert_eq!(members.len(), 3);
}

#[test]
fn numerus_to_ordo_with_default_is_accepted() {
    let (row, _) = only_ordo_route(&format!(
        "{COLOR}incipit {{ sit Color c ← 7 ↦ Color ⊥ Color.rubrum\n nota 1 }}"
    ));
    assert_eq!(row, HirOrdoRow::FromNumerus);
}

#[test]
fn numerus_to_ordo_propagates_through_a_textus_channel() {
    let (row, _) = only_ordo_route(&format!(
        "{COLOR}functio lege(numerus n) → Color ⇥ textus {{ redde n ↦ Color }}\nincipit {{ nota 1 }}"
    ));
    assert_eq!(row, HirOrdoRow::FromNumerus);
}

#[test]
fn numerus_to_ordo_propagation_requires_a_textus_channel() {
    assert_rejects_with_issue_la(
        &format!("{COLOR}functio lege(numerus n) → Color ⇥ numerus {{ redde n ↦ Color }}"),
        "conversio_failure_channel_textus_required",
    );
}

#[test]
fn infallible_ordo_rows_reject_a_default() {
    assert_rejects_with_issue_la(
        &format!("{COLOR}incipit {{ sit numerus n ← Color.viridis ↦ numerus ⊥ 0\n nota n }}"),
        "ordo_conversio_recovery_unsupported",
    );
    assert_rejects_with_issue_la(
        &format!("{COLOR}incipit {{ sit textus s ← Color.viridis ↦ textus ⊥ \"x\"\n nota s }}"),
        "ordo_conversio_recovery_unsupported",
    );
}

#[test]
fn string_valued_ordo_has_no_numeric_rows() {
    let source = "ordo Via { domus = \"home\", opus = \"work\" }\n";
    assert_rejects_with_issue_la(
        &format!("{source}incipit {{ sit numerus n ← Via.opus ↦ numerus\n nota n }}"),
        "ordo_conversio_string_member",
    );
    assert_rejects_with_issue_la(
        &format!("{source}incipit {{ sit Via v ← 1 ↦ Via ⊥ Via.domus\n nota 1 }}"),
        "ordo_conversio_string_member",
    );
    let (row, values) = only_ordo_route(&format!(
        "{source}incipit {{ sit textus s ← Via.opus ↦ textus\n nota s }}"
    ));
    assert_eq!(row, HirOrdoRow::ToTextus);
    assert_eq!(values, vec![None, None]);
}

#[test]
fn other_ordo_pairs_stay_unsupported() {
    assert_rejects_with_issue_la(
        &format!("{COLOR}incipit {{ sit fractus f ← Color.viridis ↦ fractus\n nota f }}"),
        "unsupported_conversio",
    );
    assert_rejects_with_issue_la(
        &format!("{COLOR}incipit {{ sit Color c ← \"viridis\" ↦ Color ⊥ Color.rubrum\n nota 1 }}"),
        "unsupported_conversio",
    );
}

#[test]
fn discretio_has_no_ordo_rows() {
    assert_rejects_with_issue_la(
        "discretio Figura { Circulus { fractus r } }\nfunctio f(Figura x) → numerus { redde x ↦ numerus }",
        "unsupported_conversio",
    );
}

#[test]
fn conversio_assign_through_an_ordo_row_fails_closed() {
    assert_rejects_with_issue_la(
        &format!("{COLOR}incipit {{ varia numerus n ← 0\n n ↤ Color.viridis\n nota n }}"),
        "conversio_assign_route_unsupported",
    );
}

#[test]
fn numerus_to_ordo_probes_only_the_first_member_with_a_value() {
    let (row, values) = only_ordo_route(
        "ordo Gradus { primus = 1, alter = 1, tertius }\nincipit { sit Gradus g ← 1 ↦ Gradus ⊥ Gradus.primus\n nota 1 }",
    );
    assert_eq!(row, HirOrdoRow::FromNumerus);
    assert_eq!(values, vec![Some(1), None, Some(2)]);
}
