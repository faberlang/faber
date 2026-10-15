//! Focused runner coverage for scalar integer `potentia`.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, RunSourceError, run_source};

fn session() -> Session {
    Session::new(Config::default().with_target(Target::HirTypeScript))
}

#[test]
fn integer_potentia_runner_handles_negative_base_and_zero_exponent() {
    let source = r"incipit {
    sit numerus base ← -2
    sit numerus odd ← base.potentia(3)
    sit numerus even ← base.potentia(4)
    sit numerus identity ← base.potentia(0)
    nota odd
    nota even
    nota identity
}";
    let mut host = BufferHost::default();
    match run_source(&session(), "potentia.fab", source, &mut host) {
        Ok(()) => assert_eq!(host.stdout_lines, vec!["-8", "16", "1"]),
        Err(RunSourceError::Frontend(errors)) => panic!("fixture frontend errors: {errors:?}"),
        Err(RunSourceError::Mir(errors)) => panic!("fixture MIR errors: {errors:?}"),
        Err(RunSourceError::Stepper(errors)) => panic!("fixture runner errors: {errors:?}"),
    }
}

fn run_lines(name: &str, source: &str) -> Vec<String> {
    let mut host = BufferHost::default();
    match run_source(&session(), name, source, &mut host) {
        Ok(()) => host.stdout_lines,
        Err(RunSourceError::Frontend(errors)) => panic!("fixture frontend errors: {errors:?}"),
        Err(RunSourceError::Mir(errors)) => panic!("fixture MIR errors: {errors:?}"),
        Err(RunSourceError::Stepper(errors)) => panic!("fixture runner errors: {errors:?}"),
    }
}

/// IEEE 754 `pow` results for fractus `potentia` (operator ruling 2026-09-24):
/// negative bases with integral exponents keep their sign, non-integral
/// exponents of negative bases are NaN, and zero bases follow the pole rules.
#[test]
fn fractus_potentia_runner_follows_ieee_pow_for_negative_and_zero_bases() {
    let source = r"incipit {
    sit fractus neg ← -2.0
    sit fractus zero ← 0.0
    sit fractus negzero ← -0.0
    nota neg.potentia(2.0)
    nota neg.potentia(3.0)
    nota neg.potentia(0.5)
    nota neg.potentia(-1.0)
    nota zero.potentia(-1.0)
    nota zero.potentia(2.0)
    nota negzero.potentia(-3.0)
    nota negzero.potentia(3.0)
}";
    assert_eq!(
        run_lines("potentia-ieee-sign.fab", source),
        vec!["4.0", "-8.0", "NaN", "-0.5", "inf", "0.0", "-inf", "-0.0"]
    );
}

/// Integral exponents are exact where the result is representable.
#[test]
fn fractus_potentia_runner_is_exact_for_integral_exponents() {
    let source = r"incipit {
    sit fractus two ← 2.0
    sit fractus ten ← 10.0
    sit fractus three ← 3.0
    nota two.potentia(3.0)
    nota two.potentia(-2.0)
    nota ten.potentia(2.0)
    nota three.potentia(20.0)
    nota two.potentia(0.5)
}";
    assert_eq!(
        run_lines("potentia-ieee-exact.fab", source),
        vec!["8.0", "0.25", "100.0", "3486784401.0", "1.4142135623730951"]
    );
}

/// `x^0 = 1` and `1^y = 1` even for NaN; otherwise NaN propagates; infinite
/// exponents and bases follow the IEEE magnitude rules; overflow is `∞`.
#[test]
fn fractus_potentia_runner_follows_ieee_pow_for_nan_and_infinity() {
    let source = r"incipit {
    sit fractus n ← nonnumerus
    sit fractus one ← 1.0
    sit fractus minus_one ← -1.0
    sit fractus two ← 2.0
    sit fractus half ← 0.5
    sit fractus ten ← 10.0
    sit fractus neg_inf ← -∞
    nota n.potentia(0.0)
    nota one.potentia(nonnumerus)
    nota two.potentia(nonnumerus)
    nota n.potentia(2.0)
    nota minus_one.potentia(∞)
    nota two.potentia(∞)
    nota two.potentia(-∞)
    nota half.potentia(∞)
    nota neg_inf.potentia(3.0)
    nota neg_inf.potentia(-3.0)
    nota neg_inf.potentia(2.0)
    nota ten.potentia(400.0)
    nota ten.potentia(-400.0)
    nota two.potentia(1e308)
}";
    assert_eq!(
        run_lines("potentia-ieee-nonfinite.fab", source),
        vec![
            "1.0", "1.0", "NaN", "NaN", "1.0", "inf", "0.0", "0.0", "-inf", "-0.0", "inf", "inf", "0.0", "inf"
        ]
    );
}
