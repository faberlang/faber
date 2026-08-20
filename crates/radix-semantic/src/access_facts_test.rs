//! Focused proofs for target-neutral fresh-storage access facts.

use super::{analyze_access_facts, AccessFact, AccessFactTable};
use crate::test_support::analyze_ok;
use crate::{AnalyzedApplication, AnalyzedApplicationParts, PassConfig};
use radix_hir::{
    HirBlock, HirExpression, HirExpressionKind, HirId, HirLocal, HirModule, HirStatement,
    HirStatementKind,
};

fn local_rows(source: &str) -> (radix_hir::HirModule, Vec<HirLocal>) {
    let fixture = analyze_ok(source);
    let hir = fixture
        .result
        .hir
        .expect("successful analysis should produce HIR");
    let locals = hir
        .entry
        .as_ref()
        .expect("fixture should have an entry")
        .statements
        .iter()
        .filter_map(|statement| match &statement.kind {
            HirStatementKind::Local(local) => Some(local.clone()),
            _ => None,
        })
        .collect();
    (hir, locals)
}

#[test]
fn fresh_array_initializer_is_recorded_at_expression_and_binding_sites() {
    let (hir, locals) = local_rows(
        r#"
incipit {
    fixum lista<numerus> fresh ← [1, 2, 3]
}
"#,
    );
    let local = &locals[0];
    let init = local.init.as_ref().expect("fresh local initializer");
    let facts = analyze_access_facts(&hir);

    assert_eq!(facts.expression(init.id), AccessFact::Fresh);
    assert_eq!(facts.binding(local.def_id), AccessFact::Fresh);
    assert_eq!(facts.expression_count(), 1);
    assert_eq!(facts.binding_count(), 1);
}

#[test]
fn aliases_and_calls_remain_conservatively_unknown() {
    let (hir, locals) = local_rows(
        r#"
functio make() → lista<numerus> {
    redde [4, 5, 6]
}
incipit {
    fixum lista<numerus> fresh ← [1, 2, 3]
    fixum lista<numerus> alias ← fresh
    fixum lista<numerus> called ← make()
}
"#,
    );
    let facts = analyze_access_facts(&hir);

    assert_eq!(facts.binding(locals[0].def_id), AccessFact::Fresh);
    assert_eq!(facts.binding(locals[1].def_id), AccessFact::Unknown);
    assert_eq!(facts.binding(locals[2].def_id), AccessFact::Unknown);
    assert_eq!(
        facts.expression(locals[1].init.as_ref().expect("alias initializer").id),
        AccessFact::Unknown
    );
    assert_eq!(
        facts.expression(locals[2].init.as_ref().expect("call initializer").id),
        AccessFact::Unknown
    );
}

#[test]
fn admitted_constructor_shapes_are_fresh() {
    let expressions = [
        HirExpression {
            id: HirId(1),
            kind: HirExpressionKind::Struct(radix_hir::DefId(1), Vec::new()),
            ty: None,
            span: radix_lexer::Span::default(),
        },
        HirExpression {
            id: HirId(2),
            kind: HirExpressionKind::Tuple(Vec::new(), None),
            ty: None,
            span: radix_lexer::Span::default(),
        },
        HirExpression {
            id: HirId(3),
            kind: HirExpressionKind::Verte {
                source: Box::new(HirExpression {
                    id: HirId(4),
                    kind: HirExpressionKind::Tuple(Vec::new(), None),
                    ty: None,
                    span: radix_lexer::Span::default(),
                }),
                target: radix_types::TypeId(0),
                entries: Some(Vec::new()),
            },
            ty: None,
            span: radix_lexer::Span::default(),
        },
    ];

    for expression in &expressions {
        let local = HirLocal {
            def_id: radix_hir::DefId(expression.id.0),
            name: radix_lexer::Symbol(0),
            ty: None,
            init: Some(expression.clone()),
            mutable: false,
            is_await: false,
            runtime_provided: false,
        };
        let hir = HirModule {
            items: Vec::new(),
            entry: Some(HirBlock::simple(
                vec![HirStatement {
                    id: expression.id,
                    kind: HirStatementKind::Local(local.clone()),
                    span: radix_lexer::Span::default(),
                }],
                None,
                radix_lexer::Span::default(),
            )),
            entry_annotations: Vec::new(),
            entry_args_name: None,
            entry_is_async: false,
        };
        let facts = analyze_access_facts(&hir);
        assert_eq!(facts.expression(expression.id), AccessFact::Fresh);
        assert_eq!(facts.binding(local.def_id), AccessFact::Fresh);
    }
}

#[test]
fn analyzed_application_carries_access_facts() {
    let mut parsed = radix_parser::parse(radix_lexer::lex(
        "incipit { fixum lista<numerus> fresh ← [1, 2, 3] }",
    ));
    assert!(parsed.success(), "parse errors: {:?}", parsed.errors);
    let program = parsed.program.take().expect("program");
    let result = crate::analyze(&program, &PassConfig::base(), &mut parsed.interner);
    assert!(
        result.success(),
        "semantic errors: {:?}",
        result.diagnostics
    );
    let hir = result.hir.expect("successful analysis should produce HIR");
    let local = hir
        .entry
        .as_ref()
        .expect("entry")
        .statements
        .iter()
        .find_map(|statement| match &statement.kind {
            HirStatementKind::Local(local) => Some(local),
            _ => None,
        })
        .expect("fresh local");
    let def_id = local.def_id;
    let parts = AnalyzedApplicationParts {
        hir,
        types: result.types,
        resolver: result.resolver,
        annotation_contracts: result.annotation_contracts,
        libraries: radix_hir::LibraryRegistry::default(),
        radix_lanes: result.radix_lanes,
        gpu_builtins: result.gpu_builtins,
        semantic_diagnostics: result.diagnostics,
    };
    let app = AnalyzedApplication::build(parts, &parsed.interner).expect("application");

    assert_eq!(app.access_facts().binding(def_id), AccessFact::Fresh);
}

#[test]
fn missing_sites_default_to_unknown_without_hir_rewalk() {
    let facts = AccessFactTable::default();

    assert_eq!(facts.expression(HirId(99)), AccessFact::Unknown);
    assert_eq!(facts.binding(radix_hir::DefId(99)), AccessFact::Unknown);
}
