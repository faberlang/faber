//! Fail-closed gate for native effect leaves.
//!
//! Targets declare the leaves they support ([`Target::supported_leaves`]).
//! A module that uses a leaf route the target does not declare fails before
//! emit, naming the leaf, the route, and the target. Unknown routes and
//! language builtins are not leaves and stay out of this gate.

use crate::CodegenError;
use radix_diagnostics::DiagnosticArg;
use radix_hir::visit::{HirVisitor, walk_expr, walk_program};
use radix_hir::{HirExpression, HirExpressionKind, HirModule};
use radix_lexer::Interner;
use radix_target::{Leaf, Target};

const ISSUE: &str = "leaf_unsupported_on_target";

/// Collect every `ad` / `call` route in `hir`.
pub fn collect_ad_routes(hir: &HirModule, interner: &Interner, routes: &mut Vec<String>) {
    struct Collector<'a> {
        interner: &'a Interner,
        routes: &'a mut Vec<String>,
    }
    impl HirVisitor for Collector<'_> {
        fn visit_expr(&mut self, expr: &HirExpression) {
            if let HirExpressionKind::Ad { route, .. } = &expr.kind {
                routes_push(self.routes, self.interner.resolve(*route));
            }
            walk_expr(self, expr);
        }
    }
    walk_program(&mut Collector { interner, routes }, hir);
}

fn routes_push(routes: &mut Vec<String>, route: &str) {
    if !routes.iter().any(|existing| existing == route) {
        routes.push(route.to_owned());
    }
}

/// # Errors
///
/// Returns a [`CodegenError`] (`leaf_unsupported_on_target`) when `routes`
/// names a leaf the `target` does not declare.
pub fn reject_unsupported_leaf(routes: &[String], target: Target) -> Result<(), CodegenError> {
    for route in routes {
        let Some(leaf) = Leaf::from_route(route) else {
            continue;
        };
        if target.supports_leaf(leaf) {
            continue;
        }
        return Err(CodegenError {
            message: format!(
                "the {leaf} leaf is not supported on the {target} target \
                 (route `{route}`)"
            ),
            args: vec![
                DiagnosticArg::new("issue", ISSUE),
                DiagnosticArg::new("leaf", leaf.as_str()),
                DiagnosticArg::new("route", route.as_str()),
                DiagnosticArg::new("target", target.as_str()),
            ],
        });
    }
    Ok(())
}

#[cfg(test)]
#[path = "leaf_gate_test.rs"]
mod tests;
