//! Runtime conversion target-fidelity tests (SA-002).
//!
//! WHY: phase-08 found the runner's `eval_runtime_convert` erased the declared
//! conversion target, so `textus ↦ numerus<u8>` could not fail on width
//! overflow the way Rust codegen's `.parse::<u8>()` does. The target-directed
//! `conversio` rewrite plus the width range-check close that gap. These tests
//! prove the parity by running real Faber source through the runner.

use crate::driver::{Config, Session};
use crate::mir::runner::{run_source, BufferHost, RunSourceError};

/// Run `source` through the runner with a fresh `BufferHost`.
fn run(source: &str) -> Result<BufferHost, RunSourceError> {
    let session = Session::new(Config::default());
    let mut host = BufferHost::default();
    match run_source(&session, "conversio_test.fab", source, &mut host) {
        Ok(()) => Ok(host),
        Err(error) => Err(error),
    }
}

#[test]
fn textus_to_sized_numerus_accepts_in_width_value() {
    // "200" fits numerus<u8>; the program completes and `nota` prints 200.
    let host =
        run("incipit { fixum numerus<u8> v ← \"200\" ↦ numerus<u8>\n nota v }").expect("in-width conversion runs");
    assert_eq!(host.stdout_lines, vec!["200".to_owned()]);
}

#[test]
fn textus_to_sized_numerus_at_upper_boundary() {
    // "255" is the maximum value for numerus<u8>.
    let host =
        run("incipit { fixum numerus<u8> v ← \"255\" ↦ numerus<u8>\n nota v }").expect("boundary value u8 max runs");
    assert_eq!(host.stdout_lines, vec!["255".to_owned()]);
}

#[test]
fn textus_to_sized_numerus_at_lower_boundary() {
    // "0" is the minimum value for numerus<u8>.
    let host =
        run("incipit { fixum numerus<u8> v ← \"0\" ↦ numerus<u8>\n nota v }").expect("boundary value u8 min runs");
    assert_eq!(host.stdout_lines, vec!["0".to_owned()]);
}

#[test]
fn textus_to_sized_numerus_just_over_upper_boundary() {
    // "256" overflows numerus<u8> by exactly 1.
    let result = run("incipit { fixum numerus<u8> v ← \"256\" ↦ numerus<u8>\n nota v }");
    assert!(result.is_err(), "just-over-max-bounds conversion must fail");
}

#[test]
fn textus_to_sized_numerus_rejects_empty_string() {
    // Empty string cannot parse to any numerus width.
    let result = run("incipit { fixum numerus<u8> v ← \"\" ↦ numerus<u8>\n nota v }");
    assert!(result.is_err(), "empty string conversion must fail");
}

#[test]
fn textus_to_sized_numerus_rejects_non_numeric_text() {
    // Non-numeric text cannot parse to numerus.
    let result = run("incipit { fixum numerus<u8> v ← \"abc\" ↦ numerus<u8>\n nota v }");
    assert!(result.is_err(), "non-numeric text conversion must fail");
}

#[test]
fn textus_to_sized_numerus_rejects_out_of_range_overflow() {
    // SA-002: "300" overflows numerus<u8>. Rust codegen's `.parse::<u8>()`
    // fails; the runner must fail too (previously it returned Value::Int(300)).
    let result = run("incipit { fixum numerus<u8> v ← \"300\" ↦ numerus<u8>\n nota v }");
    assert!(result.is_err(), "out-of-range conversion must fail");
}

#[test]
fn textus_to_sized_numerus_rejects_negative_for_unsigned_width() {
    // "-1" cannot fit numerus<u8> (unsigned). Matches `.parse::<u8>()`.
    let result = run("incipit { fixum numerus<u8> v ← \"-1\" ↦ numerus<u8>\n nota v }");
    assert!(result.is_err(), "negative value must not fit unsigned width");
}

#[test]
fn textus_to_unsized_numerus_accepts_wide_value() {
    // Unsized numerus is i64; "300" is fine. Proves the width check is keyed to
    // the declared target width, not a blanket rejection of large values.
    let host = run("incipit { fixum numerus v ← \"300\" ↦ numerus\n nota v }").expect("i64 conversion runs");
    assert_eq!(host.stdout_lines, vec!["300".to_owned()]);
}

#[test]
fn valor_textus_to_numerus_uses_dynamic_extraction_not_text_parse() {
    let source = r#"
incipit {
    fixum textus staticText ← "5"
    fixum numerus parsed ← staticText ↦ numerus
    nota parsed

    fixum valor dynamicText ← "5"
    fixum numerus recovered ← dynamicText ↦ numerus ⇥ 0
    nota recovered
}
"#;
    let host = run(source).expect("dynamic extraction recovery runs");
    assert_eq!(host.stdout_lines, vec!["5".to_owned(), "0".to_owned()]);
}

#[test]
fn valor_numerus_to_fractus_still_widens() {
    let source = r"
incipit {
    fixum valor wide ← 7
    fixum fractus recovered ← wide ↦ fractus
    nota recovered
}
";
    let host = run(source).expect("valor numerus widens to fractus");
    assert_eq!(host.stdout_lines, vec!["7.0".to_owned()]);
}

#[test]
fn textus_to_numerus_with_recovery_yields_alternate() {
    // SA-002 parity: a failed `↦ numerus<u8>` with `⇥` recovery yields the
    // recovery value instead of erroring, mirroring Rust codegen's
    // `.parse::<u8>().unwrap_or(...)`. The recovery literal is annotated to
    // satisfy the conversion's target width.
    let host = run("incipit { fixum numerus<u8> v ← \"300\" ↦ numerus<u8> ⇥ (0 ∷ numerus<u8>)\n nota v }")
        .expect("recovery value runs");
    assert_eq!(host.stdout_lines, vec!["0".to_owned()]);
}

#[test]
fn textus_to_fractus_is_distinct_from_numerus() {
    // Proves the target type is honored: "1.5" routes to fractus, not numerus.
    let host = run("incipit { fixum fractus v ← \"1.5\" ↦ fractus\n nota v }").expect("fractus conversion runs");
    assert_eq!(host.stdout_lines, vec!["1.5".to_owned()]);
}

#[test]
fn textus_to_bivalens_is_distinct_from_numerus() {
    // Proves target routing: a non-empty textus is truthy under ↦ bivalens,
    // and crucially this *succeeds* — "x" ↦ numerus would fail to parse.
    let host = run("incipit { fixum bivalens v ← \"x\" ↦ bivalens\n nota v }").expect("bivalens conversion runs");
    // The conversion completed and produced a single diagnostic line; the exact
    // boolean rendering is not the contract under test here.
    assert_eq!(host.stdout_lines.len(), 1, "bivalens conversion emits one nota line");
}

#[test]
fn octeti_to_valor_boxes_bytes_as_numerus_lista() {
    let host = run("incipit { fixum octeti bytes ← |de ad|\n fixum valor boxed ← bytes ↦ valor\n nota boxed }")
        .expect("octeti boxing runs");
    assert_eq!(host.stdout_lines, vec!["[222, 173]".to_owned()]);
}

#[test]
fn lista_u8_assigned_to_octeti_decodes_as_utf8() {
    let source = r#"
incipit {
    varia lista<numerus<u8>> raw ← vacua
    raw.appende(104 ∷ numerus<u8>)
    raw.appende(105 ∷ numerus<u8>)
    fixum octeti bytes ← raw
    fixum textus text ← bytes ↦ textus
    nota text
}
"#;
    let host = run(source).expect("lista<numerus<u8>> assigned to octeti must decode as UTF-8");
    assert_eq!(host.stdout_lines, vec!["hi".to_owned()]);
}

#[test]
fn valor_lista_to_lista_numerus_preserves_dynamic_strictness_per_element() {
    let source = r#"
incipit {
    fixum lista<valor> raw ← ["5"]
    fixum lista<numerus> parsed ← raw ↦ lista<numerus> ⇥ [0]
    nota parsed
}
"#;
    let host = run(source).expect("dynamic list extraction recovers");
    assert_eq!(host.stdout_lines, vec!["[0]".to_owned()]);
}

#[test]
fn valor_tabula_to_tabula_numerus_preserves_dynamic_strictness_per_value() {
    let source = r#"
incipit {
    fixum tabula<textus, valor> raw ← { "count": "5" }
    fixum tabula<textus, numerus> fallback ← { "count": 0 }
    fixum tabula<textus, numerus> parsed ← raw ↦ tabula<textus, numerus> ⇥ fallback
    fixum lista<numerus> values ← parsed ↦ lista<numerus>
    nota values
}
"#;
    let host = run(source).expect("dynamic map extraction recovers");
    assert_eq!(host.stdout_lines, vec!["[0]".to_owned()]);
}

#[test]
fn textus_to_regex_is_distinct_from_numerus() {
    // Proves target routing: textus ↦ regex produces a regex value, not a parse.
    let result = run("incipit { fixum regex v ← \"\\\\d+\" ↦ regex\n nota v }");
    // A valid pattern compiles; we only assert the conversion path is taken and
    // does not surface as a numerus parse error.
    assert!(result.is_ok(), "regex conversion should succeed: {:?}", result.err());
}

/// G12: the `consentit` match verb must answer like the Rust `regex` crate's
/// `is_match` in script mode — match, non-match, and anchored patterns.
#[test]
fn regex_consentit_answers_like_rust_regex_is_match() {
    let host = run("incipit {\n\
         \x20   fixum regex v ← \"\\\\d+\" ↦ regex\n\
         \x20   nota v.consentit(\"abc123\")\n\
         \x20   nota v.consentit(\"abc\")\n\
         \x20   fixum regex anchored ← \"^Roma\" ↦ regex\n\
         \x20   nota anchored.consentit(\"Romae\")\n\
         \x20   nota anchored.consentit(\"in Roma\")\n\
         }")
    .expect("regex consentit fixture runs");
    assert_eq!(host.stdout_lines, vec!["verum", "falsum", "verum", "falsum"]);
}

/// G12: the representative Wave-3 lint pattern shapes must be expressible via
/// `consentit` — check-source retired-syntax family, deferred-`mori` grep,
/// three.js scan, href/canonical, strip-empty whitespace, manual-JSON
/// rejection. Patterns carry single backslashes (`\.`/`\s`/`\{` pass through
/// the faber string decoder unchanged).
#[test]
fn regex_consentit_handles_wave3_lint_pattern_shapes() {
    let host = run("incipit {\n\
         \x20   fixum regex threejs ← \"three\\.js\" ↦ regex\n\
         \x20   fixum regex mori ← \"mori.*deferred\" ↦ regex\n\
         \x20   fixum regex href ← \"href=.*canonical\" ↦ regex\n\
         \x20   fixum regex whitespace ← \"\\s+\" ↦ regex\n\
         \x20   fixum regex json ← \"\\{.*\\\"manual\\\"\" ↦ regex\n\
         \x20   nota threejs.consentit(\"import 'three.js'\")\n\
         \x20   nota threejs.consentit(\"threejs\")\n\
         \x20   nota mori.consentit(\"mori \\\"deferred pending\\\"\")\n\
         \x20   nota mori.consentit(\"mori done\")\n\
         \x20   nota href.consentit(\"href=\\\"/en/canonical/\\\"\")\n\
         \x20   nota href.consentit(\"href=\\\"/other/\\\"\")\n\
         \x20   nota whitespace.consentit(\"a  b\")\n\
         \x20   nota whitespace.consentit(\"ab\")\n\
         \x20   nota json.consentit(\"{\\\"manual\\\": 1}\")\n\
         \x20   nota json.consentit(\"{\\\"auto\\\": 1}\")\n\
         }")
    .expect("wave3 pattern fixture runs");
    assert_eq!(
        host.stdout_lines,
        vec!["verum", "falsum", "verum", "falsum", "verum", "falsum", "verum", "falsum", "verum", "falsum"]
    );
}

#[test]
fn dense_tensor_to_sparsa_and_back_preserves_shape_and_nonzero_count() {
    // Sparse and dense tensors share the runner's target-neutral tensor
    // carrier.  The declared type still selects sparse operations, so both
    // conversion directions must execute instead of being rejected as an
    // unsupported MIR shape.
    let source = r"
incipit {
    fixum tf32[2, 2] dense ← [[1.0, 0.0], [0.0, 2.0]] ↦ tf32[2, 2]
    fixum sf32[2, 2] sparse ← dense ↦ sf32[2, 2]
    fixum tf32[2, 2] roundtrip ← sparse ↦ tf32[2, 2]
    nota sparse.nonnihil()
    nota roundtrip.longitudo()
}
";
    let host = run(source).expect("tensor↔sparsa conversion runs in the runner");
    assert_eq!(host.stdout_lines, vec!["2".to_owned(), "2".to_owned()]);
}

#[test]
fn numeric_value_equality_promotes_integer_and_float() {
    let source = r"incipit {
    nota 1 ≈ 1.0
    nota 42.145 ≈ 42.0
}";
    let host = run(source).expect("mixed numeric equality runs in the runner");
    assert_eq!(host.stdout_lines, vec!["verum".to_owned(), "falsum".to_owned()]);
}

#[test]
fn fractus_to_numerus_truncates_without_failing() {
    // WHY: codegen emits `value as W` for `fractus ↦ numerus` (non-fallible
    // truncation, no `⇥` arm). The runner mirrors that: 1.9 ↦ numerus is 1.
    // This documents the scope boundary — only the fallible string-parse path
    // is width-enforced; truncating casts stay non-fallible (SA-002 scope).
    let host = run("incipit { fixum numerus v ← 1.9 ↦ numerus\n nota v }").expect("fractus cast runs");
    assert!(
        host.stdout_lines.iter().any(|line| line == "1"),
        "fractus truncates toward zero"
    );
}

#[test]
fn analyzed_conversion_surfaces_width_failure_as_conversion_error() {
    // WHY: the `is_conversion_failure` predicate must recognize width-overflow
    // ("conversion out of range") so a `cape` block can catch it — parity with
    // codegen's fallible parse propagating as a catchable error. We assert the
    // handler runs (program completes) and the overflowed value never leaks.
    let source = "incipit {\n  fac {\n    fixum numerus<u8> v ← \"300\" ↦ numerus<u8>\n    nota v\n  } cape err {\n    nota err.nuntius\n  }\n}";
    let host = run(source).expect("cape recovers the width failure");
    // The overflowed value must never surface on the success path.
    assert!(
        !host.stdout_lines.iter().any(|line| line == "300"),
        "overflowed value must not leak: {:?}",
        host.stdout_lines
    );
}

#[test]
fn json_to_genus_rejects_missing_required_primitive_fields() {
    let source = r#"
@ json
genus Payload {
    textus id
    numerus count
    bivalens ok
}

incipit {
    fixum json doc ← { "id": "p1" }
    fixum Payload decoded ← doc ↦ Payload
    nota decoded.id
}
"#;
    let result = run(source);
    assert!(
        result.is_err(),
        "json ↦ genus must fail when required primitive fields are missing"
    );
}

#[test]
fn valor_to_genus_keeps_missing_required_primitive_defaults() {
    let source = r#"
@ json
genus Payload {
    textus id
    numerus count
    bivalens ok
}

incipit {
    fixum valor wide ← { "id": "p1" } ↦ valor
    fixum Payload decoded ← wide ↦ Payload
    nota decoded.count
    nota decoded.ok
}
"#;
    let host = run(source).expect("broad valor extraction keeps natural defaults");
    assert_eq!(host.stdout_lines.len(), 2, "expected count and ok output");
    assert_eq!(host.stdout_lines[0], "0");
}

#[test]
fn json_genus_instans_field_renders_as_wire_textus() {
    let source = r#"
@ json
genus Event {
    instans at
}

importa ex "faber:*" faber

incipit {
    fixum valor raw ← "1979-05-27T07:32:00Z"
    fixum instans at ← raw ↦ instans
    fixum Event event ← Event { at = at }
    fixum json doc ← event ↦ json
    nota faber.json.stringify(doc)
}
"#;
    let host = run(source).expect("json genus instans conversion runs");
    assert_eq!(host.stdout_lines, vec![r#"{"at":"1979-05-27T07:32:00Z"}"#.to_owned()]);
}

#[test]
fn json_genus_wire_key_ignores_non_json_annotation_nomen() {
    let source = r#"
@ json
genus Payload {
    @ meta { nomen = "wrong" }
    @ json { nomen = "right" }
    textus value
}

incipit {
    fixum json doc ← { "right": "ok" }
    fixum Payload decoded ← doc ↦ Payload
    nota decoded.value
}
"#;
    let host = run(source).expect("json field key uses json annotation family");
    assert_eq!(host.stdout_lines, vec!["ok".to_owned()]);
}

#[test]
fn sermo_to_numerus_recovery_uses_fallback() {
    let source = r#"
incipit {
    fixum numerus value ← ad 'runtime:echo' ("non-numeric") ↦ numerus ⇥ 0
    adfirma value ≡ 0
}
"#;
    run(source).expect("sermo numerus recovery runs");
}

#[test]
fn sermo_to_vacuum_drains_runtime_echo() {
    let source = r#"
incipit {
    fixum _ value ← ad 'runtime:echo' ("salve") ↦ vacuum
    adfirma verum
}
"#;
    run(source).expect("sermo vacuum drain runs");
}

// ── CDA-U6 — `↤` conversion-directed assignment: eval order + chains ────────

#[test]
fn conversio_assign_stores_converted_value() {
    // R1 textus→numerus: `count ↤ "42"` converts before the store.
    let host = run("incipit {\n  varia numerus count ← 0\n  count ↤ \"42\"\n  nota count\n}")
        .expect("↤ runs and stores the converted value");
    assert_eq!(host.stdout_lines, vec!["42".to_owned()]);
}

#[test]
fn conversio_assign_typed_init_stores_converted_value() {
    // Typed declaration init (`fixum numerus n ↤ "42"`): convert to the
    // written type, then initialize the binding.
    let host = run("incipit {\n  fixum numerus n ↤ \"42\"\n  nota n\n}")
        .expect("typed ↤ initializer runs and stores the converted value");
    assert_eq!(host.stdout_lines, vec!["42".to_owned()]);
}

#[test]
fn conversio_assign_evaluates_source_once() {
    // The source is an effectful call; exactly one evaluation must occur.
    let source = r#"
functio src() → textus {
    nota "src"
    redde "42"
}
incipit {
    varia numerus count ← 0
    count ↤ src()
    nota count
}
"#;
    let host = run(source).expect("↤ with effectful source runs");
    assert_eq!(host.stdout_lines, vec!["src".to_owned(), "42".to_owned()]);
}

#[test]
fn conversio_assign_evaluates_destination_index_once() {
    // Indexed destination: the index expression runs exactly once (receiver
    // and index evaluated once, like ordinary `←`).
    let source = r#"
functio idx() → numerus {
    nota "idx"
    redde 0
}
incipit {
    varia lista<numerus> xs ← [0, 0]
    xs[idx()] ↤ "7"
    nota xs[0]
}
"#;
    let host = run(source).expect("↤ with effectful destination index runs");
    assert_eq!(host.stdout_lines, vec!["idx".to_owned(), "7".to_owned()]);
}

#[test]
fn conversio_assign_successful_conversion_ignores_recovery_value() {
    // Conversion succeeds, so the stored value is the converted value (the
    // `⇥` recovery is never the result of a successful conversion).
    let source = r#"
incipit {
    varia numerus count ← 0
    count ↤ "42" ⇥ 0
    nota count
}
"#;
    let host = run(source).expect("↤ with unused recovery runs");
    assert_eq!(host.stdout_lines, vec!["42".to_owned()]);
}

#[test]
fn conversio_assign_recovery_value_used_once_on_failure() {
    // Conversion fails, so the fallback's value is stored exactly once.
    // (CTO-3: the side-effecting CALL fallback is deferred — it runs exactly
    // once, here, on the failure path; the successful-conversion case proves
    // it does NOT run on success.)
    let source = r#"
functio fb() → numerus {
    nota "fb"
    redde 0
}
incipit {
    varia numerus count ← 0
    count ↤ "x" ⇥ fb()
    nota count
}
"#;
    let host = run(source).expect("↤ with failing conversion + recovery runs");
    assert_eq!(host.stdout_lines, vec!["fb".to_owned(), "0".to_owned()]);
}

#[test]
fn conversio_assign_successful_conversion_does_not_run_call_fallback() {
    // CTO-3 / AC4 parity: the side-effecting `⇥` CALL fallback runs ONLY on
    // conversion failure. On success, `fb()` must NOT execute (no "fb" in the
    // output) — the stored value is the converted value.
    let source = r#"
functio fb() → numerus {
    nota "fb"
    redde 0
}
incipit {
    varia numerus count ← 0
    count ↤ "42" ⇥ fb()
    nota count
}
"#;
    let host = run(source).expect("↤ with successful conversion + call recovery runs");
    assert_eq!(
        host.stdout_lines,
        vec!["42".to_owned()],
        "the fallback call must not run when the conversion succeeds"
    );
}

#[test]
fn conversio_expr_successful_conversion_does_not_run_call_fallback() {
    // CTO-3 parity on the shared `↦` route: `lower_conversio` owns both the
    // `↤` and `↦` recovery lowering, so the same fallback-not-run-on-success
    // invariant holds for the expression route.
    let source = r#"
functio fb() → numerus {
    nota "fb"
    redde 0
}
incipit {
    fixum numerus value ← "42" ↦ numerus ⇥ fb()
    nota value
}
"#;
    let host = run(source).expect("↦ with successful conversion + call recovery runs");
    assert_eq!(
        host.stdout_lines,
        vec!["42".to_owned()],
        "the ↦-route fallback call must not run when the conversion succeeds"
    );
}

#[test]
fn conversio_assign_chain_converts_inner_to_outer_and_stores_both() {
    // Right-associated chain (AC6): inner `b ↤ "42"` converts and stores to b,
    // then its result (the stored value) feeds the outer conversion into a.
    // Both links are R1 textus→numerus; the outer link is identity after the
    // inner conversion.
    let source = r#"
incipit {
    varia numerus a ← 0
    varia numerus b ← 0
    a ↤ b ↤ "42"
    nota a
    nota b
}
"#;
    let host = run(source).expect("↤ chain runs inner-to-outer");
    assert_eq!(host.stdout_lines, vec!["42".to_owned(), "42".to_owned()]);
}

#[test]
fn conversio_assign_accepts_field_place() {
    // Nontrivial destination place: a struct field of a mutable binding.
    let source = r#"
genus Persona { textus nomen numerus aetas }
functio muta() → numerus {
    varia Persona p ← Persona { nomen = "Ada", aetas = 36 }
    p.aetas ↤ "37"
    redde p.aetas
}
incipit {
    nota muta()
}
"#;
    let host = run(source).expect("↤ field place runs");
    assert_eq!(host.stdout_lines, vec!["37".to_owned()]);
}
