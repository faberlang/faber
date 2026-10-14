//! Focused proofs for target-neutral fresh-storage access facts.

use super::{
    AccessFact, AccessFactTable, DeadSourceReason, analyze_access_facts,
    analyze_access_facts_with_types,
};
use crate::test_support::analyze_ok_la;
use crate::{AnalyzedApplication, AnalyzedApplicationParts, PassConfig};
use radix_hir::{
    HirBlock, HirExpression, HirExpressionKind, HirId, HirLocal, HirModule, HirStatement,
    HirStatementKind,
};

fn la_pack() -> crate::locale::LocalePack {
    crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("la"))
        .expect("load Latin locale pack")
}

fn local_rows(source: &str) -> (radix_hir::HirModule, Vec<HirLocal>) {
    let fixture = analyze_ok_la(source);
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

fn typed_local_rows(source: &str) -> (radix_hir::HirModule, crate::TypeTable, Vec<HirLocal>) {
    let fixture = analyze_ok_la(source);
    let hir = fixture
        .result
        .hir
        .expect("successful analysis should produce HIR");
    let types = fixture.result.types;
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
    (hir, types, locals)
}

#[test]
fn fresh_array_initializer_is_recorded_at_expression_and_binding_sites() {
    let (hir, locals) = local_rows(
        r"
incipit {
    fixum lista<numerus> fresh ← [1, 2, 3]
}
",
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
        r"
functio make() → lista<numerus> {
    redde [4, 5, 6]
}
incipit {
    fixum lista<numerus> fresh ← [1, 2, 3]
    fixum lista<numerus> alias ← fresh
    fixum lista<numerus> called ← make()
}
",
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
fn own_transfer_records_dead_source_identity_and_spans() {
    let (hir, types, locals) = typed_local_rows(
        r"
incipit {
    fixum lista<numerus> source ← own [1, 2, 3]
    fixum lista<numerus> destination ← own source
}
",
    );
    let source = &locals[0];
    let destination = &locals[1];
    let transfer = destination.init.as_ref().expect("own transfer initializer");
    let facts = analyze_access_facts_with_types(&hir, &types);

    let dead = facts
        .dead_sources_of(source.def_id)
        .find(|fact| fact.reason == DeadSourceReason::OwnTransfer)
        .expect("own transfer should produce a dead-source fact");
    assert_eq!(dead.site, Some(transfer.id));
    assert_eq!(dead.source, source.def_id);
    assert_eq!(
        dead.source_span,
        source.init.as_ref().expect("source initializer").span
    );
    assert_eq!(dead.site_span, transfer.span);
}

#[test]
fn provably_dead_unmarked_source_records_transfer_death() {
    let (hir, types, locals) = typed_local_rows(
        r"
incipit {
    fixum lista<numerus> source ← [1, 2, 3]
    fixum lista<numerus> destination ← own source
}
",
    );
    let source = &locals[0];
    let facts = analyze_access_facts_with_types(&hir, &types);

    assert!(
        facts
            .dead_sources_of(source.def_id)
            .any(|fact| fact.reason == DeadSourceReason::OwnTransfer)
    );
}

#[test]
fn explicit_copy_keeps_the_exclusive_source_live() {
    let (hir, types, locals) = typed_local_rows(
        r"
incipit {
    fixum lista<numerus> source ← own [1, 2, 3]
    fixum lista<numerus> copied ← copy source
    nota source.longitudo()
}
",
    );
    let source = &locals[0];
    let facts = analyze_access_facts_with_types(&hir, &types);

    assert!(
        facts
            .dead_sources_of(source.def_id)
            .all(|fact| { fact.reason != DeadSourceReason::OwnTransfer })
    );
}

#[test]
fn scope_exit_records_an_unused_exclusive_source() {
    let (hir, types, locals) = typed_local_rows(
        r"
incipit {
    fixum lista<numerus> scoped ← own [1, 2, 3]
}
",
    );
    let source = &locals[0];
    let facts = analyze_access_facts_with_types(&hir, &types);

    let dead = facts
        .dead_sources_of(source.def_id)
        .find(|fact| fact.reason == DeadSourceReason::ScopeEnd)
        .expect("scope end should produce a dead-source fact");
    assert!(dead.site.is_none());
    assert!(dead.site_span.end >= dead.site_span.start);
}

#[test]
fn unknown_branch_does_not_widen_path_local_death() {
    let (hir, types, locals) = typed_local_rows(
        r"
functio incertum() → bivalens {
    redde verum
}
incipit {
    fixum lista<numerus> source ← own [1, 2, 3]
    si incertum() {
        fixum lista<numerus> destination ← own source
    }
}
",
    );
    let source = &locals[0];
    let facts = analyze_access_facts_with_types(&hir, &types);

    assert_eq!(facts.dead_source_count(), 0);
    assert!(facts.dead_sources_of(source.def_id).next().is_none());
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
            await_store: None,
            size_decl: false,
            size_static: false,
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
    let mut parsed = radix_parser::parse(radix_lexer::lex_with_locale_pack(
        "incipit { fixum lista<numerus> fresh ← [1, 2, 3] }",
        &la_pack(),
    ));
    assert!(parsed.success(), "parse errors: {:?}", parsed.errors);
    let program = parsed.program.take().expect("program");
    let result = crate::analyze(&program, &PassConfig::base(), &mut parsed.interner);
    assert!(
        result.success(),
        "semantic errors: {:?}",
        result.diagnostics
    );
    let access_facts = result
        .access_facts
        .expect("successful analysis should produce access facts");
    let expected_access_facts = access_facts.clone();
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
        tensor_edge_policies: result.tensor_edge_policies,
        kernel_requirements: result.kernel_requirements,
        access_facts,
        region_facts: result.region_facts,
        semantic_diagnostics: result.diagnostics,
    };
    let app = AnalyzedApplication::build(parts, &parsed.interner).expect("application");

    assert_eq!(app.access_facts(), &expected_access_facts);
    assert_eq!(app.access_facts().binding(def_id), AccessFact::Fresh);
}

#[test]
fn missing_sites_default_to_unknown_without_hir_rewalk() {
    let facts = AccessFactTable::default();

    assert_eq!(facts.expression(HirId(99)), AccessFact::Unknown);
    assert_eq!(facts.binding(radix_hir::DefId(99)), AccessFact::Unknown);
}
