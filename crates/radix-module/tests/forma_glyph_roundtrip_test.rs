//! Author-format round-trip for the tensor-glyph product operators
//! (`·` `×` `⊗` `⊙`): author(author(x)) == author(x), the first pass
//! preserves the glyph spellings, and every pass re-lexes and reparses.

use radix_module::forma::test_gate::{assert_author_idempotent, author_format_once};

#[test]
fn forma_author_glyph_products_idempotent() {
    let source = "functio f() → numerus {\n    redde a · b\n}\n";
    let first = assert_author_idempotent("glyph-dot.fab", source).expect("dot glyph round-trip");
    assert!(first.contains('·'), "dot glyph must survive: {first}");
}

#[test]
fn forma_author_glyph_chain_idempotent() {
    let source = "functio f() → numerus {\n    redde a · b × c ⊗ d ⊙ e\n}\n";
    let first = assert_author_idempotent("glyph-chain.fab", source).expect("glyph chain round-trip");
    for spelling in ['·', '×', '⊗', '⊙'] {
        assert!(first.contains(spelling), "glyph {spelling} must survive: {first}");
    }
}

#[test]
fn forma_author_glyph_mixed_with_arithmetic_preserves_precedence() {
    let source = "functio f() → numerus {\n    redde a + b · c\n}\n";
    let first = assert_author_idempotent("glyph-precedence.fab", source).expect("precedence round-trip");
    // The additive root stays unparenthesized: the glyph binds tighter.
    assert!(first.contains("a + b · c"), "precedence must survive: {first}");
}

#[test]
fn forma_author_glyph_preserves_explicit_parens() {
    let source = "functio f() → numerus {\n    redde (a + b) · c\n}\n";
    let output = author_format_once("glyph-parens.fab", source).expect("parens round-trip");
    assert!(output.contains("(a + b) · c"), "needed parens must survive: {output}");
}
