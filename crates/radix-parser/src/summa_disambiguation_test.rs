//! K3 parser pins: a `summa` operator word stays an ordinary identifier,
//! call, receiver, or intrinsic method outside the owned `reducta via` form.

use crate::test_support::{assert_parse_error_issue, parse_ok};

#[test]
fn summa_head_call_stays_ordinary() {
    let _ = parse_ok("functio addere(numerus a, numerus b) → numerus { summa(2, 3) }");
}

#[test]
fn non_head_summa_identifiers_stay_ordinary() {
    let _ = parse_ok("functio f() → numerus { sit numerus summa ← 2 redde summa }");
    // Known-good lambda spelling named `summa` (closure parameter position).
    let _ = parse_ok("sit summa ← (numerus a, numerus b) ∴ a + b");
    let _ = parse_ok("functio g(numerus summa) → numerus { redde summa }");
}

#[test]
fn legacy_reduction_heads_report_the_named_migration_issue() {
    for source in [
        "functio f(tensor<numerus, [4]> xs) → numerus { redde summa ex xs }",
        "functio f(tensor<numerus, [4]> xs) → numerus { redde maxima ex xs }",
        "functio f(tensor<numerus, [4]> xs) → numerus { redde minima ex xs vel 0 }",
    ] {
        assert_parse_error_issue(source, "reducta_legacy_head");
    }
}
