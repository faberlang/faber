//! K4: tensor elementwise lift, end to end through the MIR runner (the oracle
//! for these forms). Values, element-store traps, NaN and infinity, empty
//! tensors, ranks 1 to 3, both-sides-evaluated, and the fail-closed lanes.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, RunSourceError, run_source};

fn run(body: &str) -> (Vec<String>, Vec<(String, String)>) {
    let session = Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_latin_locale(),
    );
    let source = format!("{body}\n");
    let mut host = BufferHost::default();
    let errors = match run_source(&session, "lift.fab", &source, &mut host) {
        Ok(()) => Vec::new(),
        Err(RunSourceError::Stepper(errors)) => errors.into_iter().map(|e| (e.issue, e.message)).collect(),
        Err(other) => panic!("{body}: {other:?}"),
    };
    (host.stdout_lines, errors)
}

fn ok(body: &str) -> Vec<String> {
    let (out, errors) = run(body);
    assert!(errors.is_empty(), "{body}: {errors:?}");
    out
}

fn trap(body: &str) -> (Vec<String>, String, String) {
    let (out, errors) = run(body);
    let (issue, message) = errors
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("no trap: {body}"));
    (out, issue, message)
}

/// A program printing the flattened result of `expr` over the f32 tensors
/// `a = [1, 2, 3, 4]` and `b = [4, 3, 2, 1]` (shape `[2, 2]`).
fn f32_program(expr: &str) -> String {
    format!(
        "incipit {{
sit tensor<f32, [2, 2]> a ← [[1.0, 2.0], [3.0, 4.0]] ↦ tensor<f32, [2, 2]>
sit tensor<f32, [2, 2]> b ← [[4.0, 3.0], [2.0, 1.0]] ↦ tensor<f32, [2, 2]>
sit tensor<f32, [2, 2]> r ← {expr}
nota r.planata()
}}"
    )
}

fn f32_result(expr: &str) -> Vec<String> {
    ok(&f32_program(expr))
}

/// The same over the bivalens result of a comparison.
fn mask_result(expr: &str) -> Vec<String> {
    ok(&format!(
        "incipit {{
sit tensor<f32, [2, 2]> a ← [[1.0, 2.0], [3.0, 4.0]] ↦ tensor<f32, [2, 2]>
sit tensor<f32, [2, 2]> b ← [[4.0, 3.0], [2.0, 1.0]] ↦ tensor<f32, [2, 2]>
sit tensor<bivalens, [2, 2]> r ← {expr}
nota r.planata()
}}"
    ))
}

#[test]
fn plus_minus_and_negation_lift_against_a_scalar() {
    assert_eq!(f32_result("a + 1.0"), ["[2.0, 3.0, 4.0, 5.0]"]);
    assert_eq!(f32_result("1.0 + a"), ["[2.0, 3.0, 4.0, 5.0]"]);
    assert_eq!(f32_result("a - 1.0"), ["[0.0, 1.0, 2.0, 3.0]"]);
    assert_eq!(f32_result("1.0 - a"), ["[0.0, -1.0, -2.0, -3.0]"]);
    assert_eq!(f32_result("-a"), ["[-1.0, -2.0, -3.0, -4.0]"]);
}

#[test]
fn times_divide_and_remainder_lift_against_a_scalar() {
    assert_eq!(f32_result("a * 0.5"), ["[0.5, 1.0, 1.5, 2.0]"]);
    assert_eq!(f32_result("2.0 * a"), ["[2.0, 4.0, 6.0, 8.0]"]);
    assert_eq!(f32_result("a / 2.0"), ["[0.5, 1.0, 1.5, 2.0]"]);
    assert_eq!(f32_result("a % 2.0"), ["[1.0, 0.0, 1.0, 0.0]"]);
    // A constant adopts the element type: `a * 2` stays f32.
    assert_eq!(f32_result("a * 2"), ["[2.0, 4.0, 6.0, 8.0]"]);
}

#[test]
fn integer_division_and_remainder_floor_per_element() {
    let program = "incipit {
sit tensor<i32, [4]> a ← [7, -7, 9, -9] ↦ tensor<i32, [4]>
sit tensor<i32, [4]> q ← a / 2
sit tensor<i32, [4]> r ← a % 2
sit tensor<i32, [4]> s ← a % -2
nota q.planata()
nota r.planata()
nota s.planata()
}";
    assert_eq!(ok(program), ["[3, -4, 4, -5]", "[1, 1, 1, 1]", "[-1, -1, -1, -1]"]);
}

#[test]
fn a_zero_integer_divisor_traps_and_a_zero_float_divisor_is_ieee() {
    let (out, issue, message) = trap(
        "incipit {
sit tensor<i32, [2]> a ← [4, 8] ↦ tensor<i32, [2]>
nota \"a\"
sit tensor<i32, [2]> r ← a / 0
nota r.planata()
}",
    );
    assert_eq!(out, ["a"]);
    assert_eq!(issue, "runner_numerus_division_failed");
    assert!(message.contains("division failed"), "{message}");
    let program = "incipit {
sit tensor<f32, [3]> a ← [1.0, -1.0, 0.0] ↦ tensor<f32, [3]>
sit tensor<f32, [3]> r ← a / 0.0
nota r.planata()
}";
    assert_eq!(ok(program), ["[inf, -inf, NaN]"]);
}

#[test]
fn u8_elements_are_stored_per_element_and_trap_past_the_type() {
    let setup = "sit tensor<u8, [3]> a ← [200, 1, 100] ↦ tensor<u8, [3]>\n";
    // Fits: each element is stored under `u8`.
    let fits = format!("incipit {{\n{setup}sit tensor<u8, [3]> r ← a + 50\nnota r.planata()\n}}");
    assert_eq!(ok(&fits), ["[250, 51, 150]"]);
    // 200 + 100 leaves `u8`: the element store traps with its identity.
    let over = format!("incipit {{\n{setup}nota \"a\"\nsit tensor<u8, [3]> r ← a + 100\nnota r.planata()\n}}");
    let (out, issue, message) = trap(&over);
    assert_eq!(out, ["a"]);
    assert_eq!(issue, "implicit_store_out_of_range");
    assert!(message.contains("300 does not fit in `u8`"), "{message}");
    // Below zero: `1 - 2` does not fit an unsigned element.
    let under = format!("incipit {{\n{setup}sit tensor<u8, [3]> r ← a - 2\nnota r.planata()\n}}");
    let (_, issue, message) = trap(&under);
    assert_eq!(issue, "implicit_store_out_of_range");
    assert!(message.contains("-1 does not fit in `u8`"), "{message}");
    // Negation of an unsigned element is a store too.
    let neg = format!("incipit {{\n{setup}sit tensor<u8, [3]> r ← -a\nnota r.planata()\n}}");
    let (_, issue, _) = trap(&neg);
    assert_eq!(issue, "implicit_store_out_of_range");
}

#[test]
fn a_constant_beyond_the_element_widens_the_join() {
    // `a + 300` joins `u8` with the constant's range: elements are `u16`.
    let program = "incipit {
sit tensor<u8, [3]> a ← [200, 1, 100] ↦ tensor<u8, [3]>
sit tensor<u16, [3]> r ← a + 300
nota r.planata()
}";
    assert_eq!(ok(program), ["[500, 301, 400]"]);
}

#[test]
fn mixed_integer_widths_take_the_range_rule_between_tensors() {
    // Operator ruling 2026-10-15: `+` between two aggregates
    // takes the scalar range rule. `u8 + u16` is `[0, 65790]` → `u32`, while the
    // glyph max keeps the element join → `u16`; only the bound changes.
    let program = "incipit {
sit lista<u8> a_data ← [200, 100]
sit tensor<u8, [2]> a ← a_data ↦ tensor<u8, [2]>
sit lista<u16> b_data ← [1000, 2]
sit tensor<u16, [2]> b ← b_data ↦ tensor<u16, [2]>
sit tensor<u32, [2]> r ← a + b
nota r.planata()
sit tensor<u16, [2]> m ← a ⤒ b
nota m.planata()
}";
    assert_eq!(ok(program), ["[1200, 102]", "[1000, 100]"]);
}

#[test]
fn true_division_takes_ruling_23_widths_and_traps_an_integer_zero_divisor() {
    let program = "incipit {
sit tensor<u8, [3]> a ← [1, 2, 3] ↦ tensor<u8, [3]>
sit tensor<u8, [3]> b ← [2, 4, 4] ↦ tensor<u8, [3]>
sit tensor<f32, [3]> r ← a ÷ 2
sit tensor<f32, [3]> s ← a ÷ b
nota r.planata()
nota s.planata()
}";
    assert_eq!(ok(program), ["[0.5, 1.0, 1.5]", "[0.5, 0.5, 0.75]"]);
    let (_, issue, _) = trap(
        "incipit {
sit tensor<i32, [2]> a ← [1, 2] ↦ tensor<i32, [2]>
sit tensor<i32, [2]> z ← [1, 0] ↦ tensor<i32, [2]>
sit tensor<f64, [2]> r ← a ÷ z
nota r.planata()
}",
    );
    assert_eq!(issue, "runner_numerus_division_failed");
}

#[test]
fn comparisons_produce_a_bivalens_tensor() {
    assert_eq!(mask_result("a ≻ b"), ["[falsum, falsum, verum, verum]"]);
    assert_eq!(mask_result("a ≺ b"), ["[verum, verum, falsum, falsum]"]);
    assert_eq!(mask_result("a ≤ 2.0"), ["[verum, verum, falsum, falsum]"]);
    assert_eq!(mask_result("a ≥ b"), ["[falsum, falsum, verum, verum]"]);
    assert_eq!(mask_result("a ≅ a"), ["[verum, verum, verum, verum]"]);
    assert_eq!(mask_result("a ≅ b"), ["[falsum, falsum, falsum, falsum]"]);
    assert_eq!(mask_result("a ≇ b"), ["[verum, verum, verum, verum]"]);
    assert_eq!(mask_result("a ≅ 2.0"), ["[falsum, verum, falsum, falsum]"]);
}

/// `≡ ≢` compare whole tensors (advisor rulings 2026-10-04): one `bivalens`,
/// where the numeric comparisons give a mask.
fn whole_result(expr: &str) -> Vec<String> {
    ok(&format!(
        "incipit {{
sit tensor<f32, [2, 2]> a ← [[1.0, 2.0], [3.0, 4.0]] ↦ tensor<f32, [2, 2]>
sit tensor<f32, [2, 2]> b ← [[4.0, 3.0], [2.0, 1.0]] ↦ tensor<f32, [2, 2]>
sit tensor<f32, [2, 2]> c ← [[1.0, 2.0], [3.0, 4.0]] ↦ tensor<f32, [2, 2]>
sit bivalens r ← {expr}
nota r
}}"
    ))
}

#[test]
fn equality_compares_whole_tensors_to_one_verdict() {
    assert_eq!(whole_result("a ≡ c"), ["verum"]);
    assert_eq!(whole_result("a ≡ a"), ["verum"]);
    assert_eq!(whole_result("a ≡ b"), ["falsum"]);
    assert_eq!(whole_result("a ≢ b"), ["verum"]);
    assert_eq!(whole_result("a ≢ c"), ["falsum"]);
}

#[test]
fn whole_equality_differs_across_run_time_shapes_without_an_error() {
    let program = "functio extent(numerus n) → numerus { redde n }
incipit {
sit numerus n ← extent(2)
sit numerus m ← extent(3)
sit lista<f32> short_data ← [1.0, 2.0]
sit lista<f32> long_data ← [1.0, 2.0, 3.0]
sit tensor<f32, [magnitudo n]> short ← short_data ↦ tensor<f32, [magnitudo n]>
sit tensor<f32, [magnitudo m]> long ← long_data ↦ tensor<f32, [magnitudo m]>
nota short ≡ long
nota short ≢ long
nota short ≡ short
}";
    assert_eq!(ok(program), ["falsum", "verum", "verum"]);
}

#[test]
fn nan_compares_false_everywhere_and_unequal_to_itself() {
    let program = "incipit {
sit tensor<f32, [2]> z ← [0.0, 1.0] ↦ tensor<f32, [2]>
sit tensor<f32, [2]> n ← z ÷ z
sit tensor<bivalens, [2]> lt ← n ≺ n
sit tensor<bivalens, [2]> eq ← n ≅ n
sit tensor<bivalens, [2]> ne ← n ≇ n
sit bivalens same ← n ≡ n
sit bivalens apart ← n ≢ n
nota n.planata()
nota lt.planata()
nota eq.planata()
nota ne.planata()
nota same
nota apart
}";
    // Per element a NaN is unequal to itself; whole-value, a tensor holding
    // one is not `≡` to itself either (IEEE, no special case).
    assert_eq!(
        ok(program),
        [
            "[NaN, 1.0]",
            "[falsum, falsum]",
            "[falsum, verum]",
            "[verum, falsum]",
            "falsum",
            "verum"
        ]
    );
}

#[test]
fn logic_and_select_evaluate_both_sides() {
    let program = "functio noisy(tensor<bivalens, [2]> m, textus tag) → tensor<bivalens, [2]> {
    nota tag
    redde m
}
incipit {
sit lista<bivalens> yes_data ← [verum, verum]
sit tensor<bivalens, [2]> yes ← yes_data ↦ tensor<bivalens, [2]>
sit lista<bivalens> mix_data ← [verum, falsum]
sit tensor<bivalens, [2]> mix ← mix_data ↦ tensor<bivalens, [2]>
sit tensor<bivalens, [2]> r ← yes aut noisy(mix, \"rhs-or\")
nota r.planata()
sit tensor<bivalens, [2]> s ← non yes et noisy(mix, \"rhs-and\")
nota s.planata()
}";
    assert_eq!(ok(program), ["rhs-or", "[verum, verum]", "rhs-and", "[falsum, falsum]"]);
    let select = "functio pick(tensor<f32, [2]> v, textus tag) → tensor<f32, [2]> {
    nota tag
    redde v
}
incipit {
sit tensor<f32, [2]> a ← [1.0, 5.0] ↦ tensor<f32, [2]>
sit tensor<f32, [2]> b ← [9.0, 2.0] ↦ tensor<f32, [2]>
sit tensor<f32, [2]> r ← (a ≻ b) ✓ pick(a, \"then\") ✗ pick(b, \"else\")
nota r.planata()
}";
    assert_eq!(ok(select), ["then", "else", "[9.0, 5.0]"]);
}

#[test]
fn select_takes_scalar_branches() {
    assert_eq!(f32_result("(a ≻ 2.5) ✓ 1.0 ✗ 0.0"), ["[0.0, 0.0, 1.0, 1.0]"]);
    assert_eq!(f32_result("(a ≻ b) ✓ a ✗ 0.0"), ["[0.0, 0.0, 3.0, 4.0]"]);
}

#[test]
fn extrema_lift_tensor_against_tensor_and_scalar() {
    assert_eq!(f32_result("a ⤒ b"), ["[4.0, 3.0, 3.0, 4.0]"]);
    assert_eq!(f32_result("a ⤓ b"), ["[1.0, 2.0, 2.0, 1.0]"]);
    assert_eq!(f32_result("a ⤒ 2.5"), ["[2.5, 2.5, 3.0, 4.0]"]);
}

#[test]
fn vel_fills_the_nihil_elements() {
    let setup = "sit lista<numerus ∪ nihil> t_data ← [1, nulla, 3, nulla]
sit tensor<numerus ∪ nihil, [2, 2]> t ← t_data ↦ tensor<numerus ∪ nihil, [2, 2]>
sit lista<numerus> d_data ← [10, 20, 30, 40]
sit tensor<numerus, [2, 2]> d ← d_data ↦ tensor<numerus, [2, 2]>\n";
    let scalar = format!("incipit {{\n{setup}sit tensor<numerus, [2, 2]> r ← t vel 0\nnota r.planata()\n}}");
    assert_eq!(ok(&scalar), ["[1, 0, 3, 0]"]);
    let tensor = format!("incipit {{\n{setup}sit tensor<numerus, [2, 2]> r ← t vel d\nnota r.planata()\n}}");
    assert_eq!(ok(&tensor), ["[1, 20, 3, 40]"]);
}

#[test]
fn ranks_one_to_three_lift() {
    let rank3 = "incipit {
sit lista<f32> cube_data ← [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]
sit tensor<f32, [2, 2, 2]> a ← cube_data ↦ tensor<f32, [2, 2, 2]>
sit tensor<f32, [2, 2, 2]> r ← a * 2.0
sit tensor<bivalens, [2, 2, 2]> m ← a ≻ 4.5
nota r.planata()
nota m.planata()
}";
    assert_eq!(
        ok(rank3),
        [
            "[2.0, 4.0, 6.0, 8.0, 10.0, 12.0, 14.0, 16.0]",
            "[falsum, falsum, falsum, falsum, verum, verum, verum, verum]"
        ]
    );
    let rank1 = "incipit {
sit tensor<f32, [3]> a ← [1.0, 2.0, 3.0] ↦ tensor<f32, [3]>
sit tensor<f32, [3]> r ← a - 1.0
nota r.planata()
}";
    assert_eq!(ok(rank1), ["[0.0, 1.0, 2.0]"]);
}

#[test]
fn an_empty_tensor_lifts_to_an_empty_tensor() {
    let program = "incipit {
sit lista<f32> none ← []
sit tensor<f32, [0]> a ← none ↦ tensor<f32, [0]>
sit tensor<f32, [0]> r ← a + 1.0
sit tensor<bivalens, [0]> m ← a ≻ 0.0
nota r.planata()
nota m.planata()
}";
    assert_eq!(ok(program), ["[]", "[]"]);
}

#[test]
fn tensor_times_tensor_is_still_rejected_without_a_hint() {
    // K10 owns this decision; K4 pins the status quo.
    let session = Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_latin_locale(),
    );
    let source = "incipit {
sit tensor<f32, [2]> a ← [1.0, 2.0] ↦ tensor<f32, [2]>
sit tensor<f32, [2]> r ← a * a
nota r.planata()
}\n";
    let mut host = BufferHost::default();
    let result = run_source(&session, "lift.fab", source, &mut host);
    assert!(result.is_err(), "tensor * tensor must stay rejected");
}
