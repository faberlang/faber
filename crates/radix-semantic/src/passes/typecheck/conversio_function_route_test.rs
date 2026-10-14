//! D9.3: a registered `@ conversio` (source, destination) pair also serves an
//! explicit `a ↦ B`. Only direct pairs, never chained; failability follows the
//! function's signature (`⇥ E` makes `↦ B` failable, usable with `⊥` or
//! propagation). Typecheck records the function on the `Conversio` node.

use crate::test_support::{analyze_ok_la, analyze_source_la, assert_rejects_with_issue_la, issue};
use radix_hir::visit::{HirVisitor, walk_expr};
use radix_hir::{HirConversioRoute, HirExpression, HirExpressionKind};

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

fn function_route_failable(source: &str) -> bool {
    let result = analyze_ok_la(source);
    let mut collector = RouteCollector::default();
    collector.visit_program(result.hir.as_ref().expect("hir"));
    let [HirConversioRoute::Function { failable, .. }] = collector.routes.as_slice() else {
        panic!("expected one function route, got {:?}", collector.routes);
    };
    *failable
}

const TEMPERATURES: &str = "
genus Celsius { varia fractus gradus }
genus Fahrenheit { varia fractus gradus }

@ conversio
functio adFahrenheit(Celsius c) → Fahrenheit {
    redde Fahrenheit { gradus = c.gradus * 9.0 / 5.0 + 32.0 }
}
";

const FAILABLE: &str = "
genus Mensura { varia numerus valor }
genus Positiva { varia numerus valor }

@ conversio
functio adPositivam(Mensura m) → Positiva ⇥ textus {
    si m.valor ≺ 0 { iace \"negativa\" }
    redde Positiva { valor = m.valor }
}
";

#[test]
fn registered_pair_serves_explicit_conversio() {
    let failable = function_route_failable(&format!(
        "{TEMPERATURES}incipit {{\n    fixum Fahrenheit f ← Celsius {{ gradus = 100.0 }} ↦ Fahrenheit\n    nota f.gradus\n}}"
    ));
    assert!(!failable);
}

#[test]
fn conversio_function_used_through_arrow_is_not_unused() {
    let result = analyze_source_la(&format!(
        "{TEMPERATURES}incipit {{\n    fixum Fahrenheit f ← Celsius {{ gradus = 100.0 }} ↦ Fahrenheit\n    nota f.gradus\n}}"
    ));
    assert!(result.is_ok(), "{:?}", result.diagnostics);
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|diag| issue(diag) == Some("unused_function")),
        "{:?}",
        result.diagnostics
    );
}

#[test]
fn failable_pair_takes_a_default() {
    let failable = function_route_failable(&format!(
        "{FAILABLE}incipit {{\n    fixum Positiva p ← Mensura {{ valor = -1 }} ↦ Positiva ⊥ Positiva {{ valor = 0 }}\n    nota p.valor\n}}"
    ));
    assert!(failable);
}

#[test]
fn failable_pair_propagates_through_the_declared_channel() {
    let failable = function_route_failable(&format!(
        "{FAILABLE}functio lege(Mensura m) → Positiva ⇥ textus {{ redde m ↦ Positiva }}\nincipit {{ nota 1 }}"
    ));
    assert!(failable);
}

#[test]
fn failable_pair_without_default_or_channel_is_rejected() {
    // D17.8: one rule for every failable `↦`, so the issue is
    // `failable_conversio_requires_handler`; `incipit` (main) is exempt, so
    // the site lives in a helper.
    assert_rejects_with_issue_la(
        &format!(
            "{FAILABLE}functio lege(Mensura m) → Positiva {{\n    redde m ↦ Positiva\n}}\nincipit {{ nota 1 }}"
        ),
        "failable_conversio_requires_handler",
    );
}

#[test]
fn failable_pair_in_main_is_exempt() {
    let _ = analyze_ok_la(&format!(
        "{FAILABLE}incipit {{\n    fixum Positiva p ← Mensura {{ valor = 1 }} ↦ Positiva\n    nota p.valor\n}}"
    ));
}

#[test]
fn registered_pairs_never_chain() {
    assert_rejects_with_issue_la(
        &format!(
            "{TEMPERATURES}genus Kelvin {{ varia fractus gradus }}
@ conversio
functio adKelvin(Fahrenheit f) → Kelvin {{ redde Kelvin {{ gradus = f.gradus }} }}
incipit {{
    fixum Kelvin k ← Celsius {{ gradus = 1.0 }} ↦ Kelvin
    nota k.gradus
}}"
        ),
        "unsupported_conversio",
    );
}
