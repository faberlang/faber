//! dec1 (ruling 22, D11.7): a `d64` place expression of any kind (list
//! element, genus field, tuple position, nested element) compares exactly
//! against an integer, a float or another decimal, and takes `≅ ≇ ≈ ≉` on the
//! true value rather than the raw scale-8 carrier.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, RunSourceError, run_source};

fn ok(body: &str) -> Vec<String> {
    let session = Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_latin_locale(),
    );
    let source = format!("genus Pt {{ varia d64 x }}\nincipit {{\n{body}\n}}\n");
    let mut host = BufferHost::default();
    match run_source(&session, "dec.fab", &source, &mut host) {
        Ok(()) => host.stdout_lines,
        Err(RunSourceError::Stepper(errors)) => panic!("{body}: {errors:?}"),
        Err(other) => panic!("{body}: {other:?}"),
    }
}

/// Six comparisons of `place` (holding 0.5) against constants and a float and
/// an integer variable, then the four value-equality glyphs.
fn probes(place: &str) -> String {
    format!(
        "varia fractus f ← 1.0\nvaria numerus i ← 1\n\
         nota {place} ≺ 1.0\nnota {place} ≺ 1\nnota {place} ≻ 0\nnota {place} ≺ f\nnota i ≻ {place}\n\
         nota {place} ≅ 0.5\nnota {place} ≇ 0.5\nnota {place} ≈ 0.5\nnota {place} ≉ 0.5\n\
         nota {place} ≅ i\nnota {place} ≈ f"
    )
}

const HALF: [&str; 11] = [
    "verum", "verum", "verum", "verum", "verum", "verum", "falsum", "verum", "falsum", "falsum", "falsum",
];

#[test]
fn scalar_control_compares_exactly() {
    assert_eq!(ok(&format!("varia d64 d ← 0.5\n{}", probes("d"))), HALF);
}

#[test]
fn list_element_compares_exactly() {
    assert_eq!(ok(&format!("varia lista<d64> xs ← [0.5]\n{}", probes("xs[0]"))), HALF);
}

#[test]
fn genus_field_compares_exactly() {
    assert_eq!(ok(&format!("varia Pt p ← Pt {{ x = 0.5 }}\n{}", probes("p.x"))), HALF);
}

#[test]
fn tuple_position_compares_exactly() {
    assert_eq!(
        ok(&format!(
            "varia d64 a ← 0.5\nvaria iuncta<d64, numerus> t ← iuncta<d64, numerus> [a, 1]\n{}",
            probes("t[0]")
        )),
        HALF
    );
}

#[test]
fn nested_element_compares_exactly() {
    assert_eq!(ok(&format!("varia lista<lista<d64>> g ← [[0.5]]\n{}", probes("g[0][0]"))), HALF);
}

#[test]
fn two_projected_decimals_compare_by_value() {
    assert_eq!(
        ok(
            "varia lista<d64> xs ← [0.5]\nvaria lista<d64> ys ← [1.5]\nnota xs[0] ≺ ys[0]\nnota ys[0] ≻ xs[0]\nnota xs[0] ≅ ys[0]\nnota xs[0] ≅ xs[0]"
        ),
        ["verum", "verum", "falsum", "verum"]
    );
}

#[test]
fn projected_decimal_beside_a_huge_integer_does_not_trap() {
    // Ruling 22: the comparison is exact, so no `d64` conversion of the integer.
    assert_eq!(
        ok(
            "varia lista<d64> xs ← [0.5]\nvaria numerus i ← 9223372036854775807\nnota xs[0] ≺ i\nnota i ≻ xs[0]\nnota xs[0] ≅ i"
        ),
        ["verum", "verum", "falsum"]
    );
    assert_eq!(
        ok("varia Pt p ← Pt { x = -0.5 }\nvaria u64 u ← 18446744073709551615\nnota p.x ≺ u\nnota u ≺ p.x"),
        ["verum", "falsum"]
    );
}

#[test]
fn projected_decimal_above_the_operand_is_false() {
    assert_eq!(
        ok(
            "varia lista<d64> xs ← [1.5]\nvaria fractus f ← 1.0\nvaria numerus i ← 1\nnota xs[0] ≺ f\nnota xs[0] ≺ i\nnota xs[0] ≻ f\nnota xs[0] ≻ i\nnota xs[0] ≅ f\nnota f ≅ xs[0]"
        ),
        ["falsum", "falsum", "verum", "verum", "falsum", "falsum"]
    );
}
