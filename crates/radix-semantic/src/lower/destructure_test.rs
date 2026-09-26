//! D14.1 / D14.2: destructuring evaluates its initializer once and binds the
//! rest name to the tail, never the whole value.

use crate::test_support;
use radix_hir::{
    HirDestructurePattern, HirExpression, HirExpressionKind, HirLiteral, HirLocal,
    HirStatementKind, visit::HirVisitor,
};

const PAR: &str = "functio par() → lista<numerus> {\n    redde [7, 8, 9]\n}\n";

fn entry_locals(unit: &test_support::AnalyzedFixture) -> Vec<&HirLocal> {
    let hir = unit.hir.as_ref().expect("hir");
    let entry = hir.entry.as_ref().expect("entry");
    entry
        .statements
        .iter()
        .filter_map(|stmt| match &stmt.kind {
            HirStatementKind::Local(local) => Some(local),
            _ => None,
        })
        .collect()
}

fn local<'u>(
    unit: &'u test_support::AnalyzedFixture,
    locals: &[&'u HirLocal],
    name: &str,
) -> &'u HirLocal {
    locals
        .iter()
        .copied()
        .find(|local| unit.interner.resolve(local.name) == name)
        .unwrap_or_else(|| panic!("local `{name}` missing"))
}

/// Count free-function calls in the entry block.
fn entry_call_count(unit: &test_support::AnalyzedFixture) -> usize {
    struct Calls(usize);
    impl HirVisitor for Calls {
        fn visit_expr(&mut self, expr: &HirExpression) {
            if matches!(expr.kind, HirExpressionKind::Call(..)) {
                self.0 += 1;
            }
            radix_hir::visit::walk_expr(self, expr);
        }
    }
    let entry = unit
        .hir
        .as_ref()
        .expect("hir")
        .entry
        .as_ref()
        .expect("entry");
    let mut calls = Calls(0);
    for stmt in &entry.statements {
        calls.visit_stmt(stmt);
    }
    calls.0
}

#[test]
fn array_destructuring_calls_initializer_once() {
    let unit = test_support::analyze_ok(&format!(
        "{PAR}incipit {{\n    fixum [a, b, ceteri r] ← par()\n    nota a + b\n    nota r\n}}\n"
    ));
    assert_eq!(entry_call_count(&unit), 1);
}

#[test]
fn object_destructuring_calls_initializer_once() {
    let unit = test_support::analyze_ok(
        "genus Punctum {\n    numerus x\n    numerus y\n}\nfunctio fac() → Punctum {\n    redde Punctum { x = 1, y = 2 }\n}\nincipit {\n    fixum {x, y} ← fac()\n    nota x + y\n}\n",
    );
    assert_eq!(entry_call_count(&unit), 1);
}

#[test]
fn array_rest_binds_the_tail() {
    let unit = test_support::analyze_ok(
        "incipit {\n    fixum [caput, ceteri cauda] ← [1, 2, 3, 4, 5]\n    nota caput\n    nota cauda\n}\n",
    );
    let locals = entry_locals(&unit);
    let cauda = local(&unit, &locals, "cauda");
    let Some(HirExpression {
        kind: HirExpressionKind::MethodCall(_, method, _, args, _),
        ..
    }) = &cauda.init
    else {
        panic!("rest must read a tail, got {:?}", cauda.init);
    };
    assert_eq!(unit.interner.resolve(*method), "omissa");
    assert!(matches!(
        args.as_slice(),
        [arg] if matches!(arg.expr.kind, HirExpressionKind::Literal(HirLiteral::Int(1)))
    ));
}

#[test]
fn rest_after_holes_skips_every_authored_position() {
    let unit = test_support::analyze_ok(
        "incipit {\n    fixum _ xs ← [1, 2, 3, 4]\n    fixum [_, b, ceteri r] ← xs\n    nota b\n    nota r\n}\n",
    );
    let locals = entry_locals(&unit);
    let r = local(&unit, &locals, "r");
    let Some(HirExpression {
        kind: HirExpressionKind::MethodCall(_, _, _, args, _),
        ..
    }) = &r.init
    else {
        panic!("rest must read a tail, got {:?}", r.init);
    };
    assert!(matches!(
        args[0].expr.kind,
        HirExpressionKind::Literal(HirLiteral::Int(2))
    ));
}

#[test]
fn place_subject_is_not_hoisted_and_call_subject_is() {
    let unit = test_support::analyze_ok(&format!(
        "{PAR}incipit {{\n    fixum _ xs ← [1, 2]\n    fixum [a, b] ← xs\n    fixum [c, d] ← par()\n    nota a + b + c + d\n}}\n"
    ));
    let facts = &unit.presentation.destructure_bindings;
    assert_eq!(facts.len(), 2);
    assert!(!facts[0].hoisted);
    assert_eq!(facts[0].stmt_ids.len(), 2);
    assert!(facts[1].hoisted);
    assert_eq!(facts[1].stmt_ids.len(), 3);
    assert!(matches!(
        &facts[1].pattern,
        HirDestructurePattern::Array { elements, rest: None } if elements.len() == 2
    ));
}
