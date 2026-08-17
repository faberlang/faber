//! Author-mode emit: comment preservation and idempotency.

use radix_module::forma::compile_author_str;
use radix_module::lexer::lex;
use radix_module::parser::parse;

fn parse_program(source: &str) -> (radix_module::syntax::Program, radix_module::lexer::Interner) {
    let result = parse(lex(source));
    assert!(result.success(), "{:?}", result.errors);
    let program = result.program.expect("program");
    (program, result.interner)
}

fn format_author(source: &str) -> String {
    let result = compile_author_str("test.fab", source);
    assert!(result.success(), "{:?}", result.diagnostics);
    result.output.expect("output").code
}

fn format_author_twice(source: &str) -> (String, String) {
    let first = format_author(source);
    let second = format_author(&first);
    (first, second)
}

#[test]
fn three_leading_comments_and_blank_line_before_functio() {
    let source = "# one\n# two\n# three\n\nfunctio f() → vacuum { }\n";
    let output = format_author(source);

    assert!(output.contains("# one"), "missing first comment:\n{output}");
    assert!(output.contains("# two"), "missing second comment:\n{output}");
    assert!(output.contains("# three"), "missing third comment:\n{output}");

    let one_pos = output.find("# one").expect("one");
    let two_pos = output.find("# two").expect("two");
    let three_pos = output.find("# three").expect("three");
    let functio_pos = output.find("functio f()").expect("functio");
    assert!(one_pos < two_pos && two_pos < three_pos && three_pos < functio_pos);

    let between = &output[three_pos..functio_pos];
    assert!(between.contains("\n\n"), "expected blank line before decl, got:\n{between:?}");
}

#[test]
fn author_emit_is_idempotent_on_comment_fixture() {
    let source = "# one\n# two\n# three\n\nfunctio f() → vacuum { }\n";
    let (first_code, second_code) = format_author_twice(source);

    assert_eq!(first_code, second_code, "author(author(x)) must equal author(x)");
}

#[test]
fn author_preserves_inner_block_comment() {
    let source = "functio f() → vacuum {\n  # inner\n  tacet\n}\n";
    let output = format_author(source);
    assert!(output.contains("# inner"), "inner comment lost:\n{output}");
}

#[test]
fn author_emits_method_calls_without_redundant_parens() {
    let source = "incipit {\n  fixum numerus n ← s.longitudo()\n  nota n\n}\n";
    let output = format_author(source);
    assert!(
        output.contains("s.longitudo()"),
        "method call should stay dotted, got:\n{output}"
    );
    assert!(
        !output.contains("(s.longitudo)"),
        "must not wrap receiver.method in call parens:\n{output}"
    );
}

#[test]
fn author_emits_ergo_keyword_for_statement_arms() {
    let source = "functio f(numerus x) → numerus {\n  si x ≡ 0 ergo redde 0\n  redde x\n}\n";
    let output = format_author(source);
    assert!(output.contains("≡ 0 ergo redde"), "statement arms use ergo:\n{output}");
    assert!(
        !output.contains('∴'),
        "statement arms must not emit clausura glyph ∴:\n{output}"
    );
}

#[test]
fn author_moves_si_continuations_to_line_start() {
    let source = "incipit {\n  si a { nota \"A\" } sin b { nota \"B\" } secus { nota \"F\" }\n}\n";
    let output = format_author(source);

    assert!(
        output.contains(
            "    si a {\n        nota \"A\"\n    }\n    sin b {\n        nota \"B\"\n    }\n    secus {\n        nota \"F\"\n    }\n"
        ),
        "sin/secus continuations must start their own lines:\n{output}"
    );
    assert!(!output.contains("} sin "), "sin must not attach to }}:\n{output}");
    assert!(!output.contains("} secus "), "secus must not attach to }}:\n{output}");
}

#[test]
fn author_moves_cape_continuations_to_line_start() {
    let source = "incipit {\n  fac { iace \"boom\" } cape err { nota err.nuntius }\n}\n";
    let output = format_author(source);

    assert!(
        output.contains("    fac {\n        iace \"boom\"\n    }\n    cape err {\n        nota err.nuntius\n    }\n"),
        "cape continuation must start its own line:\n{output}"
    );
    assert!(!output.contains("} cape "), "cape must not attach to }}:\n{output}");
}

#[test]
fn author_emits_sponte_after_parameter_name() {
    let source = "functio f(textus infimum sponte) → vacuum { }\n";
    let output = format_author(source);
    assert!(
        output.contains("textus infimum sponte"),
        "sponte must follow the parameter name:\n{output}"
    );
    assert!(!output.contains("sponte textus"), "sponte must not prefix the type:\n{output}");
}

#[test]
fn author_emits_ex_fields_with_commas() {
    let source = "incipit {\n  ex p fixum nomen, aetas\n}\n";
    let output = format_author(source);
    assert!(
        output.contains("ex p fixum nomen, aetas"),
        "ex fields must be comma-separated:\n{output}"
    );
}

#[test]
fn author_emits_canonical_ad_with_conversio() {
    let source = "incipit {\n  fixum textus t ← ad 'cap:op' (\"x\") ↦ textus\n}\n";
    let output = format_author(source);
    assert!(
        output.contains("ad 'cap:op'"),
        "canonical expression ad must be preserved:\n{output}"
    );
    assert!(output.contains("↦ textus"), "canonical conversio must be preserved:\n{output}");
}

#[test]
fn author_multiline_literals_do_not_gain_block_indent() {
    let source = "incipit {\n  fixum _ m ← «line one\nstill line one»\n}\n";
    let (first_code, second_code) = format_author_twice(source);
    assert_eq!(first_code, second_code, "multiline guillemet literal must be idempotent");
    assert!(
        first_code.contains("«line one\nstill line one»"),
        "literal interior must stay unindented:\n{first_code}"
    );
}

#[test]
fn author_preserves_blank_lines_inside_multiline_literals() {
    let source = "incipit {\n  fixum _ m ← «alpha\n\n\nomega»\n}\n";
    let (first_code, second_code) = format_author_twice(source);

    assert_eq!(first_code, second_code, "multiline literal blank lines must be idempotent");
    assert!(
        first_code.contains("«alpha\n\n\nomega»"),
        "literal interior blank lines must not be collapsed:\n{first_code}"
    );
}

#[test]
fn author_preserves_sugar_futura_spelling() {
    let source = "@ futura\nfunctio f() → vacuum { }\n";
    let output = format_author(source);
    assert!(
        output.contains("@ futura\n") || output.contains("@ futura\r\n"),
        "sugar @ futura must stay unbraced:\n{output}"
    );
    assert!(
        !output.contains("@ futura {"),
        "author mode must not canonicalize sugar futura:\n{output}"
    );
}

#[test]
fn author_preserves_braced_futura_spelling() {
    let source = "@ futura { }\nfunctio f() → vacuum { }\n";
    let output = format_author(source);
    assert!(output.contains("@ futura { }"), "braced input must stay braced:\n{output}");
}

#[test]
fn author_preserves_implendum_method_posture() {
    let source = "implendum Streamer {\n  functio next() fient → numerus ⇥ textus\n}\n";
    let output = format_author(source);
    assert!(
        output.contains("functio next() fient → numerus ⇥ textus"),
        "implendum method posture must be preserved:\n{output}"
    );
}

#[test]
fn author_preserves_failable_promissum_type_surface() {
    let source = "functio f(promissum<numerus ⇥ textus> future) → vacuum {\n  tacet\n}\n";
    let output = format_author(source);
    assert!(
        output.contains("promissum<numerus ⇥ textus> future"),
        "failable promissum type must round-trip:\n{output}"
    );
}

#[test]
fn author_direct_emit_matches_compile_path() {
    let source = "# lead\nnota \"hello\"\n";
    let (program, interner) = parse_program(source);
    let latin = radix_module::locale::latin_locale_pack();
    let surface = radix_module::locale::KeywordSurface::new(&latin);
    let direct =
        radix_module::forma::emit_author(&program, &surface, &interner, &radix_module::forma::FormatOptions::author());
    let via_compile = compile_author_str("test.fab", source);
    assert!(via_compile.success(), "{:?}", via_compile.diagnostics);
    assert_eq!(direct.code, via_compile.output.expect("output").code);
}

// ── Conversion-directed assignment (↤, U+21A4) round-trip ───────────────────

#[test]
fn author_round_trips_conversio_assign_expression() {
    let source = "incipit {\n  fixum textus t\n  t ↤ \"5\"\n}\n";
    let output = format_author(source);
    assert!(output.contains(" ↤ "), "↤ operator lost or rewritten:\n{output}");
    assert!(!output.contains(" ← "), "must never rewrite ↤ to ←:\n{output}");
    assert!(!output.contains(" ↦ "), "must never rewrite ↤ to ↦:\n{output}");
    parse_program(&output); // reparse proof
    let (first, second) = format_author_twice(source);
    assert_eq!(first, second, "author(author(x)) must equal author(x)");
}

#[test]
fn author_round_trips_conversio_assign_typed_init() {
    let source = "incipit {\n  fixum numerus n ↤ \"42\"\n}\n";
    let output = format_author(source);
    assert!(
        output.contains("fixum numerus n ↤"),
        "typed ↤ initializer must be preserved:\n{output}"
    );
    assert!(!output.contains("← n ↤"), "must not duplicate the binding:\n{output}");
    parse_program(&output);
    let (first, second) = format_author_twice(source);
    assert_eq!(first, second, "author(author(x)) must equal author(x)");
}

#[test]
fn author_round_trips_conversio_assign_recovery() {
    let source = "incipit {\n  fixum numerus n\n  n ↤ \"x\" ⇥ 0\n}\n";
    let output = format_author(source);
    assert!(output.contains(" ↤ "), "↤ operator lost or rewritten:\n{output}");
    assert!(output.contains(" ⇥ 0"), "⇥ recovery must be preserved:\n{output}");
    parse_program(&output);
    let (first, second) = format_author_twice(source);
    assert_eq!(first, second, "author(author(x)) must equal author(x)");
}

#[test]
fn author_preserves_ordinary_assign_spelling() {
    let source = "incipit {\n  fixum numerus n\n  n ← 5\n}\n";
    let output = format_author(source);
    assert!(output.contains(" ← "), "ordinary ← must stay ←:\n{output}");
    assert!(!output.contains(" ↤ "), "ordinary ← must not become ↤:\n{output}");
    parse_program(&output);
}

#[test]
fn author_round_trips_right_associated_conversio_chain_with_recovery() {
    let source = "incipit {\n  fixum numerus a\n  fixum numerus b\n  fixum numerus c\n  a ↤ b ↤ c ⇥ 0\n}\n";
    let output = format_author(source);
    assert!(output.contains(" ↤ "), "chain operator lost:\n{output}");
    assert!(output.contains(" ⇥ 0"), "nearest-↤ recovery lost:\n{output}");
    parse_program(&output);
    let (first, second) = format_author_twice(source);
    assert_eq!(first, second, "author(author(x)) must equal author(x)");
}
