//! R0 red carrier contract: the MIR runner must preserve exact unsigned
//! 64-bit values (`Value::UInt`, `MapKey::UInt`) and execute every modular
//! width through one width-parametric vector table. These tests run against
//! the public `run_source` runner route; they compile now and fail until R1
//! (unsigned magnitude), R2 (unsigned runner carrier), and R3 (width
//! admission) land. The shipped u32 runner vectors in `runner_test.rs` are
//! the regression floor and are not duplicated here.

use super::*;
use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::run_source;

fn run_fixture_source(source: &str, host: &mut BufferHost) -> Result<(), Vec<RunnerError>> {
    let session = Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_latin_locale(),
    );
    run_source(&session, "test.fab", source, host).map_err(|error| match error {
        RunSourceError::Stepper(errors) => errors,
        RunSourceError::Frontend(diagnostics) => {
            panic!("fixture source should analyze cleanly: {diagnostics:?}")
        }
        RunSourceError::Mir(error) => {
            panic!("fixture source should lower cleanly: {error:?}")
        }
    })
}

#[test]
fn unsigned_integer_runner_preserves_i64_max_plus_one() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r"
incipit {
    sit u64 sha ← 9223372036854775808
    nota sha
}
",
        &mut host,
    )
    .expect("runner preserves i64::MAX + 1 as an exact unsigned value");
    assert_eq!(host.stdout_lines, vec!["9223372036854775808"]);
}

#[test]
fn unsigned_integer_runner_preserves_u64_max() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r"
incipit {
    sit u64 max ← 18446744073709551615
    nota max
    nota max ↦ textus
}
",
        &mut host,
    )
    .expect("runner preserves u64::MAX as an exact unsigned value");
    assert_eq!(host.stdout_lines, vec!["18446744073709551615", "18446744073709551615"]);
}

#[test]
fn unsigned_integer_runner_does_not_encode_upper_half_as_negative() {
    // Stop-condition proof: an upper-half u64 must never surface as a
    // negative signed bit pattern.
    let mut host = BufferHost::default();
    run_fixture_source(
        r"
incipit {
    sit u64 high ← 18446744073709551615
    nota high ≻ 9223372036854775807
}
",
        &mut host,
    )
    .expect("unsigned comparison runs");
    assert_eq!(host.stdout_lines, vec!["verum"]);
}

#[test]
fn unsigned_integer_runner_tabula_key_above_i64_max() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r#"
incipit {
    varia tabula<u64, textus> index ← vacua
    index.pone(18446744073709551615, "sha")
    index.pone(9223372036854775808, "round")
    nota index.accipe(18446744073709551615) vel "absens"
    nota index.accipe(9223372036854775808) vel "absens"
    nota index.longitudo()
}
"#,
        &mut host,
    )
    .expect("unsigned tabula keys above i64::MAX stay exact and distinct");
    assert_eq!(host.stdout_lines, vec!["sha", "round", "2"]);
}

#[test]
fn unsigned_integer_runner_copia_member_above_i64_max() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r"
incipit {
    varia copia<u64> words ← vacua
    words.adde(18446744073709551615)
    nota words.habet(18446744073709551615)
    nota words.habet(9223372036854775807)
}
",
        &mut host,
    )
    .expect("unsigned copia members above i64::MAX stay exact");
    assert_eq!(host.stdout_lines, vec!["verum", "falsum"]);
}

/// One row of the shared modular width edge-vector table. Expected values
/// derive from W: max = 2^W - 1, and every operation is reduced modulo 2^W.
struct ModularWidthVector {
    width: &'static str,
    shift: &'static str,
    max: &'static str,
    half: &'static str,
}

const MODULAR_WIDTH_VECTORS: &[ModularWidthVector] = &[
    ModularWidthVector { width: "u8", shift: "8", max: "255", half: "127" },
    ModularWidthVector { width: "u16", shift: "16", max: "65535", half: "32767" },
    ModularWidthVector { width: "u64", shift: "64", max: "18446744073709551615", half: "9223372036854775807" },
];

fn modular_width_vector_source(vector: &ModularWidthVector) -> String {
    let w = vector.width;
    let max = vector.max;
    let shift = vector.shift;
    format!(
        r"
incipit {{
    sit modulus<{w}> max ← {max}
    sit modulus<{w}> one ← 1
    sit modulus<{w}> zero ← 0
    sit modulus<{w}> two ← 2

    sit modulus<{w}> r0 ← max + one
    nota r0
    sit modulus<{w}> r1 ← zero - one
    nota r1
    sit modulus<{w}> r2 ← max * two
    nota r2
    sit modulus<{w}> r3 ← -one
    nota r3
    sit modulus<{w}> r4 ← ¬zero
    nota r4
    sit modulus<{w}> r5 ← one ⇐ {shift}
    nota r5
    nota max ⇒ {shift}
    nota max / two
    nota max % two
    nota max ≻ zero

    varia modulus<{w}> inc ← max
    inc ↑
    nota inc
    varia modulus<{w}> dec ← zero
    dec ↓
    nota dec

    sit modulus<{w}> t0 ← max.addita(one)
    nota t0
    nota max.subtrahe(one)
    sit modulus<{w}> t1 ← one.sinistrata({shift})
    nota t1
    nota max.dextrata({shift})

    sit modulus<{w}> roundTrip ← max ↦ {w} ↦ modulus<{w}>
    nota roundTrip
}}
"
    )
}

fn modular_width_vector_expected(vector: &ModularWidthVector) -> Vec<String> {
    let max = vector.max;
    let max_minus_one = match vector.width {
        "u8" => "254",
        "u16" => "65534",
        _ => "18446744073709551614",
    };
    vec![
        "0".to_owned(),           // max + 1 wraps
        max.to_owned(),           // zero - one wraps
        max_minus_one.to_owned(), // max * 2 wraps
        max.to_owned(),           // -one wraps
        max.to_owned(),           // ¬zero
        "0".to_owned(),           // one ⇐ W: the count is not masked (ruling 10)
        "0".to_owned(),           // max ⇒ W: every bit shifts out (ruling 10)
        vector.half.to_owned(),   // max / 2 unsigned
        "1".to_owned(),           // max % 2
        "verum".to_owned(),       // unsigned ordering
        "0".to_owned(),           // increment wraps
        max.to_owned(),           // decrement wraps
        "0".to_owned(),           // addita twin
        max_minus_one.to_owned(), // subtrahe twin
        "0".to_owned(),           // sinistrata twin (ruling 10)
        "0".to_owned(),           // dextrata twin (ruling 10)
        max.to_owned(),           // same-width conversion round trip
    ]
}

#[test]
fn mir_modular_word_runner_runs_u8_edge_vectors() {
    let vector = &MODULAR_WIDTH_VECTORS[0];
    let mut host = BufferHost::default();
    run_fixture_source(&modular_width_vector_source(vector), &mut host).expect("modulus<u8> edge vectors run");
    assert_eq!(host.stdout_lines, modular_width_vector_expected(vector));
}

#[test]
fn mir_modular_word_runner_runs_u16_edge_vectors() {
    let vector = &MODULAR_WIDTH_VECTORS[1];
    let mut host = BufferHost::default();
    run_fixture_source(&modular_width_vector_source(vector), &mut host).expect("modulus<u16> edge vectors run");
    assert_eq!(host.stdout_lines, modular_width_vector_expected(vector));
}

#[test]
fn mir_modular_word_runner_runs_u64_edge_vectors() {
    let vector = &MODULAR_WIDTH_VECTORS[2];
    let mut host = BufferHost::default();
    run_fixture_source(&modular_width_vector_source(vector), &mut host).expect("modulus<u64> edge vectors run");
    assert_eq!(host.stdout_lines, modular_width_vector_expected(vector));
}

#[test]
fn mir_modular_word_runner_u64_upper_half_sha_constants_stay_exact() {
    // SHA-512 K constants live in the upper half of the unsigned 64-bit range;
    // the runner must keep them exact through modular arithmetic.
    let mut host = BufferHost::default();
    run_fixture_source(
        r"
incipit {
    sit modulus<u64> k ← 15165310421466868936
    sit modulus<u64> w ← 18268729513060379306
    sit modulus<u64> sum ← k + w
    nota sum
    sit modulus<u64> back ← sum - w
    nota back
}
",
        &mut host,
    )
    .expect("upper-half u64 modular arithmetic stays exact");
    // (15165310421466868936 + 18268729513060379306) mod 2^64 = 14987295860817696626
    assert_eq!(host.stdout_lines, vec!["14987295860817696626", "15165310421466868936"]);
}

// Review-regression proofs: equality, collection keys, switch matching, and
// bitwise execution must be value-correct across the Int/UInt carriers. MIR
// lowering emits the unsigned carrier for unsigned-typed literals, and the
// runner treats non-negative Int and UInt with equal magnitude as equal.

#[test]
fn unsigned_integer_runner_mixed_carrier_equality_is_value_correct() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r"
incipit {
    sit modulus<u64> one ← 1
    sit modulus<u64> uno ← 1 ↦ modulus<u64>
    nota one ≡ uno
    nota one ≡ one + 0
    sit u64 wrapped ← 1
    nota wrapped ≡ (1 ↦ u64)
    nota wrapped ≡ wrapped + 0
}
",
        &mut host,
    )
    .expect("mixed-carrier equality compares numeric value");
    assert_eq!(host.stdout_lines, vec!["verum", "verum", "verum", "verum"]);
}

#[test]
fn unsigned_integer_runner_mixed_carrier_tabula_and_copia_keys() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r#"
incipit {
    sit u64 five ← 5
    sit u64 big ← 9223372036854775808
    sit u64 computed ← big - 9223372036854775808 + 5
    varia tabula<u64, textus> t ← vacua
    t.pone(five, "quinque")
    nota t.accipe(computed) vel "absens"
    nota t.accipe(five) vel "absens"
    varia copia<u64> s ← vacua
    s.adde(five)
    nota s.habet(computed)
    nota s.habet(big)
}
"#,
        &mut host,
    )
    .expect("mixed-carrier same-value keys collide");
    assert_eq!(host.stdout_lines, vec!["quinque", "quinque", "verum", "falsum"]);
}

#[test]
fn unsigned_integer_runner_mixed_carrier_elige_matches_by_value() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r#"
incipit {
    sit modulus<u64> one ← 1
    elige one + 0 {
        casu 1 { nota "primus" }
        ceterum { nota "aliud" }
    }
    sit u64 two ← 2
    elige two ↦ u64 {
        casu 2 { nota "secundus" }
        ceterum { nota "aliud" }
    }
}
"#,
        &mut host,
    )
    .expect("elige matches unsigned subjects against literal cases by value");
    assert_eq!(host.stdout_lines, vec!["primus", "secundus"]);
}

#[test]
fn unsigned_integer_runner_upper_half_bitwise_accepts_small_literals() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r"
incipit {
    sit u64 big ← 15165310421466868936
    nota big ∧ 255
    nota big ∨ 255
    nota big ⊻ 255
    nota big ⇒ 1
}
",
        &mut host,
    )
    .expect("upper-half u64 bitwise with small literals runs");
    assert_eq!(
        host.stdout_lines,
        vec![
            "200",
            "15165310421466868991",
            "15165310421466868791",
            "7582655210733434468"
        ]
    );
}

#[test]
fn unsigned_integer_runner_upper_half_left_shift_traps_past_the_range() {
    // d11-6 ruling 10 supersedes the wrapping `⇐` of a trapping `numerus<u64>`:
    // `x ⇐ 1` is `2x`, and 2x is past the 64-bit range (wrapping is `modulus`).
    let mut host = BufferHost::default();
    let errors = run_fixture_source(
        r"
incipit {
    sit u64 big ← 15165310421466868936
    nota big ⇐ 1
}
",
        &mut host,
    )
    .expect_err("2x exceeds 2^64 - 1");
    assert_eq!(errors[0].issue, "runner_numerus_overflow");
}

#[test]
fn unsigned_integer_runner_upper_half_sha512_k_and_shift_pattern() {
    // SHA-512-style: upper-half round constant combined with ∧ and ⇐ as in a
    // compression round.
    let mut host = BufferHost::default();
    run_fixture_source(
        r"
incipit {
    sit modulus<u64> k ← 15165310421466868936
    sit modulus<u64> w ← 7017280452245743616
    sit modulus<u64> masked ← (k ∧ 255) + (w ∧ 255)
    nota masked
    sit modulus<u64> rotated ← k ⇐ 1 ∨ k ⇒ 63
    nota rotated
}
",
        &mut host,
    )
    .expect("SHA-512-style K ∧/⇐ pattern runs in the runner");
    assert_eq!(host.stdout_lines, vec!["200", "11883876769224186257"]);
}

#[test]
fn unsigned_integer_runner_sorts_lista_with_upper_half_members() {
    // Review regression: `compare_sort_values` must order UInt carriers by
    // unsigned magnitude, not by signed bit pattern.
    let mut host = BufferHost::default();
    run_fixture_source(
        r"
incipit {
    varia lista<u64> xs ← [15165310421466868936, 3, 10, 2]
    xs.ordina()
    nota xs
}
",
        &mut host,
    )
    .expect("sort of lista<u64> with upper-half member runs");
    assert_eq!(host.stdout_lines, vec!["[2, 3, 10, 15165310421466868936]"]);
}

#[test]
fn unsigned_integer_runner_sorts_modular_lista_with_upper_half_members() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r"
incipit {
    varia lista<modulus<u64>> us ← [18446744073709551615, 3, 10, 2]
    us.ordina()
    nota us
    varia lista<modulus<u64>> small ← [3, 10, 2]
    small.ordina()
    nota small
}
",
        &mut host,
    )
    .expect("sort of lista<modulus<u64>> runs");
    assert_eq!(host.stdout_lines, vec!["[2, 3, 10, 18446744073709551615]", "[2, 3, 10]"]);
}

#[test]
fn unsigned_integer_runner_modular_collection_literals_validate() {
    // Review regression: MIR validation must accept integer literal constants
    // that fit the declared modular element/key/member type.
    let mut host = BufferHost::default();
    run_fixture_source(
        r#"
incipit {
    sit lista<modulus<u32>> ws ← [4294967295, 3, 10]
    nota ws.longitudo()
    varia tabula<modulus<u32>, textus> t ← vacua
    t.pone(4294967295, "max")
    t.pone(7, "septem")
    nota t.accipe(4294967295) vel "absens"
    nota t.longitudo()
    varia copia<modulus<u64>> c ← vacua
    c.adde(18446744073709551615)
    c.adde(3)
    nota c.habet(18446744073709551615)
    sit lista<modulus<u64>> big ← [18446744073709551615, 3, 10]
    nota big.longitudo()
}
"#,
        &mut host,
    )
    .expect("modular collection literals pass MIR validation");
    assert_eq!(host.stdout_lines, vec!["3", "max", "2", "verum", "3"]);
}

#[test]
fn unsigned_integer_runner_modular_collection_delete_accepts_fitting_constant() {
    // Review regression: `dele` must accept the same modular literal keys and
    // members that `pone`/`adde` accept. MIR constants carry plain `numerus`,
    // so the delete key/element check needs the fitting-constant relaxation
    // just like put/index/append/aggregates.
    let mut host = BufferHost::default();
    run_fixture_source(
        r#"
incipit {
    varia tabula<modulus<u32>, textus> t ← vacua
    t.pone(4294967295, "max")
    nota t.dele(4294967295)
    nota t.habet(4294967295)
    nota t.longitudo()
    varia copia<modulus<u64>> c ← vacua
    c.adde(18446744073709551615)
    nota c.dele(18446744073709551615)
    nota c.habet(18446744073709551615)
}
"#,
        &mut host,
    )
    .expect("modular collection delete by literal passes MIR validation and runs");
    assert_eq!(host.stdout_lines, vec!["verum", "falsum", "0", "verum", "falsum"]);
}
