//! NTH-3: erasure, labeled-actual checking, and template arity at application.

use crate::test_support;
use radix_hir::{HirExpression, HirExpressionKind, HirLiteral, HirObjectKey, HirStatementKind};

fn entry_init(unit: &test_support::AnalyzedFixture) -> &HirExpression {
    let hir = unit.hir.as_ref().expect("hir");
    let entry = hir.entry.as_ref().expect("entry");
    entry
        .statements
        .iter()
        .find_map(|stmt| match &stmt.kind {
            HirStatementKind::Local(local) => local.init.as_ref(),
            _ => None,
        })
        .expect("entry local initializer")
}

fn describe_arg(unit: &test_support::AnalyzedFixture, kind: &HirExpressionKind) -> String {
    match kind {
        HirExpressionKind::Literal(HirLiteral::String(symbol)) => {
            format!("str:{}", unit.interner.resolve(*symbol))
        }
        HirExpressionKind::Literal(HirLiteral::Int(value)) => format!("int:{value}"),
        HirExpressionKind::Literal(HirLiteral::Bool(value)) => format!("bool:{value}"),
        _ => format!("{kind:?}"),
    }
}

fn scriptum_application(source: &str) -> (String, Vec<String>) {
    let unit = test_support::analyze_ok(source);
    let init = entry_init(&unit);
    let HirExpressionKind::Scriptum(template, args) = &init.kind else {
        panic!("expected Scriptum, got {:?}", init.kind);
    };
    (
        unit.interner.resolve(*template).to_owned(),
        args.iter()
            .map(|arg| describe_arg(&unit, &arg.kind))
            .collect(),
    )
}

fn literal_text(source: &str) -> String {
    let unit = test_support::analyze_ok(source);
    let init = entry_init(&unit);
    let HirExpressionKind::Literal(HirLiteral::String(symbol)) = &init.kind else {
        panic!("expected string literal, got {:?}", init.kind);
    };
    unit.interner.resolve(*symbol).to_owned()
}

fn forma_template_text(source: &str) -> String {
    let unit = test_support::analyze_ok(source);
    let init = entry_init(&unit);
    let HirExpressionKind::Verte {
        entries: Some(entries),
        ..
    } = &init.kind
    else {
        panic!("expected forma Verte, got {:?}", init.kind);
    };
    for field in entries {
        let HirObjectKey::Ident(_) = &field.key else {
            continue;
        };
        let Some(value) = &field.value else {
            continue;
        };
        if let HirExpressionKind::Literal(HirLiteral::String(symbol)) = &value.kind {
            return unit.interner.resolve(*symbol).to_owned();
        }
    }
    panic!("forma capture missing template string, entries={entries:?}");
}

#[test]
fn bare_named_hole_literal_stays_raw() {
    let text = literal_text(
        r#"
incipit {
    fixum textus raw ← "§{greet} world"
    nota raw
}
"#,
    );
    assert_eq!(text, "§{greet} world");
}

#[test]
fn positional_named_hole_application_erases_to_section_form() {
    let (template, args) = scriptum_application(
        r#"
incipit {
    fixum textus out ← "§{greet} world"("salve")
    nota out
}
"#,
    );
    assert_eq!(template, "§ world");
    assert_eq!(args, vec!["str:salve".to_owned()]);
}

#[test]
fn labeled_actual_erases_and_reorders_identically_to_positional() {
    let labeled = scriptum_application(
        r#"
incipit {
    fixum textus out ← "§{greet} world"(greet: "salve")
    nota out
}
"#,
    );
    let positional = scriptum_application(
        r#"
incipit {
    fixum textus out ← "§ world"("salve")
    nota out
}
"#,
    );
    assert_eq!(labeled, positional);
    assert_eq!(labeled.0, "§ world");
}

#[test]
fn labeled_actuals_reorder_to_hole_positions() {
    let labeled = scriptum_application(
        r#"
incipit {
    fixum textus out ← "§{a} §{b}"(b: "two", a: "one")
    nota out
}
"#,
    );
    let positional = scriptum_application(
        r#"
incipit {
    fixum textus out ← "§ §"("one", "two")
    nota out
}
"#,
    );
    assert_eq!(labeled, positional);
    assert_eq!(labeled.1, vec!["str:one".to_owned(), "str:two".to_owned()]);
}

#[test]
fn mixed_named_and_anonymous_holes_remain_positional() {
    let (template, args) = scriptum_application(
        r#"
incipit {
    fixum textus out ← "§{greet} §"("salve", "mundus")
    nota out
}
"#,
    );
    assert_eq!(template, "§ §");
    assert_eq!(args, vec!["str:salve".to_owned(), "str:mundus".to_owned()]);
}

#[test]
fn under_arity_template_application_is_rejected() {
    test_support::assert_rejects_with_issue(
        r#"
incipit {
    fixum textus out ← "§ §"("x")
    nota out
}
"#,
        "template_application_arity_mismatch",
    );
}

#[test]
fn over_arity_template_application_is_rejected() {
    test_support::assert_rejects_with_issue(
        r#"
incipit {
    fixum textus out ← "§"("x", "y")
    nota out
}
"#,
        "template_application_arity_mismatch",
    );
}

#[test]
fn unknown_label_at_call_site_is_rejected() {
    test_support::assert_rejects_with_issue(
        r#"
incipit {
    fixum textus out ← "§{greet} world"(other: "salve")
    nota out
}
"#,
        "unknown_template_hole_label",
    );
}

#[test]
fn duplicate_named_hole_at_application_is_rejected() {
    test_support::assert_rejects_with_issue(
        r#"
incipit {
    fixum textus out ← "§{a} §{a}"("x", "y")
    nota out
}
"#,
        "duplicate_template_hole_label",
    );
}

#[test]
fn mixed_labeled_and_positional_actuals_are_rejected() {
    test_support::assert_rejects_with_issue(
        r#"
incipit {
    fixum textus out ← "§{a} §"("x", a: "y")
    nota out
}
"#,
        "mixed_template_actuals_unsupported",
    );
}

#[test]
fn unfillable_named_hole_is_rejected() {
    test_support::assert_rejects_with_issue(
        r#"
incipit {
    fixum textus out ← "§{a} §{b}"(a: "x", extra: "y")
    nota out
}
"#,
        "unfillable_template_hole",
    );
}

#[test]
fn labeled_actual_on_ordinary_call_is_rejected_at_parse() {
    test_support::assert_rejects_with_issue(
        r#"
functio f(textus x) → textus {
    redde x
}
incipit {
    fixum textus out ← f(greet: "x")
    nota out
}
"#,
        "labeled_actual_non_template_call",
    );
}

#[test]
fn scriptum_keyword_is_not_arity_checked() {
    test_support::assert_accepts(
        r#"
incipit {
    fixum textus out ← scriptum("§ §", "x")
    nota out
}
"#,
    );
}

#[test]
fn labeled_forma_application_erases_template_text() {
    let labeled = forma_template_text(
        r#"
incipit {
    fixum _ captured ← `hello §{id}`(id: 1)
    nota captured
}
"#,
    );
    let positional = forma_template_text(
        r#"
incipit {
    fixum _ captured ← `hello §`(1)
    nota captured
}
"#,
    );
    assert_eq!(labeled, "hello §");
    assert_eq!(labeled, positional);
}

#[test]
fn bare_forma_literal_keeps_named_hole_text() {
    let text = forma_template_text(
        r#"
incipit {
    fixum _ captured ← `hello §{id}`
    nota captured
}
"#,
    );
    assert_eq!(text, "hello §{id}");
}
