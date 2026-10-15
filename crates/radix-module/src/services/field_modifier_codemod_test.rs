//! Span-based `varia` insertion for unmarked genus fields (DECISIONS D16.4(1)).
//!
//! The codemod inserts the reader pack's `varia` spelling before every genus
//! field that lacks a modifier and leaves every other byte alone. It is
//! idempotent, keeps comments and layout, and leaves discretio variant and
//! union member fields untouched (F7 held).

use super::insert_field_modifiers;
use crate::locale::LocalePack;

fn pack(locale: &str) -> LocalePack {
    LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path(locale))
        .unwrap_or_else(|err| panic!("installed {locale} pack should load: {err:?}"))
}

/// Remove every inserted `word ` from `output`; what is left must equal the input.
fn strip_inserted(output: &str, word: &str) -> String {
    output.replace(&format!("{word} "), "")
}

const EN: &str = "+++\nlocale = \"en\"\n+++\n# header comment\n@ public\nclass Punctum {\n    # doc for x\n    int  x\n    let int y\n    static int Z = 3\n    @ private\n    list<int> xs\n    string ∪ none label optional\n\n    fn norm() → int {\n        let int local ← 1\n        return self.x + local\n    }\n}\n\nunion Figura {\n    Circulus { float radius }\n}\n";

#[test]
fn en_inserts_mut_before_unmarked_fields_only() {
    let result = insert_field_modifiers(&pack("en"), EN).expect("codemod");
    assert_eq!(result.inserted, 3);
    assert!(result.source.contains("    mut int  x\n"), "{}", result.source);
    assert!(result.source.contains("    let int y\n"), "{}", result.source);
    assert!(result.source.contains("    static int Z = 3\n"), "{}", result.source);
    assert!(
        result
            .source
            .contains("    @ private\n    mut list<int> xs\n"),
        "{}",
        result.source
    );
    assert!(
        result
            .source
            .contains("    mut string ∪ none label optional\n"),
        "{}",
        result.source
    );
    assert!(result.source.contains("        let int local ← 1\n"), "{}", result.source);
    assert!(result.source.contains("    Circulus { float radius }\n"), "{}", result.source);
}

#[test]
fn output_is_byte_identical_except_inserted_keywords() {
    let result = insert_field_modifiers(&pack("en"), EN).expect("codemod");
    // Every inserted span is exactly `var ` and nothing else moved.
    assert_eq!(result.source.len(), EN.len() + result.inserted * "var ".len());
    assert_eq!(strip_inserted(&result.source, "mut"), EN);
}

#[test]
fn codemod_is_idempotent() {
    let once = insert_field_modifiers(&pack("en"), EN).expect("codemod");
    let twice = insert_field_modifiers(&pack("en"), &once.source).expect("codemod");
    assert_eq!(twice.inserted, 0);
    assert_eq!(twice.source, once.source);
}

#[test]
fn latin_file_gets_varia() {
    let source = "genus Punctum {\n    numerus x\n    sit numerus y\n}\n";
    let result = insert_field_modifiers(&pack("la"), source).expect("codemod");
    assert_eq!(result.inserted, 1);
    assert_eq!(result.source, "genus Punctum {\n    varia numerus x\n    sit numerus y\n}\n");
}

#[test]
fn unparsable_source_is_refused_not_guessed() {
    assert!(insert_field_modifiers(&pack("la"), "genus Punctum {\n    numerus\n").is_err());
}
