//! Integration tests for the MIR runner (target-neutral interpreter).
//!
//! These tests prove that the runner executes corpus exemplars,
//! reports numeric overflow and division errors at the correct width
//! boundary, runs tensor construction and indexed access, evaluates
//! matrix and vector operations, handles instans conversio, and
//! propagates failable runtime errors through try_call and fac/cape.

use super::*;
use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::run_source;
use std::fs;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;

fn run_fixture_source(source: &str, host: &mut BufferHost) -> Result<(), Vec<RunnerError>> {
    let session = Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_dev_stdlib(),
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

fn run_exemplum(path: &Path, host: &mut BufferHost) -> Result<(), Vec<RunnerError>> {
    let source = fs::read_to_string(path).expect("read exemplum source");
    run_fixture_source(&source, host)
}

fn expected_lines(path: &Path) -> Vec<String> {
    fs::read_to_string(path.with_extension("expected"))
        .expect("read exemplum expected")
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn runner_runs_module_nota_for_salve_munde() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/incipit/salve-munde.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs salve-munde");
    assert_eq!(host.stdout_lines, expected_lines(&path));
}

#[test]
fn runner_runs_numeric_operator_methods_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/intrinseca/numeric-operator-methods.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs numeric operator methods fixture");
    assert_eq!(host.stdout_lines, vec!["9.0".to_owned(), "15.0".to_owned(), "8".to_owned()]);
}

#[test]
fn mir_modular_word_runner_runs_corpus_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/operatores/modular-word.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs modular word corpus fixture");
    assert_eq!(host.stdout_lines, expected_lines(&path));
}

#[test]
fn mir_modular_word_runner_runs_sha_round_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/operatores/modular-word-sha-round.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs modular word SHA round fixture");
    assert_eq!(host.stdout_lines, expected_lines(&path));
}

#[test]
fn mir_modular_word_runner_runs_explicit_conversion_in_both_directions() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r"
typus Verbum = modulus<u32>

incipit {
    fixum numerus<u32> checked ← 4294967295
    fixum modulus<u32> word ← checked ↦ modulus<u32>
    fixum numerus<u32> roundTrip ← word ↦ numerus<u32>
    fixum Verbum aliased ← checked ↦ Verbum
    nota word + 1
    nota roundTrip
    nota aliased + 1
}
",
        &mut host,
    )
    .expect("runner runs explicit modular word conversion");
    assert_eq!(host.stdout_lines, vec!["0", "4294967295", "0"]);
}

#[test]
fn runner_runs_fractus_f32_remainder() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r"
incipit {
    fixum fractus<f32> value ← 5.5 % 2.0
    nota value
}
",
        &mut host,
    )
    .expect("runner runs fractus<f32> remainder");
    assert_eq!(host.stdout_lines, vec!["1.5".to_owned()]);
}

#[test]
fn runner_concatenates_textus_addition() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r#"
incipit {
    fixum textus greeting ← "salve" + " munde"
    nota greeting
}
"#,
        &mut host,
    )
    .expect("runner runs textus addition");
    assert_eq!(host.stdout_lines, vec!["salve munde".to_owned()]);
}

#[test]
fn runner_concatenates_empty_textus() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r#"
incipit {
    fixum textus empty ← ""
    fixum textus greeting ← empty + " munde"
    nota greeting
}
"#,
        &mut host,
    )
    .expect("runner concatenates empty + non-empty textus");
    assert_eq!(host.stdout_lines, vec![" munde".to_owned()]);
}

#[test]
fn runner_reports_numerus_overflow() {
    let mut host = BufferHost::default();
    let errors = run_fixture_source(
        r"
incipit {
    fixum numerus max ← 9223372036854775807
    nota max + 1
}
",
        &mut host,
    )
    .expect_err("overflowing numerus addition should fail");
    assert!(
        errors
            .iter()
            .any(|error| error.issue == "runner_numerus_overflow"),
        "expected numerus overflow error, got {errors:?}"
    );
}

#[test]
fn mir_modular_word_add_wraps_on_overflow() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r"
incipit {
    fixum modulus<u32> max ← 4294967295
    fixum modulus<u32> one ← 1
    nota max + one
}
",
        &mut host,
    )
    .expect("modular word max + 1 wraps to 0");
    assert_eq!(host.stdout_lines, vec!["0"]);
}

#[test]
fn mir_modular_word_sub_wraps_on_underflow() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r"
incipit {
    fixum modulus<u32> one ← 1
    fixum modulus<u32> zero ← 0
    nota zero - one
}
",
        &mut host,
    )
    .expect("modular word 0 - 1 wraps to max");
    assert_eq!(host.stdout_lines, vec!["4294967295"]);
}

#[test]
fn mir_modular_word_mul_wraps_on_overflow() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r"
incipit {
    fixum modulus<u32> max ← 4294967295
    nota max * 2
}
",
        &mut host,
    )
    .expect("modular word max * 2 wraps");
    assert_eq!(host.stdout_lines, vec!["4294967294"]);
}

#[test]
fn mir_modular_word_negation_wraps() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r"
incipit {
    fixum modulus<u32> one ← 1
    nota -one
}
",
        &mut host,
    )
    .expect("modular word negation of 1 wraps to max");
    assert_eq!(host.stdout_lines, vec!["4294967295"]);
}

#[test]
fn mir_modular_word_bitwise_not_wraps() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r"
incipit {
    fixum modulus<u32> zero ← 0
    nota ¬zero
}
",
        &mut host,
    )
    .expect("modular word bitwise not of 0 yields all ones");
    assert_eq!(host.stdout_lines, vec!["4294967295"]);
}

#[test]
fn mir_modular_word_shift_left_by_width_wraps() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r"
incipit {
    fixum modulus<u32> one ← 1
    nota one ⇐ 32
}
",
        &mut host,
    )
    .expect("modular word shift left by 32 wraps to 1");
    assert_eq!(host.stdout_lines, vec!["1"]);
}

#[test]
fn mir_modular_word_shift_right_by_width_wraps() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r"
incipit {
    fixum modulus<u32> max ← 4294967295
    nota max ⇒ 32
}
",
        &mut host,
    )
    .expect("modular word shift right by 32 wraps");
    assert_eq!(host.stdout_lines, vec!["4294967295"]);
}

#[test]
fn mir_modular_word_greater_than_compares_wrapped_value() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r"
incipit {
    fixum modulus<u32> max ← 4294967295
    fixum modulus<u32> zero ← 0
    nota max > zero
}
",
        &mut host,
    )
    .expect("modular word max > 0 is verum");
    assert_eq!(host.stdout_lines, vec!["verum"]);
}

#[test]
fn mir_modular_word_runner_rejects_division_by_zero() {
    let mut host = BufferHost::default();
    let errors = run_fixture_source(
        r"
incipit {
    fixum modulus<u32> one ← 1
    fixum modulus<u32> zero ← 0
    nota one / zero
}
",
        &mut host,
    )
    .expect_err("modular division by zero should fail");
    assert!(
        errors
            .iter()
            .any(|error| error.issue == "modular_division_by_zero"),
        "expected modular division failure, got {errors:?}"
    );
}

#[test]
fn runner_reports_numerus_negation_overflow() {
    // FAO-008: numerus negation overflow must fail with a Faber-level RunnerError,
    // not a Rust-style unwrap panic. The runner owns this semantic via width-checked
    // negation in `eval_unop`. Construct i64::MIN via subtraction so the negation
    // itself is the only overflowing step (a literal `MAX + 1` would overflow at
    // the add and never reach negation, as the prior version of this test did).
    let mut host = BufferHost::default();
    let errors = run_fixture_source(
        r"
incipit {
    fixum numerus max ← 9223372036854775807
    fixum numerus min ← 0 - max - 1
    nota -min
}
",
        &mut host,
    )
    .expect_err("negation of MIN numerus must fail");
    assert!(
        errors.iter().any(|e| e.issue == "runner_numerus_overflow"),
        "expected numerus overflow on negation, got {errors:?}"
    );
}

#[test]
fn runner_reports_width_specific_addition_overflow() {
    // FAO-008 (width fidelity): the runner is the target-neutral numeric
    // authority, so `numerus<i8>` overflow must be caught at the i8 boundary —
    // not silently computed in i64. `127 + 127 = 254` fits i64 but overflows
    // i8. The old i64-only runner accepted this; the width-aware runner must
    // reject it, matching `i8::checked_add` in Rust codegen.
    let mut host = BufferHost::default();
    let errors = run_fixture_source(
        r"
incipit {
    fixum numerus<i8> a ← 127
    fixum numerus<i8> b ← 127
    fixum numerus<i8> sum ← a + b
    nota sum
}
",
        &mut host,
    )
    .expect_err("numerus<i8> 127 + 127 must overflow i8");
    assert!(
        errors.iter().any(|e| e.issue == "runner_numerus_overflow"),
        "expected numerus overflow for i8 addition, got {errors:?}"
    );
}

#[test]
fn runner_reports_width_specific_negation_overflow() {
    // FAO-008 (width fidelity): negation of i8::MIN overflows i8 (|-128| = 128
    // > 127) even though it fits i64. Reach MIN via subtraction so no
    // intermediate step overflows: `0 - 127 - 1 = -128`.
    let mut host = BufferHost::default();
    let errors = run_fixture_source(
        r"
incipit {
    fixum numerus<i8> max ← 127
    fixum numerus<i8> min ← 0 - max - 1
    nota -min
}
",
        &mut host,
    )
    .expect_err("negation of i8 MIN must overflow i8");
    assert!(
        errors.iter().any(|e| e.issue == "runner_numerus_overflow"),
        "expected numerus overflow for i8 negation, got {errors:?}"
    );
}

#[test]
fn runner_reports_numerus_division_failure() {
    let mut host = BufferHost::default();
    let errors = run_fixture_source(
        r"
incipit {
    nota 10 / 0
}
",
        &mut host,
    )
    .expect_err("integer division by zero should fail");
    assert!(
        errors
            .iter()
            .any(|error| error.issue == "runner_numerus_division_failed"),
        "expected numerus division failure, got {errors:?}"
    );
}

#[test]
fn runner_normalizes_numerus_shift_width() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r"
incipit {
    nota 1 ⇐ 65
}
",
        &mut host,
    )
    .expect("wide shift count should be normalized");
    assert_eq!(host.stdout_lines, vec!["2".to_owned()]);
}

#[test]
fn runner_shifts_numerus_by_zero() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r"
incipit {
    nota 1 ⇐ 0
}
",
        &mut host,
    )
    .expect("shift by zero is identity");
    assert_eq!(host.stdout_lines, vec!["1".to_owned()]);
}

#[test]
fn runner_shifts_numerus_by_exact_width() {
    // Shift by 64 on i64 is 64 % 64 = 0, so identity.
    let mut host = BufferHost::default();
    run_fixture_source(
        r"
incipit {
    nota 1 ⇐ 64
}
",
        &mut host,
    )
    .expect("shift by exact width is identity");
    assert_eq!(host.stdout_lines, vec!["1".to_owned()]);
}

#[test]
fn runner_runs_assignatio_fixture_with_textus_addition() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/assignatio/assignatio.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs assignatio fixture");
    assert_eq!(
        host.stdout_lines,
        vec![
            "20".to_owned(),
            "25".to_owned(),
            "15".to_owned(),
            "30".to_owned(),
            "10".to_owned(),
            "salve munde".to_owned(),
        ]
    );
}

#[test]
fn runner_runs_format_string_template() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r#"
incipit {
    nota "Salve, §!"("Munde")
}
"#,
        &mut host,
    )
    .expect("runner runs format-string fixture");
    assert_eq!(host.stdout_lines, vec!["Salve, Munde!".to_owned()]);
}

#[test]
fn runner_runs_format_string_empty_template() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r#"
incipit {
    nota ""()
}
"#,
        &mut host,
    )
    .expect("runner runs empty format-string fixture");
    assert_eq!(host.stdout_lines, vec![String::new()]);
}

#[test]
fn runner_converts_textus_to_instans_seconds() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r#"
incipit {
    fixum valor utc ← "1979-05-27T07:32:00.123456Z"
    fixum instans seconds ← utc ↦ instans
    adfirma seconds ↦ textus ≡ "1979-05-27T07:32:00Z"
    nota seconds ↦ textus
}
"#,
        &mut host,
    )
    .expect("runner converts textus to instans seconds");
    assert_eq!(host.stdout_lines, vec!["1979-05-27T07:32:00Z".to_owned()]);
}

#[test]
fn runner_converts_instans_to_millis_round_trip() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r#"
incipit {
    fixum valor utc ← "1979-05-27T07:32:00.123456Z"
    fixum instans seconds ← utc ↦ instans
    fixum instans<ms> millis ← seconds ↦ instans<ms>
    adfirma millis ↦ textus ≡ "1979-05-27T07:32:00.000Z"
}
"#,
        &mut host,
    )
    .expect("runner converts instans to millis");
}

#[test]
fn runner_converts_textus_to_instans_micros_round_trip() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r#"
incipit {
    fixum valor utc ← "1979-05-27T07:32:00.123456Z"
    fixum instans<us> micros ← utc ↦ instans<us>
    adfirma micros ↦ textus ≡ "1979-05-27T07:32:00.123456Z"
}
"#,
        &mut host,
    )
    .expect("runner converts textus to instans<us>");
}

#[test]
fn runner_converts_textus_to_instans_nanos_round_trip() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r#"
incipit {
    fixum valor utc ← "1979-05-27T07:32:00.123456Z"
    fixum instans<ns> nanos ← utc ↦ instans<ns>
    adfirma nanos ↦ textus ≡ "1979-05-27T07:32:00.123456000Z"
}
"#,
        &mut host,
    )
    .expect("runner converts textus to instans<ns>");
}

#[test]
fn runner_narrows_instans_ns_to_seconds() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r#"
incipit {
    fixum valor utc ← "1979-05-27T07:32:00.123456Z"
    fixum instans<ns> nanos ← utc ↦ instans<ns>
    fixum instans narrowed ← nanos ↦ instans
    adfirma narrowed ↦ textus ≡ "1979-05-27T07:32:00Z"
}
"#,
        &mut host,
    )
    .expect("runner narrows instans<ns> to instans seconds");
}

#[test]
fn runner_recovery_instans_conversio_with_fallback() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r#"
incipit {
    fixum valor utc ← "1979-05-27T07:32:00.123456Z"
    fixum instans seconds ← utc ↦ instans
    fixum valor bad ← "not-a-datetime"
    fixum instans recovered ← bad ↦ instans ⇥ seconds
    adfirma recovered ≡ seconds
}
"#,
        &mut host,
    )
    .expect("runner recovers instans conversio with fallback");
}

#[test]
fn runner_rejects_direct_json_literal_to_textus_conversion() {
    let session = Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_dev_stdlib(),
    );
    let source = r#"
incipit {
    fixum json documentum ← {"id": 1, "name": "Ada"}
    fixum textus wire ← documentum ↦ textus
    nota wire
}
"#;
    let Err(diagnostics) = crate::driver::analyze_source(&session, "test.fab", source) else {
        panic!("json to textus conversion should be unsupported");
    };
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic
            .args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == "unsupported_conversio")),
        "expected unsupported_conversio diagnostic, got {diagnostics:?}"
    );
}

#[test]
fn runner_runs_instans_conversio_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/conversio/instans.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs instans conversio fixture");
    assert_eq!(
        host.stdout_lines,
        vec![
            "1979-05-27T07:32:00Z".to_owned(),
            "1979-05-27T07:32:00.000Z".to_owned(),
            "1979-05-27T07:32:00.123456Z".to_owned(),
            "1979-05-27T07:32:00.123456000Z".to_owned(),
            "1979-05-27T07:32:00Z".to_owned(),
        ]
    );
    assert!(host.stderr_lines.is_empty());
}

#[test]
fn runner_propagates_failable_runtime_conversio_through_try_call() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r#"
functio parseInstans(valor v) → instans ⇥ textus {
    redde v ↦ instans
}

functio tutum(valor v) → instans {
    fixum instans epoch ← "1970-01-01T00:00:00Z" ↦ instans
    fac {
        redde parseInstans(v)
    } cape err {
        mone err
        redde epoch
    }
}

incipit {
    fixum valor good ← "1979-05-27T07:32:00Z"
    fixum valor bad ← "not-a-datetime"

    adfirma tutum(good) ≡ good ↦ instans
    adfirma tutum(bad) ≡ "1970-01-01T00:00:00Z" ↦ instans

    nota tutum(good) ↦ textus
}
"#,
        &mut host,
    )
    .expect("runner propagates failable conversio through try_call");
    assert_eq!(host.stdout_lines, vec!["1979-05-27T07:32:00Z".to_owned()]);
    assert_eq!(host.stderr_lines, vec!["valor to instans conversion failed".to_owned()]);
}

#[test]
fn runner_handles_direct_conversio_inside_fac_cape() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r#"
functio tutumDirect(valor v) → instans {
    fixum instans epoch ← "1970-01-01T00:00:00Z" ↦ instans
    fac {
        redde v ↦ instans
    } cape err {
        mone err
        redde epoch
    }
}

incipit {
    fixum valor good ← "1979-05-27T07:32:00Z"
    fixum valor bad ← "not-a-datetime"

    adfirma tutumDirect(good) ≡ good ↦ instans
    adfirma tutumDirect(bad) ≡ "1970-01-01T00:00:00Z" ↦ instans

    nota tutumDirect(good) ↦ textus
}
"#,
        &mut host,
    )
    .expect("runner handles direct conversio inside fac/cape");
    assert_eq!(host.stdout_lines, vec!["1979-05-27T07:32:00Z".to_owned()]);
    assert_eq!(host.stderr_lines, vec!["valor to instans conversion failed".to_owned()]);
}

#[test]
fn runner_propagates_failable_function_call_through_alternate_exit() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/iace/functio-propagans.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs failable call propagation fixture");
    assert_eq!(host.stdout_lines, vec!["4".to_owned(), "0".to_owned()]);
    assert_eq!(host.stderr_lines, vec!["division by zero".to_owned()]);
}

#[test]
fn runner_runs_collection_conversio_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/conversio/collectiones.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs collection conversio fixture");
    assert_eq!(
        host.stdout_lines,
        vec![
            "3".to_owned(),
            "3".to_owned(),
            "3".to_owned(),
            "2".to_owned(),
            "2".to_owned(),
            "3".to_owned()
        ]
    );
    assert!(host.stderr_lines.is_empty());
}

#[test]
fn runner_runs_conversio_assign_corpus_fixture() {
    // CDA-U8 — the `↤` corpus exemplum (assignment, typed init, recovery,
    // chain) executes through the shared `↦` route with the expected values.
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/assignatio/conversio-assign.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs conversio-assign corpus fixture");
    assert_eq!(
        host.stdout_lines,
        vec![
            "42".to_owned(),
            "7".to_owned(),
            "0".to_owned(),
            "42".to_owned(),
            "42".to_owned()
        ]
    );
    assert!(host.stderr_lines.is_empty());
}

#[test]
fn runner_runs_octeti_conversio_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/conversio/octeti.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs octeti conversio fixture");
    assert_eq!(
        host.stdout_lines,
        vec![
            "2".to_owned(),
            "hi".to_owned(),
            "?".to_owned(),
            "2".to_owned(),
            "hi".to_owned(),
            "x".to_owned()
        ]
    );
    assert!(host.stderr_lines.is_empty());
}

#[test]
fn runner_runs_octeti_unify_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/octeti/unify.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs octeti unify fixture");
    assert_eq!(
        host.stdout_lines,
        vec!["4".to_owned(), "222".to_owned(), "5".to_owned(), "2".to_owned()]
    );
    assert!(host.stderr_lines.is_empty());
}

#[test]
fn runner_runs_sparse_representation_fixtures() {
    // L23 (d57baa50): `decl`'s all-numerus `nota` space-joins into ONE line.
    // L28 (ab91f49f): `access`/`sugar` carry f32 fractus args, which now route
    // through the shared f32→textus render (FormatString f32 display carrier),
    // so their multi-arg notas space-join like the HIR-Rust lane too.
    for (fixture, expected) in [
        ("access", vec!["4.0 0.0 1"]),
        ("decl", vec!["2 0"]),
        ("sugar", vec!["4.0 0.0 1 2"]),
    ] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/sparsa")
            .join(format!("{fixture}.fab"));
        let mut host = BufferHost::default();
        run_exemplum(&path, &mut host).unwrap_or_else(|errors| panic!("runner runs {fixture}: {errors:?}"));
        let expected = expected.into_iter().map(str::to_owned).collect::<Vec<_>>();
        assert_eq!(host.stdout_lines, expected, "fixture {fixture}");
    }
}

#[test]
fn runner_runs_stage3_mir_closeout_fixtures() {
    for fixture in [
        "intervallum/algebra.fab",
        "intervallum/conversio.fab",
        "ad/sermo-tuus.fab",
        "ad/sermo-live-directional.fab",
    ] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus")
            .join(fixture);
        let mut host = BufferHost::default();
        run_exemplum(&path, &mut host).unwrap_or_else(|errors| panic!("runner runs {fixture}: {errors:?}"));
    }
}

#[test]
fn runner_runs_tensor_decl_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/tensor/decl.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs tensor decl fixture");
    assert_eq!(host.stdout_lines, vec!["0".to_owned()]);
    assert!(host.stderr_lines.is_empty());
}

#[test]
fn runner_runs_valor_tensor_conversio_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/conversio/valor-tensor.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs valor tensor conversio fixture");
    assert_eq!(host.stdout_lines, expected_lines(&path));
    assert!(host.stderr_lines.is_empty());
}

#[test]
fn runner_runs_shaped_lista_tensor_conversio_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/conversio/lista-tensor-shaped.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs shaped lista tensor conversio fixture");
    assert_eq!(host.stdout_lines, expected_lines(&path));
    assert!(host.stderr_lines.is_empty());
}

#[test]
fn runner_rejects_shaped_lista_tensor_count_mismatch() {
    let mut host = BufferHost::default();
    let err = run_fixture_source(
        r"
incipit {
    fixum lista<f32> flat ← [1.0, 2.0, 3.0]
    fixum tf32[2, 2] grid ← flat ↦ tf32[2, 2]
    nota grid.magnitudines()
}
",
        &mut host,
    )
    .expect_err("shaped lista tensor conversio should reject element count mismatch");

    assert!(
        err.iter()
            .any(|error| error.issue == "runner_tensor_structa_element_count_does_not_match_shape"),
        "expected tensor structa element count mismatch, got: {err:?}"
    );
}

#[test]
fn runner_runs_tensor_shape_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/tensor/shape.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs tensor shape fixture");
    assert_eq!(
        host.stdout_lines,
        vec![
            "2".to_owned(),
            "[2, 3]".to_owned(),
            "2".to_owned(),
            "9.0".to_owned(),
            "3".to_owned(),
        ]
    );
    assert!(host.stderr_lines.is_empty());
}

#[test]
fn runner_reports_tensor_negative_index() {
    let mut host = BufferHost::default();
    let err = run_fixture_source(
        r"
incipit {
    fixum tf32[] seed ← vacua
    varia tf32[2] input ← seed.crea(0.0, [2])
    nota input.accipe([-1]) vel 0.0
}
",
        &mut host,
    )
    .expect_err("negative tensor accipe index should be recoverable runtime error");

    assert!(
        err.iter()
            .any(|error| error.issue == "runner_tensor_accipe_invalid_index"),
        "expected tensor accipe invalid index, got: {err:?}"
    );
}

#[test]
fn runner_reports_tensor_ponde_out_of_bounds() {
    let mut host = BufferHost::default();
    let err = run_fixture_source(
        r"
incipit {
    fixum tf32[] seed ← vacua
    varia tf32[2] input ← seed.crea(0.0, [2])
    input.ponde([2], 9.0)
}
",
        &mut host,
    )
    .expect_err("out-of-bounds tensor ponde should be recoverable runtime error");

    assert!(
        err.iter()
            .any(|error| error.issue == "runner_tensor_ponde_invalid_index"),
        "expected tensor ponde invalid index, got: {err:?}"
    );
}

#[test]
fn runner_runs_tensor_indexed_access() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r"
incipit {
    fixum tf32[] seed ← vacua
    varia tf32[2] input ← seed.crea(0.0, [2])
    input.ponde([1], 9.0)
    nota input.accipe([1]) vel 0.0
}
",
        &mut host,
    )
    .expect("runner runs tensor get/set");

    assert_eq!(host.stdout_lines, vec!["9.0".to_owned()]);
    assert!(host.stderr_lines.is_empty());
}

#[test]
fn runner_runs_tensor_runtime_abi_success_path() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r"
incipit {
    fixum tf32[] seed ← vacua
    varia tf32[2] input ← seed.crea(0.0, [2])
    input.ponde([0], 4.0)
    input.ponde([1], 5.0)
    nota input.accipe([0]) vel 0.0
    nota input.accipe([1]) vel 0.0
}
",
        &mut host,
    )
    .expect("runner runs tensor runtime ABI success path");

    assert_eq!(host.stdout_lines, vec!["4.0".to_owned(), "5.0".to_owned()]);
    assert!(host.stderr_lines.is_empty());
}

#[test]
fn runner_reports_tensor_runtime_abi_negative_index_failure() {
    let mut host = BufferHost::default();
    let err = run_fixture_source(
        r"
incipit {
    fixum tf32[] seed ← vacua
    varia tf32[2] input ← seed.crea(0.0, [2])
    nota input.accipe([-1]) vel 0.0
}
",
        &mut host,
    )
    .expect_err("negative tensor index should be recoverable runtime error");

    assert!(
        err.iter()
            .any(|error| error.issue == "runner_tensor_accipe_invalid_index"),
        "expected tensor accipe invalid index, got: {err:?}"
    );
}

#[test]
fn runner_runs_tensor_textus_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/tensor/textus.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs tensor textus fixture");
    assert_eq!(host.stdout_lines, vec!["alpha".to_owned()]);
    assert!(host.stderr_lines.is_empty());
}

#[test]
fn runner_runs_tensor_arithmetic_elementwise_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/tensor/arithmetic-elementwise.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs tensor arithmetic elementwise fixture");
    assert_eq!(host.stdout_lines, expected_lines(&path));
    assert!(host.stderr_lines.is_empty());
}

#[test]
fn runner_runs_tensor_arithmetic_reduction_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/tensor/arithmetic-reduction.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs tensor arithmetic reduction fixture");
    assert_eq!(host.stdout_lines, expected_lines(&path));
    assert!(host.stderr_lines.is_empty());
}

#[test]
fn runner_runs_tensor_arithmetic_matmul_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/tensor/arithmetic-matmul.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs tensor arithmetic matmul fixture");
    assert_eq!(host.stdout_lines, expected_lines(&path));
    assert!(host.stderr_lines.is_empty());
}

#[test]
fn runner_runs_matrix_matmul_identity_product() {
    let mut host = BufferHost::default();
    // 2×2: [[1,2],[3,4]] * I = [[1,2],[3,4]] — print first row cells via applica.
    run_fixture_source(
        r"
incipit {
    fixum matrix<f32, [2, 2]> a ← [[1.0, 2.0], [3.0, 4.0]] ↦ matrix<f32, [2, 2]>
    fixum matrix<f32, [2, 2]> i ← [[1.0, 0.0], [0.0, 1.0]] ↦ matrix<f32, [2, 2]>
    fixum matrix<f32, [2, 2]> p ← a.matmul(i)
    fixum vector<f32, 2> row0 ← p.applica([1.0, 0.0] ↦ vector<f32, 2>)
    fixum vector<f32, 2> row1 ← p.applica([0.0, 1.0] ↦ vector<f32, 2>)
    nota row0
    nota row1
}
",
        &mut host,
    )
    .expect("runner runs matrix matmul identity product");
    assert_eq!(host.stdout_lines, vec!["[1.0, 3.0]".to_owned(), "[2.0, 4.0]".to_owned()]);
    assert!(host.stderr_lines.is_empty());
}

#[test]
fn runner_runs_matrix_applica_known_product() {
    let mut host = BufferHost::default();
    // [[1,2],[3,4]] * [1,1] = [3,7]
    run_fixture_source(
        r"
incipit {
    fixum matrix<f32, [2, 2]> m ← [[1.0, 2.0], [3.0, 4.0]] ↦ matrix<f32, [2, 2]>
    fixum vector<f32, 2> v ← [1.0, 1.0] ↦ vector<f32, 2>
    fixum vector<f32, 2> r ← m.applica(v)
    nota r
}
",
        &mut host,
    )
    .expect("runner runs matrix applica known product");
    assert_eq!(host.stdout_lines, vec!["[3.0, 7.0]".to_owned()]);
    assert!(host.stderr_lines.is_empty());
}

/// Shared Vector3-class ops (add/sub/elementwise mul/dot/cross) on register
/// vectors — numbers match the Triga Stage 1 CPU math meanings for the same
/// mathematical operations (domain Vector3 remains a separate genus).
#[test]
#[ignore = "tracking: bulk-test-cleanup — feature not yet complete"]
fn runner_runs_vector_shared_subset_ops() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r"
incipit {
    fixum vector<f32, 3> a ← [1.0, 2.0, 3.0] ↦ vector<f32, 3>
    fixum vector<f32, 3> b ← [4.0, 5.0, 6.0] ↦ vector<f32, 3>
    nota a.addita(b)
    nota a.subtrahe(b)
    nota a.multiplica(b)
    nota a.dot(b)
    nota a.cross(b)
}
",
        &mut host,
    )
    .expect("runner runs vector shared subset ops");
    // add [5,7,9]; sub [-3,-3,-3]; mul [4,10,18];
    // dot 1*4+2*5+3*6=32; cross [2*6-3*5, 3*4-1*6, 1*5-2*4]=[-3,6,-3]
    assert_eq!(
        host.stdout_lines,
        vec![
            "[5.0, 7.0, 9.0]".to_owned(),
            "[-3.0, -3.0, -3.0]".to_owned(),
            "[4.0, 10.0, 18.0]".to_owned(),
            "32.0".to_owned(),
            "[-3.0, 6.0, -3.0]".to_owned(),
        ]
    );
    assert!(host.stderr_lines.is_empty());
}

/// Affine scale-then-translate point product matching triga-transforms.fab:
/// T(5,-2,3) * S(2,3,4) applied to (1,2,-1) → (7,4,-1).
///
/// Layout: language `matrix` is row-major cell order with column-vector
/// applica (row·vector). Triga Matrix4 stores column-major lista; the
/// mathematical product agrees when both represent the same affine map.
#[test]
fn runner_runs_vector_longitudo_and_normalizata() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r"
incipit {
    fixum vector<f32, 3> v ← [3.0, 0.0, 4.0] ↦ vector<f32, 3>
    nota v.longitudo()
    nota v.normalizata()
    fixum vector<f32, 3> z ← [0.0, 0.0, 0.0] ↦ vector<f32, 3>
    nota z.normalizata()
}
",
        &mut host,
    )
    .expect("runner runs vector longitudo/normalizata");
    assert_eq!(host.stdout_lines[0], "5.0");
    assert_eq!(host.stdout_lines[1], "[0.6, 0.0, 0.8]");
    assert_eq!(host.stdout_lines[2], "[0.0, 0.0, 0.0]");
    assert!(host.stderr_lines.is_empty());
}

#[test]
fn runner_runs_matrix_inversa_2x2_known_inverse() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r"
incipit {
    fixum matrix<f32, [2, 2]> m ← [[4.0, 7.0], [2.0, 6.0]] ↦ matrix<f32, [2, 2]>
    fixum matrix<f32, [2, 2]> id ← [[1.0, 0.0], [0.0, 1.0]] ↦ matrix<f32, [2, 2]>
    fixum matrix<f32, [2, 2]> i ← m.inversa() vel id
    fixum vector<f32, 2> r ← i.applica([1.0, 0.0] ↦ vector<f32, 2>)
    nota r
}
",
        &mut host,
    )
    .expect("runner runs matrix inversa 2x2");
    // inv([[4,7],[2,6]]) = 0.1 * [[6,-7],[-2,4]] → first column via e0 applica ≈ [0.6, -0.2]
    // (float formatting may print 0.6000000000000001)
    assert_eq!(host.stdout_lines.len(), 1, "{:?}", host.stdout_lines);
    assert!(
        host.stdout_lines[0].starts_with("[0.6") && host.stdout_lines[0].contains("-0.2"),
        "unexpected inverse column: {}",
        host.stdout_lines[0]
    );
    assert!(host.stderr_lines.is_empty());
}

#[test]
fn runner_rejects_singular_matrix_inverse() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r#"
incipit {
    fixum matrix<f32, [2, 2]> singular ← [[1.0, 2.0], [2.0, 4.0]] ↦ matrix<f32, [2, 2]>
    adfirma singular.inversa() est nihil secus "singular inverse rejects"
}
"#,
        &mut host,
    )
    .expect("runner rejects singular matrix inverse");
    assert!(host.stderr_lines.is_empty());
}

#[test]
fn runner_runs_matrix4_affine_inversa_round_trip() {
    let mut host = BufferHost::default();
    // Same T*S map as triga-transforms; inverse should restore local point.
    run_fixture_source(
        r"
incipit {
    fixum matrix<f32, [4, 4]> scale ← [
        [2.0, 0.0, 0.0, 0.0],
        [0.0, 3.0, 0.0, 0.0],
        [0.0, 0.0, 4.0, 0.0],
        [0.0, 0.0, 0.0, 1.0]
    ] ↦ matrix<f32, [4, 4]>
    fixum matrix<f32, [4, 4]> translation ← [
        [1.0, 0.0, 0.0, 5.0],
        [0.0, 1.0, 0.0, (0.0 - 2.0)],
        [0.0, 0.0, 1.0, 3.0],
        [0.0, 0.0, 0.0, 1.0]
    ] ↦ matrix<f32, [4, 4]>
    fixum matrix<f32, [4, 4]> world ← translation.matmul(scale)
    fixum matrix<f32, [4, 4]> id ← [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0]
    ] ↦ matrix<f32, [4, 4]>
    fixum matrix<f32, [4, 4]> inv ← world.inversa() vel id
    fixum vector<f32, 4> transformed ← [7.0, 4.0, (0.0 - 1.0), 1.0] ↦ vector<f32, 4>
    fixum vector<f32, 4> local ← inv.applica(transformed)
    nota local
}
",
        &mut host,
    )
    .expect("runner runs matrix4 affine inversa round-trip");
    assert_eq!(host.stdout_lines.len(), 1, "{:?}", host.stdout_lines);
    // expect ≈ [1, 2, -1, 1]
    assert!(
        host.stdout_lines[0].contains("1.0")
            && host.stdout_lines[0].contains("2.0")
            && host.stdout_lines[0].contains("-1.0"),
        "round-trip local: {}",
        host.stdout_lines[0]
    );
    assert!(host.stderr_lines.is_empty());
}

#[test]
fn runner_runs_matrix4_transform_chain_matches_cpu() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r"
incipit {
    fixum matrix<f32, [4, 4]> scale ← [
        [2.0, 0.0, 0.0, 0.0],
        [0.0, 3.0, 0.0, 0.0],
        [0.0, 0.0, 4.0, 0.0],
        [0.0, 0.0, 0.0, 1.0]
    ] ↦ matrix<f32, [4, 4]>
    fixum matrix<f32, [4, 4]> translation ← [
        [1.0, 0.0, 0.0, 5.0],
        [0.0, 1.0, 0.0, (0.0 - 2.0)],
        [0.0, 0.0, 1.0, 3.0],
        [0.0, 0.0, 0.0, 1.0]
    ] ↦ matrix<f32, [4, 4]>
    fixum matrix<f32, [4, 4]> world ← translation.matmul(scale)
    fixum vector<f32, 4> local ← [1.0, 2.0, (0.0 - 1.0), 1.0] ↦ vector<f32, 4>
    fixum vector<f32, 4> transformed ← world.applica(local)
    nota transformed
}
",
        &mut host,
    )
    .expect("runner runs matrix4 transform chain matching triga CPU numbers");
    assert_eq!(host.stdout_lines, vec!["[7.0, 4.0, -1.0, 1.0]".to_owned()]);
    assert!(host.stderr_lines.is_empty());
}

#[test]
fn runner_appends_octeti_byte_in_place() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r"
incipit {
    fixum octeti bytes ← |de ad|
    bytes.appende(0xbe ∷ numerus<u8>)
    nota bytes.longitudo()
    nota bytes.accipe(2)
}
",
        &mut host,
    )
    .expect("runner appends octeti byte in place");
    assert_eq!(host.stdout_lines, vec!["3".to_owned(), "190".to_owned()]);
    assert!(host.stderr_lines.is_empty());
}

#[test]
fn runner_runs_in_parameter_summa_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/in/in.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs in parameter summa fixture");
    assert_eq!(host.stdout_lines, vec!["6".to_owned()]);
    assert!(host.stderr_lines.is_empty());
}

#[test]
fn runner_sums_empty_numerus_lista() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r"
incipit {
    fixum lista<numerus> xs ← vacua
    nota xs.summa()
}
",
        &mut host,
    )
    .expect("runner sums empty numerus lista");
    assert_eq!(host.stdout_lines, vec!["0".to_owned()]);
    assert!(host.stderr_lines.is_empty());
}

#[test]
fn runner_sums_modular_word_lista_with_wrapping() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r"
incipit {
    fixum lista<modulus<u32>> values ← [4294967295, 1]
    nota values.summa()
}
",
        &mut host,
    )
    .expect("runner sums modular word lista with wrapping");
    assert_eq!(host.stdout_lines, vec!["0".to_owned()]);
}

#[test]
fn runner_sums_modular_word_u64_lista_with_wrapping() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r"
incipit {
    fixum lista<modulus<u64>> values ← [18446744073709551615, 1]
    nota values.summa()
}
",
        &mut host,
    )
    .expect("runner sums modular word u64 lista with wrapping");
    assert_eq!(host.stdout_lines, vec!["0".to_owned()]);
}

#[test]
fn runner_longitudo_of_empty_lista_is_zero() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r"
incipit {
    fixum lista<numerus> xs ← vacua
    nota xs.longitudo()
}
",
        &mut host,
    )
    .expect("runner empty lista longitudo is zero");
    assert_eq!(host.stdout_lines, vec!["0".to_owned()]);
    assert!(host.stderr_lines.is_empty());
}

#[test]
fn runner_runs_fractus_approximata_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/intrinseca/fractus-approximata.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs fractus approximata fixture");
    assert_eq!(host.stdout_lines, vec!["verum".to_owned(), "falsum".to_owned()]);
    assert!(host.stderr_lines.is_empty());
}

#[test]
fn runner_runs_radix_conversio_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/conversio/radix.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs radix conversio fixture");
    assert_eq!(host.stdout_lines, vec!["255".to_owned(), "10".to_owned(), "493".to_owned()]);
    assert!(host.stderr_lines.is_empty());
}

#[test]
fn runner_keeps_inline_conversio_recovery_local() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r#"
functio inlineRecovery(valor v) → instans {
    fixum instans epoch ← "1970-01-01T00:00:00Z" ↦ instans
    redde v ↦ instans ⇥ epoch
}

incipit {
    fixum valor bad ← "not-a-datetime"
    nota inlineRecovery(bad) ↦ textus
}
"#,
        &mut host,
    )
    .expect("runner keeps inline conversio recovery local");
    assert_eq!(host.stdout_lines, vec!["1970-01-01T00:00:00Z".to_owned()]);
    assert!(host.stderr_lines.is_empty());
}

#[test]
fn runner_keeps_non_failable_conversio_failure_hard() {
    let mut host = BufferHost::default();
    let errors = run_fixture_source(
        r#"
functio parseInstans(valor v) → instans {
    redde v ↦ instans
}

incipit {
    fixum valor bad ← "not-a-datetime"
    nota parseInstans(bad) ↦ textus
}
"#,
        &mut host,
    )
    .expect_err("non-failable conversio failure remains hard");
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].issue, "runner_valor_to_instans_conversion_failed");
}

#[test]
fn runner_runs_fixum_exemplum() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/fixum/fixum.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs fixum exemplum");
    assert_eq!(host.stdout_lines, expected_lines(&path));
}

#[test]
fn runner_runs_lista_exemplum() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/lista/lista.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs lista exemplum");
    assert_eq!(host.stdout_lines, expected_lines(&path));
}

#[test]
fn runner_runs_elige_exemplum() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/elige/elige.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs elige exemplum");
    assert_eq!(host.stdout_lines, expected_lines(&path));
}

#[test]
fn runner_runs_mone_to_stderr_buffer() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r#"
incipit {
    mone "cave"
}
"#,
        &mut host,
    )
    .expect("runner runs mone fixture");
    assert!(host.stdout_lines.is_empty());
    assert_eq!(host.stderr_lines, vec!["cave".to_owned()]);
}

#[test]
fn runner_runs_numeric_loop_fixture() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r"
incipit {
    varia numerus summa ← 0
    varia numerus n ← 1
    dum n ≤ 3 {
        summa ← summa + n
        n ← n + 1
    }
    nota summa
}
",
        &mut host,
    )
    .expect("runner runs loop fixture");
    assert_eq!(host.stdout_lines, vec!["6".to_owned()]);
}

#[test]
fn runner_initializes_top_level_const_before_skippable_branch_use() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r#"
fixum numerus LIMIT ← 4

functio read_limit() → numerus {
    varia numerus n ← 0
    si n > 0 {
        nota LIMIT
    }
    redde LIMIT
}

incipit {
    nota read_limit()
}
"#,
        &mut host,
    )
    .expect("top-level const remains initialized when its first use is skipped");
    assert_eq!(host.stdout_lines, vec!["4".to_owned()]);
}

#[test]
fn runner_runs_try_call_fac_cape_fixture() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r#"
functio divide(numerus a, numerus b) → numerus ⇥ textus {
    si b ≡ 0 ergo iace "division by zero"
    redde a / b
}

functio tutum(numerus a, numerus b) → numerus {
    fac {
        redde divide(a, b)
    } cape err {
        mone err
        redde 0
    }
}

incipit {
    nota tutum(10, 2)
    nota tutum(10, 0)
}
"#,
        &mut host,
    )
    .expect("runner runs try-call fixture");
    assert_eq!(host.stdout_lines, vec!["5".to_owned(), "0".to_owned()]);
    assert_eq!(host.stderr_lines, vec!["division by zero".to_owned()]);
}

#[test]
fn runner_runs_argv_provider_fixture() {
    let mut host = BufferHost::with_argumenta(vec!["alpha".to_owned(), "beta".to_owned()]);
    run_fixture_source(
        r#"
importa ex "faber:processus" processus

incipit {
    fixum _ args ← processus.argv()
    nota args
}
"#,
        &mut host,
    )
    .expect("runner runs argv fixture");
    assert_eq!(host.stdout_lines.len(), 1);
    assert!(host.stdout_lines[0].contains("alpha"));
    assert!(host.stdout_lines[0].contains("beta"));
}

#[test]
fn runner_runs_processus_env_provider_fixture() {
    let mut host = BufferHost::default().with_env("FABER_SCRIPTING_TEST_ENV", "salve");
    run_fixture_source(
        r#"
importa ex "faber:processus" processus

incipit {
    nota processus.read_env("FABER_SCRIPTING_TEST_ENV")
}
"#,
        &mut host,
    )
    .expect("runner runs processus.read_env fixture");
    assert_eq!(host.stdout_lines, vec!["salve".to_owned()]);
}

#[test]
fn runner_processus_read_env_fails_when_env_unset() {
    let probe = "FABER_SCRIPTING_TEST_UNSET_ENV";
    assert!(std::env::var(probe).is_err(), "probe var must not pre-exist in real env");

    let mut host = BufferHost::default();
    let errors = run_fixture_source(
        r#"
importa ex "faber:processus" processus

incipit {
    nota processus.read_env("FABER_SCRIPTING_TEST_UNSET_ENV")
}
"#,
        &mut host,
    )
    .expect_err("unset env must fail processus.read_env");
    assert!(
        errors
            .iter()
            .any(|error| error.issue == "runner_processus_read_env_env_missing"),
        "expected unset env error, got {errors:?}"
    );
    assert!(host.stdout_lines.is_empty());
}

#[test]
fn runner_processus_read_env_succeeds_for_set_empty_string() {
    let probe = "FABER_SCRIPTING_TEST_EMPTY_ENV";
    let mut host = BufferHost::default().with_env(probe, "");
    run_fixture_source(
        r#"
importa ex "faber:processus" processus

incipit {
    nota processus.read_env("FABER_SCRIPTING_TEST_EMPTY_ENV")
}
"#,
        &mut host,
    )
    .expect("set-but-empty env must succeed processus.read_env");
    assert_eq!(host.stdout_lines, vec![String::new()]);
}

#[test]
fn runner_processus_write_env_isolates_buffer_host_env() {
    // WHY: BufferHost is the test/embedder sandbox. `processus.write_env` must
    // mutate its in-memory env map, never the real cargo process env, so
    // parallel test runs cannot pollute each other or the host shell.
    let probe = "FABER_SCRIPTING_TEST_SANDBOX";
    assert!(std::env::var(probe).is_err(), "probe var must not pre-exist in real env");

    let mut host = BufferHost::default();
    run_fixture_source(
        r#"
importa ex "faber:processus" processus

incipit {
    processus.write_env("FABER_SCRIPTING_TEST_SANDBOX", "inter")
    nota processus.read_env("FABER_SCRIPTING_TEST_SANDBOX")
}
"#,
        &mut host,
    )
    .expect("runner runs processus.write_env fixture");
    assert_eq!(host.stdout_lines, vec!["inter".to_owned()]);
    assert_eq!(host.env.get(probe).map(String::as_str), Some("inter"));
    assert!(std::env::var(probe).is_err(), "write_env must not leak to real process env");
}

#[test]
fn runner_runs_processus_cwd_provider_fixture() {
    let mut host = BufferHost::default().with_cwd("/faber/sandbox");
    run_fixture_source(
        r#"
importa ex "faber:processus" processus

incipit {
    fixum _ cwd ← processus.cwd()
    nota cwd
}
"#,
        &mut host,
    )
    .expect("runner runs processus.cwd fixture");
    assert_eq!(host.stdout_lines, vec!["/faber/sandbox".to_owned()]);
}

#[test]
fn runner_runs_bitwise_exemplum() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/operatores/bitwise.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs bitwise exemplum");
    assert_eq!(
        host.stdout_lines,
        vec!["8".to_owned(), "6".to_owned(), "16".to_owned(), "4".to_owned()]
    );
}

#[test]
fn runner_runs_logica_exemplum() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/operatores/logica.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs logica exemplum");
    assert_eq!(
        host.stdout_lines,
        vec!["14".to_owned(), "verum".to_owned(), "falsum".to_owned()]
    );
}

#[test]
fn runner_short_circuits_logical_operands() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r#"
functio side() → bivalens {
    nota "bad"
    redde verum
}

incipit {
    sit left_false ← falsum et side()
    sit left_true ← verum aut side()
    nota left_false
    nota left_true
}
"#,
        &mut host,
    )
    .expect("runner runs short-circuit fixture");
    assert_eq!(host.stdout_lines, vec!["falsum".to_owned(), "verum".to_owned()]);
}

#[test]
fn runner_runs_tabula_exemplum() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/tabula/tabula.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs tabula exemplum");
    assert_eq!(host.stdout_lines, expected_lines(&path));
}

#[test]
fn runner_runs_tabula_methodi_accessus_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/tabula/methodi-accessus.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs tabula methodi accessus fixture");
    // L23 (d57baa50): the multi-arg nota grouping space-joins every argument
    // (including option payloads via `display_option` semantics) into ONE
    // diagnostic line, mirroring the Rust oracle.
    assert_eq!(host.stdout_lines, vec!["95 nihil 0 verum verum 1 falsum".to_owned()]);
    assert!(host.stderr_lines.is_empty());
}

#[test]
fn runner_runs_copia_fundamenta_exemplum() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/intrinseca/copia-fundamenta.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs copia-fundamenta exemplum");
    assert_eq!(host.stdout_lines, vec!["verum verum 2 falsum".to_owned()]);
}

#[test]
fn runner_runs_fac_dum_exemplum() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/fac/fac-dum.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs fac-dum exemplum");
    assert_eq!(host.stdout_lines, expected_lines(&path));
}

#[test]
fn runner_runs_conversio_exemplum() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/conversio/conversio.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs conversio exemplum");
    assert_eq!(host.stdout_lines, expected_lines(&path));
}

#[test]
fn runner_runs_bivalens_conversio_fixture() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r#"
incipit {
    fixum _ b1 ← 0 ↦ bivalens
    fixum _ b2 ← 42 ↦ bivalens
    nota b1
    nota b2

    fixum _ b3 ← "" ↦ bivalens
    fixum _ b4 ← "salve" ↦ bivalens
    nota b3
    nota b4

    fixum _ habet ← ("123" ↦ numerus ⇥ 0) ↦ bivalens
    nota habet
}
"#,
        &mut host,
    )
    .expect("runner runs bivalens conversio fixture");
    assert_eq!(
        host.stdout_lines,
        vec![
            "falsum".to_owned(),
            "verum".to_owned(),
            "falsum".to_owned(),
            "verum".to_owned(),
            "verum".to_owned(),
        ]
    );
}

#[test]
fn runner_runs_valor_genus_conversio_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/conversio/valor-genus.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs valor genus conversio fixture");
    assert_eq!(host.stdout_lines, expected_lines(&path));
}

#[test]
fn runner_runs_regex_conversio_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/literalia/regex.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs regex conversio fixture");
    assert_eq!(
        host.stdout_lines,
        vec!["\\d+".to_owned(), "(?i)\\w+".to_owned(), "/home/acme/.*".to_owned(),]
    );
}

#[test]
fn runner_runs_lege_regex_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/lege/lege.fab");
    let mut host = BufferHost::default().with_stdin_lines(["salve"]);
    run_exemplum(&path, &mut host).expect("runner runs lege regex fixture");
    assert_eq!(host.stdout_lines, vec!["(?g)\\d+".to_owned(), "salve".to_owned()]);
}

#[test]
fn runner_lege_eof_is_nullable() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r#"
incipit {
    fixum textus ∪ nihil line ← lege
    nota line vel "eof"
}
"#,
        &mut host,
    )
    .expect("runner runs nullable lege EOF fixture");
    assert_eq!(host.stdout_lines, vec!["eof".to_owned()]);
}

#[test]
fn runner_runs_optionalis_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/optionalis/optionalis.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs optionalis fixture");
    assert_eq!(
        host.stdout_lines,
        vec![
            "Roma".to_owned(),
            "Italia".to_owned(),
            "nihil".to_owned(),
            "nihil".to_owned(),
            "a".to_owned(),
            "nihil".to_owned(),
            "ignota".to_owned(),
        ]
    );
}

#[test]
fn runner_runs_optional_chain_operator_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/operatores/optional-chain.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs optional-chain operator fixture");
    assert_eq!(
        host.stdout_lines,
        vec![
            "Roma".to_owned(),
            "Italia".to_owned(),
            "nihil".to_owned(),
            "nihil".to_owned(),
            "a".to_owned(),
            "nihil".to_owned(),
        ]
    );
}

#[test]
fn runner_keeps_ordinary_index_strict() {
    let mut host = BufferHost::default();
    let errors = run_fixture_source(
        r#"
incipit {
    fixum _ litterae ← ["a", "b", "c"]
    nota litterae[10]
}
"#,
        &mut host,
    )
    .expect_err("ordinary index access remains strict");
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].issue, "runner_array_index_out_of_bounds");
}

#[test]
fn runner_reports_nonnull_assertion_failure() {
    // FAO-010: non-null assertion `!.` must fail with Faber-level RunnerError,
    // not a Rust-style panic or silent default. The MIR runner owns this
    // semantic via `MirOptionUnwrapMode::Assert → host.abort("unwrap of nihil")`.
    let mut host = BufferHost::default();
    let failed = catch_unwind(AssertUnwindSafe(|| {
        run_fixture_source(
            r"
genus Persona {
    textus nomen
    textus urbs sponte
}

incipit {
    fixum Persona ∪ nihil p ← nihil
    nota p!.nomen
}
",
            &mut host,
        )
    }));
    assert!(failed.is_err(), "nonnull assertion on nihil must abort");
    assert_eq!(host.last_abort.as_deref(), Some("unwrap of nihil"));
}

#[test]
fn runner_runs_finge_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/finge/finge.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs finge fixture");
    assert_eq!(host.stdout_lines, vec!["finge expressiones paratae".to_owned()]);
}

#[test]
fn runner_runs_omnia_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/omnia/omnia.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs omnia fixture");
    assert_eq!(host.stdout_lines, vec!["activa".to_owned()]);
}

#[test]
fn runner_runs_discerne_insanum_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/integratio/discerne-insanum.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs discerne-insanum fixture");
    assert_eq!(
        host.stdout_lines,
        vec![
            "moveo forum per 8".to_owned(),
            "loquere salve".to_owned(),
            "silentium".to_owned(),
        ]
    );
}

#[test]
fn runner_runs_unio_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/unio/unio.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs unio fixture");
    assert_eq!(
        host.stdout_lines,
        vec![
            "42".to_owned(),
            "salve".to_owned(),
            "n: 7".to_owned(),
            "t: faber".to_owned()
        ]
    );
}

#[test]
fn runner_runs_tacet_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/tacet/tacet.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs tacet fixture");
    assert_eq!(host.stdout_lines, vec!["cond verum".to_owned(), "finis".to_owned()]);
}

#[test]
fn runner_runs_ordo_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/ordo/ordo.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs ordo fixture");
    assert_eq!(host.stdout_lines, vec!["rubrum".to_owned(), "actum".to_owned()]);
}

#[test]
fn runner_runs_discerne_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/discerne/discerne.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs discerne fixture");
    assert_eq!(host.stdout_lines, vec!["discerne exempla parata".to_owned()]);
}

#[test]
fn runner_runs_discerne_nihil_literal_switch() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r#"
incipit {
    fixum textus ∪ nihil missing ← nihil
    fixum textus ∪ nihil present ← "faber"

    discerne missing {
        casu nihil { nota "missing" }
        ceterum { nota "present" }
    }
    discerne present {
        casu nihil { nota "missing" }
        ceterum { nota "present" }
    }
}
"#,
        &mut host,
    )
    .expect("runner runs nihil literal switch");
    assert_eq!(host.stdout_lines, vec!["missing".to_owned(), "present".to_owned()]);
}

#[test]
fn runner_runs_lista_methodi_mutatio_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/lista/methodi-mutatio.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs lista methodi-mutatio fixture");
    assert_eq!(host.stdout_lines, vec!["[1, 2]".to_owned(), "3".to_owned(), "4".to_owned()]);
}

#[test]
fn runner_runs_lista_methodi_copiae_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/lista/methodi-copiae.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs lista methodi-copiae fixture");
    assert_eq!(
        host.stdout_lines,
        vec![
            "[1, 2, 3, 4, 5, 6]".to_owned(),
            "[2, 3, 4]".to_owned(),
            "[1, 2]".to_owned(),
            "[4, 5]".to_owned(),
            "[3, 4, 5]".to_owned(),
            "[5, 4, 3, 2, 1]".to_owned(),
            "[1, 2, 3]".to_owned(),
        ]
    );
}

#[test]
fn runner_runs_lista_methodi_functionales_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/lista/methodi-functionales.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs lista methodi-functionales fixture");
    assert_eq!(
        host.stdout_lines,
        vec!["[2, 4]".to_owned(), "[2, 4, 6, 8, 10]".to_owned(), "15".to_owned()]
    );
}

#[test]
fn runner_runs_captured_collection_callback() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r"
incipit {
    fixum _ xs ← [1, 2, 3] ∷ lista<numerus>
    fixum numerus factor ← 3
    nota xs.mappata(numerus x ∴ x * factor)
}
",
        &mut host,
    )
    .expect("runner runs captured collection callback");
    assert_eq!(host.stdout_lines, vec!["[3, 6, 9]".to_owned()]);
}

#[test]
fn runner_runs_destructura_literal_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/destructura/literal.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs destructura literal fixture");
    assert_eq!(host.stdout_lines.len(), 4);
    assert!(host.stdout_lines[3].contains("medium"));
}

#[test]
fn runner_runs_sermo_conversio_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/ad/sermo-conversio.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs sermo conversio fixture");
    assert!(host.stdout_lines.is_empty());
}

#[test]
fn runner_runs_morphologia_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/morphologia/morphologia.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs morphologia fixture");
    assert_eq!(
        host.stdout_lines,
        vec![
            "filtrata nova: [2, 4]".to_owned(),
            "addita nova: [1, 2, 3, 4, 5, 6]".to_owned(),
            "appende mutat: [1, 2, 3, 4]".to_owned(),
            "inversa nova: [5, 4, 3, 2, 1]".to_owned(),
            "inverte mutat: [3, 2, 1]".to_owned(),
            "ordinata nova: [1, 2, 3, 4, 5]".to_owned(),
            "ordina mutat: [1, 2, 3]".to_owned(),
            "mappata nova: [2, 4, 6, 8, 10]".to_owned(),
            "res prima: [1, 2, 3, 4, 5]".to_owned(),
        ]
    );
}

#[test]
fn runner_rejects_omitte_without_incipit() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/omitte/omitte.fab");
    let mut host = BufferHost::default();
    let errors = run_exemplum(&path, &mut host).expect_err("omitte fixture has no script entry");
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].issue, "runner_no_entry_function_in_mir_program");
    assert!(host.stdout_lines.is_empty());
}

#[test]
fn runner_adfirma_uses_custom_message() {
    let mut host = BufferHost::default();
    let failed = catch_unwind(AssertUnwindSafe(|| {
        run_fixture_source(
            r#"
incipit {
    adfirma falsum secus "custom failure"
}
"#,
            &mut host,
        )
    }));
    assert!(failed.is_err(), "failing adfirma should abort");
    assert_eq!(host.last_abort.as_deref(), Some("custom failure"));
}

#[test]
fn runner_runs_function_call_fixture() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r"
functio duplex(numerus n) → numerus {
    redde n * 2
}

incipit {
    nota duplex(21)
}
",
        &mut host,
    )
    .expect("runner runs function-call fixture");
    assert_eq!(host.stdout_lines, vec!["42".to_owned()]);
}

#[test]
fn runner_runs_same_program_captured_closure_call() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r"
incipit {
    fixum _ base ← 10
    fixum _ adde ← numerus x ∴ x + base
    nota adde(5)
}
",
        &mut host,
    )
    .expect("runner runs captured closure call");
    assert_eq!(host.stdout_lines, vec!["15".to_owned()]);
}

#[test]
fn runner_runs_est_fixture() {
    // S5-U3: focused runner proof for the `est` surface (codex-gap Stage 5).
    // The corpus fixture exercises null checks (`x is null`), exact boolean
    // checks (`enabled is true`), `est <type>` variant-tag lowering, and
    // `est`-based dispatch in functions with `unknown`/`bivalens` parameters.
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/est/est.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs est fixture");
    assert_eq!(host.stdout_lines, expected_lines(&path));
}

#[test]
fn runner_runs_type_hole_union_fixture() {
    // S5-U4: focused runner proof for the type-hole-union surface
    // (codex-gap Stage 5). The corpus fixture exercises `_` vs `∪` holes,
    // `list<∪>`, `T ∪ nihil`, and `match` over union-typed slots.
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/type-hole-union/type-hole-union.fab");
    let mut host = BufferHost::default();
    run_exemplum(&path, &mut host).expect("runner runs type-hole-union fixture");
    assert_eq!(host.stdout_lines, expected_lines(&path));
}

#[test]
fn runner_matches_union_typed_slots() {
    // S5-U4: focused match-over-union proof mirroring the type-hole-union
    // fixture's consumer functions: literal arms, a `null` arm, and a
    // wildcard-only match all dispatch against union-typed values. The
    // frontmatter block activates the locale lex pack so union type
    // signatures parse (same as the corpus pipeline).
    let mut host = BufferHost::default();
    run_fixture_source(
        "+++\nterm = \"type-hole-union\"\nkind = \"type\"\ncategory = \"types\"\ncanonical = true\nsummary = \"union slots\"\nsyntax = \"a\"\naliases = []\nrelated = [\"unio\", \"nihil\"]\nlocale = \"en\"\n+++\n\nfn member_count(list<int ∪ string> xs) → int {\n    match xs {\n        case _ { return 1 }\n    }\n}\n\nfn consume(int ∪ string value) → int {\n    match value {\n        case 7 { return 1 }\n        case \"x\" { return 2 }\n        default { return 0 }\n    }\n}\n\nfn maybe_len(string ∪ null value) → int {\n    match value {\n        case null { return 0 }\n        default { return 1 }\n    }\n}\n\nmain {\n    const list<∪> nodes ← [1, \"two\", 3]\n    var ∪ mixed ← 1\n    mixed ← \"salve\"\n    const string ∪ null opt ← \"text\"\n    const _ maybe ← opt\n\n    const _ m ← member_count(nodes)\n    const _ n ← consume(mixed)\n    const _ o ← maybe_len(maybe)\n    const _ p ← maybe_len(null)\n\n    assert m ≡ 1\n    assert n ≡ 0\n    assert o ≡ 1\n    assert p ≡ 0\n\n    print m\n    print n\n    print o\n    print p\n}\n",
        &mut host,
    )
    .expect("runner runs match over union-typed slots");
    assert_eq!(host.stdout_lines, vec!["1", "0", "1", "0"]);
}

#[test]
fn runner_traps_numerus_overflow_at_i64_width() {
    // S5-U5: focused runner proof for the checked-arithmetic trap
    // (codex-gap Stage 5). The corpus fixture prints `max + 1` at bare
    // `numerus` width (i64); the runner must trap with a `numerus
    // overflow` RunnerError instead of wrapping. The trap is the intended,
    // observable behavior — the fixture's `.expected` is empty, so a harness
    // "pass" would be a silent run, but checked semantics demand the trap
    // (Stage 0 ledger §3.2 re-buckets the row to an expected-trap class).
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/operatores/numerus-overflow.fab");
    let mut host = BufferHost::default();
    let errors = run_exemplum(&path, &mut host).expect_err("numerus i64 overflow must trap instead of wrapping");
    let issues = errors
        .iter()
        .map(|error| error.issue.as_str())
        .collect::<Vec<_>>()
        .join(" | ");
    assert!(
        issues.contains("runner_numerus_overflow"),
        "expected a `numerus overflow` trap at the i64 boundary, got: {issues}"
    );
    assert!(
        errors
            .iter()
            .any(|error| error.issue == "runner_numerus_overflow"),
        "trap must carry the structured `runner_numerus_overflow` issue, got: {issues}"
    );
    assert!(host.stdout_lines.is_empty(), "the overflow trap must pre-empt any stdout");
}

#[test]
fn runner_type_checks_valor_variant_tags() {
    // S5-U3: focused `est <type>` variant-tag proof mirroring the est fixture's
    // asserts: each valor-carried kind matches its type and non-matching types
    // fail both positive and negative checks.
    let mut host = BufferHost::default();
    run_fixture_source(
        r#"
incipit {
    fixum valor name ← "faber" ↦ valor
    fixum valor count ← 42 ↦ valor
    fixum valor tags ← ["a", "b"] ↦ valor
    fixum valor active ← verum ↦ valor
    fixum valor ratio ← 1.5 ↦ valor

    adfirma name est textus
    adfirma count est numerus
    adfirma tags est lista<valor>
    adfirma active est bivalens
    adfirma ratio est fractus

    adfirma count non est textus
    adfirma name non est numerus
    adfirma count non est fractus
    adfirma ratio non est bivalens

    nota "est variant tags verified"
}
"#,
        &mut host,
    )
    .expect("runner runs est variant-tag checks");
    assert_eq!(host.stdout_lines, vec!["est variant tags verified".to_owned()]);
}

// ===========================================================================
// Glyph product families (`·` `×` `⊗` `⊙`) on the MIR runner
// (TENSOR-GLYPHS-3B). Each fixture mirrors the matching corpus glyph exemplum
// (corpus/{vector,tensor}/glyph-*.fab) and proves the 3A-lowered lane/cell
// forms execute with the runner's honest scalar-lane stdout.
// ===========================================================================

#[test]
fn runner_runs_glyph_vector_dot_product() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r"
incipit {
    fixum vector<f32, 3> a ← [1.0, 2.0, 3.0] ↦ vector<f32, 3>
    fixum vector<f32, 3> b ← [4.0, 5.0, 6.0] ↦ vector<f32, 3>
    fixum f32 d ← a · b
    nota d
}
",
        &mut host,
    )
    .expect("runner runs glyph vector dot product");
    assert_eq!(host.stdout_lines, vec!["32.0".to_owned()]);
    assert!(host.stderr_lines.is_empty());
}

#[test]
fn runner_runs_glyph_vector_cross_product() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r"
incipit {
    fixum vector<f32, 3> a ← [1.0, 2.0, 3.0] ↦ vector<f32, 3>
    fixum vector<f32, 3> b ← [4.0, 5.0, 6.0] ↦ vector<f32, 3>
    fixum vector<f32, 3> c ← a × b
    nota c
}
",
        &mut host,
    )
    .expect("runner runs glyph vector cross product");
    assert_eq!(host.stdout_lines, vec!["[-3.0, 6.0, -3.0]".to_owned()]);
    assert!(host.stderr_lines.is_empty());
}

#[test]
fn runner_runs_glyph_vector_hadamard_product() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r"
incipit {
    fixum vector<f32, 3> a ← [1.0, 2.0, 3.0] ↦ vector<f32, 3>
    fixum vector<f32, 3> b ← [4.0, 5.0, 6.0] ↦ vector<f32, 3>
    fixum vector<f32, 3> v ← a ⊙ b
    nota v
}
",
        &mut host,
    )
    .expect("runner runs glyph vector hadamard product");
    assert_eq!(host.stdout_lines, vec!["[4.0, 10.0, 18.0]".to_owned()]);
    assert!(host.stderr_lines.is_empty());
}

#[test]
fn runner_runs_glyph_vector_outer_product() {
    let mut host = BufferHost::default();
    run_fixture_source(
        r"
incipit {
    fixum vector<f32, 3> a ← [1.0, 2.0, 3.0] ↦ vector<f32, 3>
    fixum vector<f32, 2> b ← [4.0, 5.0] ↦ vector<f32, 2>
    fixum matrix<f32, [3, 2]> o ← a ⊗ b
    nota o
}
",
        &mut host,
    )
    .expect("runner runs glyph vector outer product");
    // The runner renders matrix registers as nested row-major rows, matching
    // the compiled host route's matrix display contract.
    assert_eq!(host.stdout_lines, vec!["[[4.0, 5.0], [8.0, 10.0], [12.0, 15.0]]".to_owned()]);
    assert!(host.stderr_lines.is_empty());
}

#[test]
fn runner_runs_glyph_matrix_matmul_product() {
    let mut host = BufferHost::default();
    // NOTE (TENSOR-GLYPHS-3B residual): the 3A `lower_matrix_matmul` MIR shape
    // is correct only on the square subset (result shape and inner dimension
    // come from the receiver, so rhs must be square). The corpus
    // glyph-matmul.fab pair [3,2]×[2,4] silently drops the rhs columns under
    // that shape and is recorded as a known 3A lowering residual, not covered
    // by a runner fixture. This fixture proves the square subset executes.
    run_fixture_source(
        r"
incipit {
    fixum matrix<f32, [2, 2]> left ← [[1.0, 2.0], [3.0, 4.0]] ↦ matrix<f32, [2, 2]>
    fixum matrix<f32, [2, 2]> right ← [[5.0, 6.0], [7.0, 8.0]] ↦ matrix<f32, [2, 2]>
    fixum matrix<f32, [2, 2]> product ← left · right
    nota product
}
",
        &mut host,
    )
    .expect("runner runs glyph matrix matmul product");
    // [[19,22],[43,50]] nested row-major cells.
    assert_eq!(host.stdout_lines, vec!["[[19.0, 22.0], [43.0, 50.0]]".to_owned()]);
    assert!(host.stderr_lines.is_empty());
}
