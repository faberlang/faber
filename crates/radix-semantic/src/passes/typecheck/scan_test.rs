//! Semantic laws for ordered prefix scans.

use crate::test_support::{
    assert_accepts_la, assert_rejects_with_issue_la, compile_and_get_result_la,
};
use radix_hir::{HirExpressionKind, HirItemKind, HirScanKeyDomain, HirStatementKind};
use radix_types::NumericWidth;

#[test]
fn ungrouped_inclusive_and_identity_exclusive_scans_keep_source_shape() {
    assert_accepts_la(
        r#"
@ nucleum
functio summa(tensor<f32, [4]> xs) → tensor<f32, [4]> {
    redde scan via summa ex xs apud [i] filum lane sit value { redde value }
}
@ nucleum
functio productum(tensor<f32, [4]> xs) → tensor<f32, [4]> {
    redde scan via factum total exclusive ex xs apud [i] filum lane sit value { redde value }
}
@ nucleum
functio omnia(tensor<bivalens, [4]> xs) → tensor<bivalens, [4]> {
    redde scan via omnia ex xs apud [i] filum lane sit value { redde value }
}
@ nucleum
functio numeratio(tensor<bivalens, [4]> xs) → tensor<i32, [4]> {
    redde scan via numeratio total exclusive ex xs apud [i] filum lane sit value { redde value }
}
"#,
    );
}

#[test]
fn grouped_scan_carries_a_sized_finite_integer_key_domain() {
    let grouped = compile_and_get_result_la(
        r#"
@ nucleum
functio per_key(tensor<f32, [4]> xs, tensor<u8, [4]> keys) → tensor<f32, [4]> {
    redde scan via summa ex xs apud [i] pro keys[i] filum lane sit value { redde value }
}
"#,
    );
    assert!(grouped.success(), "diagnostics: {:?}", grouped.result.diagnostics);
    assert_eq!(
        first_group_domain(&grouped),
        HirScanKeyDomain::IntegerWidth(NumericWidth::U8)
    );
}

#[test]
fn group_domain_uses_coordinate_arithmetic_and_preserves_widened_bounds() {
    let modulo = compile_and_get_result_la(
        r#"
@ nucleum
functio modulo(tensor<f32, [8]> xs) → tensor<f32, [8]> {
    redde scan via summa ex xs apud [i] pro i % 4 filum lane sit value { redde value }
}
"#,
    );
    assert!(modulo.success(), "diagnostics: {:?}", modulo.result.diagnostics);
    assert_eq!(
        first_group_domain(&modulo),
        HirScanKeyDomain::IntegerRange { min: 0, max: 3 }
    );

    let widened = compile_and_get_result_la(
        r#"
@ nucleum
functio widened(tensor<i32, [4]> xs, tensor<i32, [4]> keys) → tensor<i32, [4]> {
    redde scan via summa ex xs apud [i] pro keys[i] + 1 filum lane sit value { redde value }
}
"#,
    );
    assert!(widened.success(), "diagnostics: {:?}", widened.result.diagnostics);
    assert_eq!(
        first_group_domain(&widened),
        HirScanKeyDomain::IntegerRange {
            min: i128::from(i32::MIN) + 1,
            max: i128::from(i32::MAX) + 1,
        }
    );
}

#[test]
fn exclusive_extrema_and_unbounded_group_keys_are_named_refusals() {
    assert_rejects_with_issue_la(
        r#"
@ nucleum
functio minimum(tensor<f32, [4]> xs) → tensor<f32, [4]> {
    redde scan via minima total exclusive ex xs apud [i] filum lane sit value { redde value }
}
"#,
        "scan_exclusive_extremum_unsupported",
    );
    assert_rejects_with_issue_la(
        r#"
functio label() → inf { redde 18446744073709551616 }
@ nucleum
functio unbounded(tensor<f32, [4]> xs) → tensor<f32, [4]> {
    redde scan via summa ex xs apud [i] pro label() filum lane sit value { redde value }
}
"#,
        "scan_group_domain_unbounded",
    );
}

fn first_group_domain(unit: &crate::test_support::AnalyzedFixture) -> HirScanKeyDomain {
    let function = unit
        .result
        .hir
        .as_ref()
        .expect("analyzed HIR")
        .items
        .iter()
        .find_map(|item| match &item.kind {
            HirItemKind::Function(function) => Some(function),
            _ => None,
        })
        .expect("scan function");
    let HirStatementKind::Redde(Some(value)) = &function
        .body
        .as_ref()
        .expect("body")
        .statements[0]
        .kind
    else {
        panic!("expected scan return expression");
    };
    let HirExpressionKind::Scan {
        group_domain: Some(domain),
        ..
    } = &value.kind
    else {
        panic!("expected proven grouped scan domain: {:?}", value.kind);
    };
    assert_eq!(domain.key_domains.len(), 1);
    domain.key_domains[0]
}
