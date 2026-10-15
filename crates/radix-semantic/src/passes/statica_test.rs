//! Coverage for top-level statics and their compile-time evaluation
//! (DECISIONS D5.7, D5.8, D5.9, D8.4).
//!
//! Contract:
//! - a `generis` static's initializer is folded to a literal: arithmetic,
//!   comparison, `textus` concatenation, and references to other statics in
//!   dependency order, so every backend sees the constant;
//! - collection literals of constants are accepted and left structural;
//! - a non-constant initializer is SEM060 with a named issue;
//! - a cycle between statics is SEM007 `constant_cycle`;
//! - arithmetic overflow and division by zero are compile errors;
//! - a legacy top-level `fixum`/`varia` is SEM062 `top_level_binding`
//!   (D5.7: the library sweep landed, so the migration warning is now an
//!   error).

use crate::test_support::{analyze_source, assert_rejects_with_issue};
use radix_hir::{
    HirConst, HirExpression, HirExpressionKind, HirItemKind, HirLiteral, HirStatic, HirUnOp,
};

/// Analyze `source`, assert success, and return the named static's HIR const.
fn static_const(source: &str, name: &str) -> (HirConst, radix_lexer::Interner) {
    let fixture = analyze_source(source);
    assert!(fixture.is_ok(), "diagnostics: {:?}", fixture.diagnostics);
    let hir = fixture.result.hir.as_ref().expect("hir");
    let konst = hir
        .items
        .iter()
        .find_map(|item| match &item.kind {
            HirItemKind::Constant(konst) if fixture.interner.resolve(konst.name) == name => {
                Some(konst.clone())
            }
            _ => None,
        })
        .expect("static item");
    assert!(konst.statica.is_some(), "{name} should be a static");
    (konst, fixture.interner)
}

fn int_value(expr: &HirExpression) -> i128 {
    match &expr.kind {
        HirExpressionKind::Literal(HirLiteral::Int(v)) => i128::from(*v),
        HirExpressionKind::Unary(HirUnOp::Neg, inner) => match &inner.kind {
            HirExpressionKind::Literal(HirLiteral::Int(v)) => -i128::from(*v),
            other => panic!("expected negated int literal, got {other:?}"),
        },
        other => panic!("expected folded int literal, got {other:?}"),
    }
}

#[test]
fn arithmetic_initializer_folds_to_literal() {
    let (konst, _) = static_const("fixum numerus LIMES = 4 * 1024\n", "LIMES");
    assert_eq!(int_value(&konst.value), 4096);
}

#[test]
fn folding_keeps_the_authored_initializer_for_reemission() {
    let (konst, _) = static_const("fixum numerus LIMES = 4 * 1024\n", "LIMES");
    let Some(HirStatic {
        authored: Some(authored),
    }) = &konst.statica
    else {
        panic!("folded static keeps its authored initializer");
    };
    assert!(matches!(authored.kind, HirExpressionKind::Binary(..)));
}

#[test]
fn literal_initializer_is_not_rewritten() {
    let (konst, _) = static_const("fixum numerus N = 7\n", "N");
    assert_eq!(int_value(&konst.value), 7);
    assert_eq!(konst.statica, Some(HirStatic { authored: None }));
}

#[test]
fn negative_results_fold_to_negated_literals() {
    let (konst, _) = static_const("fixum numerus N = 3 - 10\n", "N");
    assert_eq!(int_value(&konst.value), -7);
}

// d11-6 ruling 14 supersedes the earlier truncation pin: `/` floors and `%`
// takes the divisor's sign.
#[test]
fn integer_division_and_remainder_floor() {
    let (div, _) = static_const("fixum numerus D = 7 / 2\n", "D");
    assert_eq!(int_value(&div.value), 3);
    let (neg, _) = static_const("fixum numerus D = (0 - 7) / 2\n", "D");
    assert_eq!(int_value(&neg.value), -4);
    let (rem, _) = static_const("fixum numerus R = (0 - 7) % 3\n", "R");
    assert_eq!(int_value(&rem.value), 2);
    let (rem, _) = static_const("fixum numerus R = 7 % (0 - 2)\n", "R");
    assert_eq!(int_value(&rem.value), -1);
}

#[test]
fn intermediates_are_exact_over_the_64_bit_range() {
    // Ruling 17a: `MAX * 2 + 1` passes through 2^64 - 2, which no `i64` holds.
    let source = "fixum u64 M = 9223372036854775807\nfixum u64 N = M * 2 + 1\n";
    let (konst, _) = static_const(source, "N");
    assert_eq!(int_value(&konst.value), i128::from(u64::MAX));
    let source = "fixum u64 M = 9223372036854775807\nfixum u64 N = M + 1\n";
    let (konst, _) = static_const(source, "N");
    assert_eq!(int_value(&konst.value), 1_i128 << 63);
}

#[test]
fn the_static_itself_must_fit_its_declared_type() {
    assert_rejects_with_issue(
        "fixum u8 A = 200\nfixum u8 B = A + 100\n",
        "constant_arithmetic_overflow",
    );
    assert_rejects_with_issue(
        "fixum u64 M = 18446744073709551615\nfixum u64 N = M + 1\n",
        "constant_arithmetic_overflow",
    );
}

#[test]
fn shifts_fold_as_pure_math() {
    let (shl, _) = static_const("fixum u64 S = 1 ⇐ 63\n", "S");
    assert_eq!(int_value(&shl.value), 1_i128 << 63);
    let (shr, _) = static_const("fixum numerus S = (0 - 5) ⇒ 1\n", "S");
    assert_eq!(int_value(&shr.value), -3);
    let (big, _) = static_const("fixum numerus S = 5 ⇒ 200\n", "S");
    assert_eq!(int_value(&big.value), 0);
    // 2^64 is a constant in an `=` position: refused at the store.
    assert_rejects_with_issue("fixum u64 S = 1 ⇐ 64\n", "store_constant_out_of_range");
}

#[test]
fn a_negative_shift_count_is_an_error_even_for_a_zero_operand() {
    // The runner rejects a negative count before looking at the value
    // (exact_int.rs NEGATIVE_SHIFT); a zero left operand must not fold to 0.
    // A literal count is refused earlier by the typechecker, so the count
    // reaches the fold through a constant reference.
    assert_rejects_with_issue(
        "fixum numerus N = 0 - 1\nfixum numerus S = 0 ⇐ N\n",
        "constant_arithmetic_overflow",
    );
    assert_rejects_with_issue(
        "fixum numerus N = 0 - 1\nfixum numerus S = 0 ⇒ N\n",
        "constant_arithmetic_overflow",
    );
}

#[test]
fn a_zero_operand_shifted_by_a_nonnegative_constant_folds_to_zero() {
    let (zero, _) = static_const("fixum numerus N = 3\nfixum numerus S = 0 ⇐ N\n", "S");
    assert_eq!(int_value(&zero.value), 0);
}

#[test]
fn references_fold_in_dependency_order() {
    // `B` is declared before the static it depends on.
    let source = "fixum numerus B = A * 2 + 1\nfixum numerus A = 4\n";
    let (konst, _) = static_const(source, "B");
    assert_eq!(int_value(&konst.value), 9);
}

#[test]
fn fractus_arithmetic_folds() {
    let (konst, _) = static_const("fixum fractus F = 1.5 * 2.0\n", "F");
    assert!(
        matches!(konst.value.kind, HirExpressionKind::Literal(HirLiteral::Float(v)) if (v - 3.0).abs() < f64::EPSILON),
        "got {:?}",
        konst.value.kind
    );
}

#[test]
fn comparison_folds_to_bivalens() {
    let (konst, _) = static_const(
        "fixum numerus A = 3\nfixum bivalens MAGNUS = A ≻ 2\n",
        "MAGNUS",
    );
    assert!(matches!(
        konst.value.kind,
        HirExpressionKind::Literal(HirLiteral::Bool(true))
    ));
}

#[test]
fn textus_concatenation_folds() {
    let (konst, interner) = static_const(
        "fixum textus PRAE = \"ab\"\nfixum textus NOMEN = PRAE + \"cd\"\n",
        "NOMEN",
    );
    let HirExpressionKind::Literal(HirLiteral::String(sym)) = konst.value.kind else {
        panic!("expected folded textus literal, got {:?}", konst.value.kind);
    };
    assert_eq!(interner.resolve(sym), "abcd");
}

#[test]
fn collection_literal_of_constants_is_accepted() {
    let (konst, _) = static_const(
        "fixum numerus N = 2\nfixum lista<numerus> XS = [1, N * 3, 4]\n",
        "XS",
    );
    let HirExpressionKind::Array(elements) = &konst.value.kind else {
        panic!("collection stays structural, got {:?}", konst.value.kind);
    };
    let radix_hir::HirArrayElement::Expr(second) = &elements[1] else {
        panic!("expected expression element");
    };
    assert_eq!(int_value(second), 6);
}

#[test]
fn call_initializer_is_not_constant() {
    assert_rejects_with_issue(
        "functio f() → numerus { redde 1 }\nfixum numerus N = f()\n",
        "constant_initializer_not_constant",
    );
}

#[test]
fn reference_to_top_level_fixum_rejects_the_fixum_itself() {
    // D5.7: the top-level `fixum` is illegal on its own now, so a static
    // that references it never reaches the "not constant" fold check —
    // the fixum's own top_level_binding error is what's reported.
    assert_rejects_with_issue(
        "sit numerus A ← 1\nfixum numerus N = A + 1\n",
        "top_level_binding",
    );
}

#[test]
fn constant_cycle_is_rejected() {
    assert_rejects_with_issue(
        "fixum numerus A = B + 1\nfixum numerus B = A + 1\n",
        "constant_cycle",
    );
}

#[test]
fn self_reference_is_a_cycle() {
    assert_rejects_with_issue("fixum numerus A = A + 1\n", "constant_cycle");
}

#[test]
fn overflow_is_rejected() {
    // A reference keeps the fold with the static pass: `MAX + 1` is not a
    // literal-only constant, so the checked fold owns the overflow.
    assert_rejects_with_issue(
        "fixum numerus MAX = 9223372036854775807\nfixum numerus N = MAX + 1\n",
        "constant_arithmetic_overflow",
    );
}

#[test]
fn literal_constant_that_exceeds_the_slot_is_a_store_error() {
    // d11-6 U4 (ruling 21, `=` rule): the exact value 2^63 is a valid
    // intermediate, but a constant in an `=` position must fit its slot, so
    // the literal form is refused at the store before the fold runs. This
    // replaces the old literal-form pin on `constant_arithmetic_overflow`.
    assert_rejects_with_issue(
        "fixum numerus N = 9223372036854775807 + 1\n",
        "store_constant_out_of_range",
    );
}

#[test]
fn division_by_zero_is_rejected() {
    assert_rejects_with_issue("fixum numerus N = 1 / 0\n", "constant_division_by_zero");
}

#[test]
fn statics_are_usable_from_functions() {
    let fixture = analyze_source(
        "fixum numerus LIMES = 4 * 1024\nfunctio f() → numerus { redde LIMES + 1 }\n",
    );
    assert!(fixture.is_ok(), "diagnostics: {:?}", fixture.diagnostics);
}

#[test]
fn statics_are_immutable() {
    assert_rejects_with_issue(
        "fixum numerus LIMES = 4\nfunctio f() → vacuum { LIMES ← 5 }\n",
        "assignment_to_constant",
    );
}

#[test]
fn top_level_fixum_rejects_with_migration_issue() {
    assert_rejects_with_issue("sit numerus N ← 4\n", "top_level_binding");
}

#[test]
fn top_level_varia_rejects_with_migration_issue() {
    assert_rejects_with_issue("varia numerus N ← 4\n", "top_level_binding");
}

#[test]
fn top_level_static_does_not_reject() {
    let fixture = analyze_source("fixum numerus N = 4\n");
    assert!(
        fixture.diagnostics.is_empty(),
        "diagnostics: {:?}",
        fixture.diagnostics
    );
}

#[test]
fn failed_dependency_declared_later_still_reports_its_own_error() {
    // `B` evaluates `A` first as a dependency; `A` must still carry its own
    // diagnostic when the loop reaches it.
    assert_rejects_with_issue(
        "functio f() → numerus { redde 1 }\nfixum numerus B = A + 1\nfixum numerus A = f()\n",
        "constant_initializer_not_constant",
    );
}

// ST2-1: imported constants fold in `=` initializers.

fn consts_interface(
    value: Option<crate::file_interface::InterfaceConstValue>,
) -> crate::file_interface::FileInterface {
    use crate::file_interface::{
        FileExport, FileExportKind, InterfaceCallable, InterfaceTypeSnapshot,
    };
    let mut interface = crate::file_interface::FileInterface::new();
    interface.insert(FileExport {
        name: "BASE".to_owned(),
        kind: FileExportKind::Const(
            InterfaceTypeSnapshot::Primitive(crate::Primitive::Numerus),
            value,
        ),
    });
    interface.insert(FileExport {
        name: "septem".to_owned(),
        kind: FileExportKind::Function(InterfaceCallable {
            type_params: Vec::new(),
            params: Vec::new(),
            ret: Box::new(InterfaceTypeSnapshot::Primitive(crate::Primitive::Numerus)),
            err: None,
            is_async: false,
            is_generator: false,
            tensor_edge_policy_requirements: Vec::new(),
            device: None,
        }),
    });
    interface
}

fn analyze_with_consts(
    source: &str,
    value: Option<crate::file_interface::InterfaceConstValue>,
) -> crate::test_support::AnalyzedFixture {
    use crate::passes::visibility_annotation::VisibilityTier::Publica;
    use std::collections::BTreeMap;
    crate::test_support::analyze_source_with_import_contract(
        source,
        BTreeMap::from([(
            "consts".to_owned(),
            BTreeMap::from([("BASE".to_owned(), Publica), ("septem".to_owned(), Publica)]),
        )]),
        BTreeMap::from([("consts".to_owned(), consts_interface(value))]),
    )
}

#[test]
fn imported_constant_with_folded_value_folds_in_initializer() {
    let fixture = analyze_with_consts(
        "importa ex \"./consts\" consts\nfixum numerus D = consts.BASE * 2\n",
        Some(crate::file_interface::InterfaceConstValue::Int(5)),
    );
    assert!(fixture.is_ok(), "diagnostics: {:?}", fixture.diagnostics);
    let hir = fixture.result.hir.as_ref().expect("hir");
    let konst = hir
        .items
        .iter()
        .find_map(|item| match &item.kind {
            HirItemKind::Constant(konst) if fixture.interner.resolve(konst.name) == "D" => {
                Some(konst)
            }
            _ => None,
        })
        .expect("D");
    assert_eq!(int_value(&konst.value), 10);
    assert!(
        konst.statica.as_ref().is_some_and(|s| s.authored.is_some()),
        "the authored initializer stays for re-emission"
    );
}

#[test]
fn imported_constant_folded_in_initializer_uses_the_import() {
    let fixture = analyze_with_consts(
        "importa ex \"./consts\" consts\nfixum numerus D = consts.BASE * 2\n",
        Some(crate::file_interface::InterfaceConstValue::Int(5)),
    );
    assert!(fixture.is_ok(), "diagnostics: {:?}", fixture.diagnostics);
    assert!(
        !fixture
            .diagnostics
            .iter()
            .any(|err| crate::test_support::issue(err) == Some("unused_import")),
        "diagnostics: {:?}",
        fixture.diagnostics
    );
}

#[test]
fn imported_constant_without_folded_value_stays_not_constant() {
    let fixture = analyze_with_consts(
        "importa ex \"./consts\" consts\nfixum numerus D = consts.BASE * 2\n",
        None,
    );
    assert!(!fixture.is_ok());
    assert!(
        fixture.diagnostics.iter().any(|err| err.is_error()
            && crate::test_support::issue(err) == Some("constant_initializer_not_constant")),
        "diagnostics: {:?}",
        fixture.diagnostics
    );
}

#[test]
fn imported_function_call_stays_not_constant() {
    let fixture = analyze_with_consts(
        "importa ex \"./consts\" consts\nfixum numerus G = consts.septem()\n",
        Some(crate::file_interface::InterfaceConstValue::Int(5)),
    );
    assert!(!fixture.is_ok());
    assert!(
        fixture.diagnostics.iter().any(|err| err.is_error()
            && crate::test_support::issue(err) == Some("constant_initializer_not_constant")),
        "diagnostics: {:?}",
        fixture.diagnostics
    );
}

#[test]
fn export_value_reads_folded_literals_by_declared_type() {
    use crate::file_interface::InterfaceConstValue;
    let (konst, interner) = static_const("fixum numerus LIMES = 4 * 1024\n", "LIMES");
    let fixture = analyze_source("fixum numerus LIMES = 4 * 1024\n");
    assert_eq!(
        super::statica::export_value(&konst.value, konst.ty, &fixture.types, &interner),
        Some(InterfaceConstValue::Int(4096))
    );
}
