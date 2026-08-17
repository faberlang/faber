//! Leaf-owned tests for the pretty-v1 layout engine.
//!
//! Unit 1 (`…-s3-engine-blocks-indent`): one statement per line, 4-space
//! indent, braces attached, non-cuddled `si`/`sin`/`secus` and `fac`/`cape`
//! chains, compact one-statement arms that fit, compact empty blocks.

use crate::forma::compile_pretty_author_str;

/// Run the full pretty-v1 pipeline and return the emitted code.
fn pretty(source: &str) -> String {
    let result = compile_pretty_author_str("engine-local.fab", source);
    assert!(
        result.success(),
        "pretty-v1 failed: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| format!("{:?}: {:?}", d.code, d.issue()))
            .collect::<Vec<_>>()
    );
    result.output.expect("output present").code
}

/// Parse the emitted code back (guard against non-reparsing output).
fn assert_reparses(code: &str) {
    let lex = crate::lexer::lex(code);
    assert!(lex.success(), "lex errors: {:?}", lex.errors);
    let parse = crate::parser::parse(lex);
    assert!(parse.success(), "parse errors: {:?}", parse.errors);
}

#[test]
fn block_body_one_statement_per_line_four_space_indent() {
    let source = "functio f() → numerus {\n    fixum numerus a ← 1\n    fixum numerus b ← 2\n    si a ≻ b {\n        redde a\n    }\n    redde b\n}\n";
    let output = pretty(source);
    let expected = "functio f() → numerus {\n    fixum numerus a ← 1\n    fixum numerus b ← 2\n    si a ≻ b {\n        redde a\n    }\n    redde b\n}\n";
    assert_eq!(output, expected);
    assert_reparses(&output);
}

#[test]
fn compact_source_expands_to_one_statement_per_line() {
    let source = "functio f() → numerus {\n    si a ≻ b { redde a } redde b\n}\n";
    let output = pretty(source);
    let expected = "functio f() → numerus {\n    si a ≻ b {\n        redde a\n    }\n    redde b\n}\n";
    assert_eq!(output, expected);
    assert_reparses(&output);
}

#[test]
fn si_sin_secus_chain_is_non_cuddled() {
    let source = "functio f() → numerus {\n    si code ≡ 1 { redde prima() } sin code ≡ 2 { redde secunda() } secus { redde fallback() }\n}\n";
    let output = pretty(source);
    let expected = "functio f() → numerus {\n    si code ≡ 1 {\n        redde prima()\n    }\n    sin code ≡ 2 {\n        redde secunda()\n    }\n    secus {\n        redde fallback()\n    }\n}\n";
    assert_eq!(output, expected);
    // Never `} sin … {` on one line.
    assert!(!output.contains("} sin"), "cuddled sin: {output}");
    assert!(!output.contains("} secus"), "cuddled secus: {output}");
    assert_reparses(&output);
}

#[test]
fn fac_cape_handler_is_non_cuddled() {
    let source = "functio f() {\n    fac { iace \"boom\" } cape err { nota err.nuntius }\n}\n";
    let output = pretty(source);
    let expected =
        "functio f() {\n    fac {\n        iace \"boom\"\n    }\n    cape err {\n        nota err.nuntius\n    }\n}\n";
    assert_eq!(output, expected);
    assert!(!output.contains("} cape"), "cuddled cape: {output}");
    assert_reparses(&output);
}

#[test]
fn si_cape_is_non_cuddled() {
    let source = "functio f() {\n    si a { nota \"x\" } cape err { scribe err }\n}\n";
    let output = pretty(source);
    let expected =
        "functio f() {\n    si a {\n        nota \"x\"\n    }\n    cape err {\n        scribe err\n    }\n}\n";
    assert_eq!(output, expected);
    assert_reparses(&output);
}

#[test]
fn compact_ergo_arm_stays_compact_when_it_fits() {
    let source = "functio f() → numerus {\n    si x ≻ 0 ergo redde 1\n    redde 0\n}\n";
    let output = pretty(source);
    let expected = "functio f() → numerus {\n    si x ≻ 0 ergo redde 1\n    redde 0\n}\n";
    assert_eq!(output, expected);
    assert_reparses(&output);
}

// ---------------------------------------------------------------------------
// Glyph products (`·` `×` `⊗` `⊙`) round-trip at multiplicative precedence
// ---------------------------------------------------------------------------

/// `a + b · c` stays parenthesization-free: additive (10) is weaker than the
/// multiplicative glyph (11), so the formatter must not add parens.
#[test]
fn glyph_product_binds_tighter_than_additive_without_parens() {
    let source = "functio f() → numerus {\n    redde a + b · c\n}\n";
    let output = pretty(source);
    assert_eq!(output, source);
    assert_reparses(&output);
}

/// A parenthesized additive lhs under a glyph keeps its parens on re-emit.
#[test]
fn glyph_product_preserves_needed_parens_on_lhs() {
    let source = "functio f() → numerus {\n    redde (a + b) · c\n}\n";
    let output = pretty(source);
    assert_eq!(output, source);
    assert_reparses(&output);
}

/// Left-associative glyph chains round-trip. The pretty formatter
/// parenthesizes equal-precedence children (unambiguous reparse over
/// associativity minimality), so the canonical output is the re-parenthesized
/// chain — and it must re-parse to the same left-associative tree.
#[test]
fn glyph_product_chain_round_trips() {
    let source = "functio f() → numerus {\n    redde a · b × c\n}\n";
    let output = pretty(source);
    let expected = "functio f() → numerus {\n    redde (a · b) × c\n}\n";
    assert_eq!(output, expected);
    assert_reparses(&output);
}

/// All four glyph spellings survive pretty re-emission.
#[test]
fn all_glyph_product_spellings_round_trip() {
    for spelling in ["·", "×", "⊗", "⊙"] {
        let source = format!("functio f() → numerus {{\n    redde a {spelling} b\n}}\n");
        let output = pretty(&source);
        assert_eq!(output, source, "glyph {spelling}");
        assert_reparses(&output);
    }
}

// ---------------------------------------------------------------------------
// Postfix gradient selection (`call(...) ∇` / `call(...) ∇ [places]`)
// ---------------------------------------------------------------------------

/// The bare postfix gradient round-trips with one space before `∇`.
#[test]
fn bare_gradient_round_trips() {
    let source = "functio f() {\n    redde loss(x, y) ∇\n}\n";
    let output = pretty(source);
    assert_eq!(output, source);
    assert_reparses(&output);
}

/// The selected form round-trips in the canonical spaced form: one space on
/// both sides of `∇`.
#[test]
fn selected_gradient_round_trips() {
    let source = "functio f() {\n    redde loss(x, y) ∇ [x, y]\n}\n";
    let output = pretty(source);
    assert_eq!(output, source);
    assert_reparses(&output);
}

/// Whitespace-insensitive input `∇[x]` canonicalizes to the spaced form.
#[test]
fn gradient_selector_bracket_canonicalizes_to_spaced_form() {
    let source = "functio f() {\n    redde loss(x, y) ∇[x]\n}\n";
    let output = pretty(source);
    let expected = "functio f() {\n    redde loss(x, y) ∇ [x]\n}\n";
    assert_eq!(output, expected);
    assert_reparses(&output);
}

/// Ordinary postfix continuation applies after the gradient result and
/// round-trips without re-parenthesization.
#[test]
fn gradient_postfix_continuation_round_trips() {
    let source = "functio f() {\n    redde loss(x, y) ∇ [x].magnitudo\n}\n";
    let output = pretty(source);
    assert_eq!(output, source);
    assert_reparses(&output);
}

#[test]
fn ergo_chain_stays_compact() {
    let source =
        "functio f() → numerus {\n    si x ≡ 1 ergo redde 1\n    sin x ≡ 2 ergo redde 2\n    secus ergo redde 3\n}\n";
    let output = pretty(source);
    let expected =
        "functio f() → numerus {\n    si x ≡ 1 ergo redde 1\n    sin x ≡ 2 ergo redde 2\n    secus ergo redde 3\n}\n";
    assert_eq!(output, expected);
    assert_reparses(&output);
}

#[test]
fn ergo_arm_expands_to_block_when_the_arm_exceeds_the_soft_width() {
    let cond = "this_is_a_very_long_condition_name_that_keeps_going_and_going";
    let value = "someVeryLongReturnValueIdentifierThatWillNotFit";
    let source = format!("functio f() → numerus {{\n    si {cond} ergo redde {value}\n    redde 0\n}}\n");
    let output = pretty(&source);
    let expected =
        format!("functio f() → numerus {{\n    si {cond} {{\n        redde {value}\n    }}\n    redde 0\n}}\n");
    assert_eq!(output, expected);
    assert_reparses(&output);
}

#[test]
fn empty_blocks_stay_compact() {
    let source = "functio f() → vacuum { }\n";
    let output = pretty(source);
    assert_eq!(output, "functio f() → vacuum {}\n");
    assert_reparses(&output);
}

#[test]
fn empty_si_block_stays_compact() {
    // `Ident { }` after a bare-identifier condition is consumed as a
    // zero-field typed constructor (`b {}`), so the empty si block is
    // expressed with a keyword-literal condition, which parses unambiguously.
    let source = "functio f() → numerus {\n    si verum { }\n    redde 0\n}\n";
    let output = pretty(source);
    let expected = "functio f() → numerus {\n    si verum {}\n    redde 0\n}\n";
    assert_eq!(output, expected);
    assert_reparses(&output);
}

#[test]
fn nested_blocks_indent_by_four_each_level() {
    let source = "functio f() → numerus {\n    si a {\n        si b {\n            si c {\n                redde 1\n            }\n        }\n    }\n    redde 0\n}\n";
    let output = pretty(source);
    assert_eq!(output, source);
    assert!(output.contains("            redde 1"), "deep indent: {output}");
    assert_reparses(&output);
}

#[test]
fn dum_loop_block_form_is_non_cuddled_with_cape() {
    let source = "functio f() {\n    dum a { perge } cape err { scribe err }\n}\n";
    let output = pretty(source);
    let expected = "functio f() {\n    dum a {\n        perge\n    }\n    cape err {\n        scribe err\n    }\n}\n";
    assert_eq!(output, expected);
    assert_reparses(&output);
}

#[test]
fn itera_block_form_is_structural() {
    let source = "functio f() {\n    itera ex list fixum item { scribe item }\n}\n";
    let output = pretty(source);
    let expected = "functio f() {\n    itera ex list fixum item {\n        scribe item\n    }\n}\n";
    assert_eq!(output, expected);
    assert_reparses(&output);
}

#[test]
fn elige_cases_and_cape_are_non_cuddled() {
    let source = "functio f() → numerus {\n    elige x {\n        casu 1 ergo redde 10\n        ceterum ergo redde 0\n    } cape err { scribe err }\n}\n";
    let output = pretty(source);
    let expected = "functio f() → numerus {\n    elige x {\n        casu 1 ergo redde 10\n        ceterum ergo redde 0\n    }\n    cape err {\n        scribe err\n    }\n}\n";
    assert_eq!(output, expected);
    assert_reparses(&output);
}

#[test]
fn interna_annotation_round_trips() {
    let source = "@ interna\nfunctio hidden() → vacuum { }\n";
    let output = pretty(source);
    assert!(output.contains("@ interna"), "missing @ interna: {output}");
    assert_reparses(&output);
}

#[test]
fn interna_member_annotation_round_trips() {
    let source = "genus Demo {\n    @ interna\n    functio hidden() → vacuum { }\n}\n";
    let output = pretty(source);
    assert!(output.contains("@ interna"), "missing @ interna: {output}");
    assert_reparses(&output);
}

#[test]
fn engine_is_idempotent_on_block_fixtures() {
    let source = "functio f() → numerus {\n    si code ≡ 1 { redde prima() } sin code ≡ 2 { redde secunda() } secus { redde fallback() }\n    fac { iace \"boom\" } cape err { nota err.nuntius }\n    si x ≻ 0 ergo redde 1\n    redde 0\n}\n";
    let first = pretty(source);
    assert_reparses(&first);
    let second = pretty(&first);
    assert_eq!(first, second, "pretty(pretty(x)) must equal pretty(x)");
}
