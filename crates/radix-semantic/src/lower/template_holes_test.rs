//! NTH-3: erasure, labeled-actual checking, and template arity at application.
//! NTH-4: semantic HIR and forma parity pins for named template holes.

use crate::test_support;
use radix_hir::{
    HirArrayElement, HirExpression, HirExpressionKind, HirLiteral, HirObjectKey, HirStatementKind,
};
use std::fmt::Write;

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
    let unit = test_support::analyze_ok_la(source);
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
    let unit = test_support::analyze_ok_la(source);
    let init = entry_init(&unit);
    let HirExpressionKind::Literal(HirLiteral::String(symbol)) = &init.kind else {
        panic!("expected string literal, got {:?}", init.kind);
    };
    unit.interner.resolve(*symbol).to_owned()
}

fn forma_template_text(source: &str) -> String {
    let unit = test_support::analyze_ok_la(source);
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

fn forma_capture(source: &str) -> (String, Vec<String>) {
    let unit = test_support::analyze_ok_la(source);
    let init = entry_init(&unit);
    let HirExpressionKind::Verte {
        entries: Some(entries),
        ..
    } = &init.kind
    else {
        panic!("expected forma Verte, got {:?}", init.kind);
    };
    let mut template = None;
    let mut params = None;
    for field in entries {
        let HirObjectKey::Ident(key) = &field.key else {
            continue;
        };
        let Some(value) = &field.value else {
            continue;
        };
        match unit.interner.resolve(*key) {
            "template" => {
                let HirExpressionKind::Literal(HirLiteral::String(symbol)) = &value.kind else {
                    panic!("forma template field is not a string: {value:?}");
                };
                template = Some(unit.interner.resolve(*symbol).to_owned());
            }
            "params" => {
                let HirExpressionKind::Array(elements) = &value.kind else {
                    panic!("forma params field is not an array: {value:?}");
                };
                params = Some(
                    elements
                        .iter()
                        .map(|element| match element {
                            HirArrayElement::Expr(expr) => describe_arg(&unit, &expr.kind),
                            HirArrayElement::Spread(expr) => {
                                format!("spread:{:?}", expr.kind)
                            }
                            HirArrayElement::Infer(_) => "shape-hole".to_owned(),
                        })
                        .collect(),
                );
            }
            _ => {}
        }
    }
    (
        template.expect("forma template field"),
        params.expect("forma params field"),
    )
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
fn template_literal_text_keeps_fullwidth_punctuation() {
    let plain = literal_text(
        r#"
incipit {
    fixum textus raw ← "a，b、c。é"
    nota raw
}
"#,
    );
    assert_eq!(plain, "a，b、c。é");
    let (template, args) = scriptum_application(
        r#"
incipit {
    fixum textus out ← "a，§，b、c。é"("salve")
    nota out
}
"#,
    );
    assert_eq!(template, "a，§，b、c。é");
    assert_eq!(args, vec!["str:salve".to_owned()]);
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
fn repeated_numbered_hole_takes_one_value() {
    let (template, args) = scriptum_application(
        r#"
incipit {
    fixum textus out ← "§0, §0, §0, and §1"("her", "me")
    nota out
}
"#,
    );
    assert_eq!(template, "§0, §0, §0, and §1");
    assert_eq!(args, vec!["str:her".to_owned(), "str:me".to_owned()]);
}

#[test]
fn repeated_numbered_hole_with_duplicate_values_is_rejected() {
    test_support::assert_rejects_with_issue_la(
        r#"
incipit {
    fixum textus out ← "§0, §0, §0, and §1"("her", "her", "her", "me")
    nota out
}
"#,
        "template_application_arity_mismatch",
    );
}

#[test]
fn gap_in_numbered_holes_is_rejected() {
    test_support::assert_rejects_with_issue_la(
        r#"
incipit {
    fixum textus out ← "§0 and §2"("a", "b", "c")
    nota out
}
"#,
        "template_hole_index_gap",
    );
    test_support::assert_rejects_with_issue_la(
        r#"
incipit {
    fixum textus out ← "§0 and §2"("a", "b")
    nota out
}
"#,
        "template_hole_index_gap",
    );
}

#[test]
fn under_arity_template_application_is_rejected() {
    test_support::assert_rejects_with_issue_la(
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
    test_support::assert_rejects_with_issue_la(
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
    test_support::assert_rejects_with_issue_la(
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
    test_support::assert_rejects_with_issue_la(
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
    test_support::assert_rejects_with_issue_la(
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
    test_support::assert_rejects_with_issue_la(
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
    test_support::assert_rejects_with_issue_la(
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
    test_support::assert_accepts_la(
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
        r"
incipit {
    fixum _ captured ← `hello §{id}`(id: 1)
    nota captured
}
",
    );
    let positional = forma_template_text(
        r"
incipit {
    fixum _ captured ← `hello §`(1)
    nota captured
}
",
    );
    assert_eq!(labeled, "hello §");
    assert_eq!(labeled, positional);
}

#[test]
fn labeled_forma_capture_parity_preserves_template_and_positional_params() {
    let labeled = forma_capture(
        r"
incipit {
    fixum _ captured ← `hello §{first} §{second}`(second: 2, first: 1)
    nota captured
}
",
    );
    let positional = forma_capture(
        r"
incipit {
    fixum _ captured ← `hello § §`(1, 2)
    nota captured
}
",
    );
    assert_eq!(
        labeled,
        (
            "hello § §".to_owned(),
            vec!["int:1".to_owned(), "int:2".to_owned()]
        )
    );
    assert_eq!(labeled, positional);
}

#[test]
fn forma_repeated_numbered_hole_takes_one_value() {
    let (template, params) = forma_capture(
        r#"
incipit {
    fixum _ captured ← `x = §0 or y = §0`("v")
    nota captured
}
"#,
    );
    assert_eq!(template, "x = §0 or y = §0");
    assert_eq!(params, vec!["str:v".to_owned()]);
}

#[test]
fn bare_forma_literal_keeps_named_hole_text() {
    let text = forma_template_text(
        r"
incipit {
    fixum _ captured ← `hello §{id}`
    nota captured
}
",
    );
    assert_eq!(text, "hello §{id}");
}

/// Produce the semantic equivalent of a `radix hir` dump for parity cases.
///
/// `test_support` analyzes every source as the same `test.fab` file. Each parity
/// fixture also pads the shorter call spelling so corresponding HIR spans have
/// equal offsets; only interner-assigned Symbol ids need canonicalization.
fn canonical_hir_dump(source: &str) -> String {
    let unit = test_support::analyze_ok_la(source);
    let hir = unit.hir.as_ref().expect("hir");
    canonicalize_symbol_ids(&format!("{hir:#?}"), &unit)
}

fn assert_hir_dump_eq(label: &str, left: &str, right: &str) {
    let left_dump = canonical_hir_dump(left);
    let right_dump = canonical_hir_dump(right);
    assert_eq!(left_dump, right_dump, "{label} HIR differs");
}

fn canonicalize_symbol_ids(dump: &str, unit: &test_support::AnalyzedFixture) -> String {
    let mut canonical = String::with_capacity(dump.len());
    let mut rest = dump;
    while let Some(start) = rest.find("Symbol(") {
        canonical.push_str(&rest[..start]);
        let after_prefix = &rest[start + "Symbol(".len()..];
        let digit_start = after_prefix
            .find(|character: char| character.is_ascii_digit())
            .expect("Symbol debug output has an integer id");
        let digits = &after_prefix[digit_start..];
        let digit_end = digits
            .find(|character: char| !character.is_ascii_digit())
            .unwrap_or(digits.len());
        let id = digits[..digit_end]
            .parse::<u32>()
            .expect("Symbol debug output has a valid integer id");
        let close = digits[digit_end..]
            .find(')')
            .expect("Symbol debug output closes");
        let end = digit_start + digit_end + close + 1;
        let symbol = radix_lexer::Symbol(id);
        canonical.push_str("Symbol(");
        write!(canonical, "{:?}", unit.interner.resolve(symbol))
            .expect("formatting into String cannot fail");
        canonical.push(')');
        rest = &after_prefix[end..];
    }
    canonical.push_str(rest);
    canonical
}

#[test]
fn labeled_positional_scriptum_hir_parity_dumps_are_byte_identical() {
    let labeled = canonical_hir_dump(
        r#"
incipit {
    fixum textus out ← "§{greet} world"(greet: "salve")
    nota out
}
"#,
    );
    let positional = canonical_hir_dump(
        r#"
incipit {
    fixum textus out ← "§ world"              ("salve")
    nota out
}
"#,
    );
    let scriptum = canonical_hir_dump(
        r#"
incipit {
    fixum textus out ← scriptum(    "§ world", "salve")
    nota out
}
"#,
    );
    assert_eq!(
        labeled, positional,
        "labeled HIR differs from positional HIR"
    );
    assert_eq!(
        positional, scriptum,
        "positional HIR differs from scriptum HIR"
    );
}

#[test]
fn mixed_named_and_anonymous_template_hir_parity_is_positional() {
    assert_hir_dump_eq(
        "§{a} § ≡ § §",
        r#"
incipit {
    fixum textus out ← "§{a} §"("one", "two")
    nota out
}
"#,
        r#"
incipit {
    fixum textus out ← "§ §"   ("one", "two")
    nota out
}
"#,
    );
}

#[test]
fn explicit_index_before_named_hole_has_positional_hir_parity() {
    assert_hir_dump_eq(
        "§1 §{greet} ≡ §1 §",
        r#"
incipit {
    fixum textus out ← "§1 §{greet}"("one", "two")
    nota out
}
"#,
        r#"
incipit {
    fixum textus out ← "§1 §"       ("one", "two")
    nota out
}
"#,
    );
}

#[test]
fn named_hole_before_repeated_explicit_index_has_positional_hir_parity() {
    assert_hir_dump_eq(
        "§{greet} §0 ≡ § §0",
        r#"
incipit {
    fixum textus out ← "§{greet} §0"("one")
    nota out
}
"#,
        r#"
incipit {
    fixum textus out ← "§ §0"       ("one")
    nota out
}
"#,
    );
}
