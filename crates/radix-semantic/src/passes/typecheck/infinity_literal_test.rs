//! IL-1 semantic pins for contextual non-finite literal typing.

use crate::test_support;
use crate::{NumericWidth, Primitive, Type};

#[test]
fn infinity_and_nan_literals_follow_contextual_float_width() {
    let unit = test_support::analyze_ok(
        r"incipit {
    sit f32 narrow ← ∞
    sit f64 wide ← ∞
    sit fractus default_width ← ∞
    sit f32 nan_width ← nonnumerus
    sit f32 negated ← -∞
    nota narrow
    nota wide
    nota default_width
    nota nan_width
    nota negated
}",
    );
    let tys = test_support::entry_local_tys(&unit);
    assert_eq!(tys.len(), 5);
    assert!(matches!(
        unit.types.get(tys[0]),
        Type::SizedNumeric(Primitive::Fractus, NumericWidth::F32)
    ));
    assert!(matches!(
        unit.types.get(tys[1]),
        Type::SizedNumeric(Primitive::Fractus, NumericWidth::F64)
    ));
    assert!(matches!(
        unit.types.get(tys[2]),
        Type::Primitive(Primitive::Fractus)
    ));
    assert!(matches!(
        unit.types.get(tys[3]),
        Type::SizedNumeric(Primitive::Fractus, NumericWidth::F32)
    ));
    assert!(matches!(
        unit.types.get(tys[4]),
        Type::SizedNumeric(Primitive::Fractus, NumericWidth::F32)
    ));
}

#[test]
fn non_finite_literals_fail_closed_in_numerus_contexts() {
    for source in [
        "incipit { sit numerus bad ← ∞ }",
        "incipit { sit numerus bad ← nonnumerus }",
        "incipit { sit fractus bad ← nan }",
    ] {
        let result = test_support::analyze_source(source);
        assert!(
            !result.success(),
            "numerus/wrong-locale source unexpectedly accepted: {source}"
        );
    }
}
