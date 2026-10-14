//! RR-2 pins: ratio Type::Record lowering, uniqueness, access, fence closure.

use crate::test_support::{self, AnalyzedFixture};
use crate::{PassConfig, Type};
use radix_lexer::lex_with_locale_pack;
use radix_parser::parse;
use radix_target::Target;

fn latin_pack() -> crate::locale::LocalePack {
    crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("la"))
        .expect("load Latin locale pack")
}

fn analyze_cli(source: &str) -> AnalyzedFixture {
    let mut parsed = parse(lex_with_locale_pack(source, &latin_pack()));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let program = parsed.program.as_ref().expect("program");
    let cli = crate::cli::analyze(program, &parsed.interner);
    assert!(
        cli.errors.is_empty(),
        "cli analysis errors: {:?}",
        cli.errors
    );
    assert!(cli.program.is_some(), "expected CLI program metadata");
    let config = PassConfig::for_target(Target::HirRust);
    let result =
        crate::analyze_with_cli(program, &config, &mut parsed.interner, cli.program.as_ref());
    AnalyzedFixture {
        result,
        interner: parsed.interner,
    }
}

fn assert_cli_accepts(source: &str) {
    let result = analyze_cli(source);
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
}

fn assert_cli_rejects_with_issue(source: &str, expected: &str) {
    let result = analyze_cli(source);
    assert!(
        !result.is_ok(),
        "expected failure, got: {:?}",
        result.diagnostics
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|err| err.is_error() && test_support::issue(err) == Some(expected)),
        "expected diagnostic issue {expected:?}, got: {:?}",
        result.diagnostics
    );
}

#[test]
fn ratio_annotation_and_construction_round_trip() {
    test_support::assert_accepts_la(
        r"
functio first() → f32 {
    fixum ratio<g: f32> r ← ratio<g: f32> { g = 1.0 }
    redde r.g
}
",
    );
}

#[test]
fn ratio_lowers_to_record() {
    let result = test_support::analyze_ok_la(
        r"
incipit {
    fixum ratio<g: f32> r ← ratio<g: f32> { g = 1.0 }
    nota r.g
}
",
    );
    let local_ty = test_support::entry_local_tys(&result)
        .into_iter()
        .next()
        .expect("r local");
    assert!(
        matches!(result.types.get(local_ty), Type::Record(_)),
        "expected Record, got {:?}",
        result.types.get(local_ty)
    );
}

#[test]
fn ratio_keyword_label_member_access_typechecks() {
    test_support::assert_accepts_la(
        r"
functio first() → f32 {
    fixum ratio<fixum: f32> r ← ratio<fixum: f32> { fixum = 1.0 }
    redde r.fixum
}
",
    );
}

#[test]
fn ratio_unknown_field_access_rejects() {
    test_support::assert_rejects_with_issue_la(
        r"
functio bad() → f32 {
    fixum ratio<g: f32> r ← ratio<g: f32> { g = 1.0 }
    redde r.nope
}
",
        "unknown_record_field",
    );
}

#[test]
fn ratio_unknown_construction_field_rejects() {
    test_support::assert_rejects_with_issue_la(
        r"
incipit {
    fixum ratio<g: f32> r ← ratio<g: f32> { g = 1.0, w = 2.0 }
    nota r.g
}
",
        "unknown_record_field",
    );
}

#[test]
fn ratio_missing_construction_field_rejects() {
    test_support::assert_rejects_with_issue_la(
        r"
incipit {
    fixum ratio<g: f32, w: f32> r ← ratio<g: f32, w: f32> { g = 1.0 }
    nota r.g
}
",
        "ratio_field_missing",
    );
}

#[test]
fn ratio_init_type_mismatch_rejects() {
    test_support::assert_rejects_with_issue_la(
        r#"
incipit {
    fixum ratio<g: f32> r ← ratio<g: f32> { g = "x" }
    nota r.g
}
"#,
        "field_initializer_type_mismatch",
    );
}

#[test]
fn ratio_distinct_field_types_do_not_assign() {
    test_support::assert_rejects_with_issue_la(
        r"
incipit {
    fixum ratio<g: f32> a ← ratio<g: f32> { g = 1.0 }
    fixum ratio<g: numerus> b ← a
    nota b.g
}
",
        "initializer_annotation_mismatch",
    );
}

#[test]
fn ratio_does_not_assign_to_same_field_genus() {
    test_support::assert_rejects_with_issue_la(
        r"
genus P {
    varia f32 g
}

incipit {
    fixum ratio<g: f32> r ← ratio<g: f32> { g = 1.0 }
    fixum P p ← r
    nota p.g
}
",
        "initializer_annotation_mismatch",
    );
}

#[test]
fn ratio_label_order_does_not_change_identity() {
    test_support::assert_accepts_la(
        r"
incipit {
    fixum ratio<g: f32, w: f32> a ← ratio<g: f32, w: f32> { g = 1.0, w = 2.0 }
    fixum ratio<w: f32, g: f32> b ← a
    nota b.g
}
",
    );
}

#[test]
fn ratio_object_pattern_destructuring() {
    test_support::assert_accepts_la(
        r"
incipit {
    fixum ratio<g: f32, w: f32> r ← ratio<g: f32, w: f32> { g = 1.0, w = 2.0 }
    fixum { g, w } ← r
    nota g
    nota w
}
",
    );
}

#[test]
fn ratio_object_pattern_ceteri() {
    test_support::assert_accepts_la(
        r"
incipit {
    fixum ratio<g: f32, w: f32> r ← ratio<g: f32, w: f32> { g = 1.0, w = 2.0 }
    fixum { g, ceteri rest } ← r
    nota g
    nota rest.w
}
",
    );
}

#[test]
fn ratio_object_pattern_unknown_label_rejects() {
    test_support::assert_rejects_with_issue_la(
        r"
incipit {
    fixum ratio<g: f32> r ← ratio<g: f32> { g = 1.0 }
    fixum { nope } ← r
    nota nope
}
",
        "unknown_record_field",
    );
}

#[test]
fn ratio_to_genus_conversio_rejects() {
    test_support::assert_rejects_with_issue_la(
        r"
genus P {
    varia f32 g
}

incipit {
    fixum ratio<g: f32> r ← ratio<g: f32> { g = 1.0 }
    fixum P p ← r ↦ P
    nota p.g
}
",
        "record_conversion_requires_reconstruction",
    );
}

#[test]
fn genus_to_ratio_conversio_rejects() {
    test_support::assert_rejects_with_issue_la(
        r"
genus P {
    varia f32 g
}

incipit {
    fixum P p ← P { g = 1.0 }
    fixum ratio<g: f32> r ← p ↦ ratio<g: f32>
    nota r.g
}
",
        "record_conversion_requires_reconstruction",
    );
}

#[test]
fn identical_record_conversio_stays() {
    test_support::assert_accepts_la(
        r"
typus R = ratio<g: f32>
incipit {
    fixum ratio<g: f32> a ← ratio<g: f32> { g = 1.0 }
    fixum ratio<g: f32> b ← a ↦ R
    nota b.g
}
",
    );
}

#[test]
fn ratio_identifier_binding_stays() {
    test_support::assert_accepts_la(
        r"
incipit {
    fixum numerus ratio ← 3
    nota ratio
}
",
    );
}

/// Row G: CLI record → genus conversio is the ruling's fence hole.
#[test]
fn args_to_genus_conversio_rejects_with_reconstruction_issue() {
    assert_cli_rejects_with_issue(
        r#"
genus P {
    varia bivalens verbose
}

@ cli "tool"
@ optio verbose brevis "v" longum "verbose" typus bivalens
incipit argumenta args {
    fixum P p ← args ↦ P
    nota p.verbose
}
"#,
        "record_conversion_requires_reconstruction",
    );
}

#[test]
fn cli_seam_field_access() {
    assert_cli_accepts(
        r#"
@ cli "tool"
@ optio verbose brevis "v" longum "verbose" typus bivalens
incipit argumenta args {
    nota args.verbose
}
"#,
    );
}

#[test]
fn cli_seam_object_pattern() {
    assert_cli_accepts(
        r#"
@ cli "tool"
@ optio verbose brevis "v" longum "verbose" typus bivalens
@ optio limen brevis "l" longum "limen" typus numerus
incipit argumenta args {
    fixum { verbose, limen } ← args
    nota verbose
    nota limen
}
"#,
    );
}

#[test]
fn cli_seam_object_pattern_ceteri() {
    assert_cli_accepts(
        r#"
@ cli "tool"
@ optio verbose brevis "v" longum "verbose" typus bivalens
@ optio limen brevis "l" longum "limen" typus numerus
incipit argumenta args {
    fixum { verbose, ceteri rest } ← args
    nota verbose
    nota rest.limen
}
"#,
    );
}

#[test]
fn cli_seam_reconstruction() {
    assert_cli_accepts(
        r#"
genus P {
    varia bivalens verbose
}

@ cli "tool"
@ optio verbose brevis "v" longum "verbose" typus bivalens
incipit argumenta args {
    fixum P p ← P { verbose = args.verbose }
    nota p.verbose
}
"#,
    );
}
