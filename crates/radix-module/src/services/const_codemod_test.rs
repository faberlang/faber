//! Span-based `static` → `const` migration for module constants (st1 R18).
//!
//! The codemod swaps the one keyword token of every module `static` for the
//! reader pack's `fixum` spelling and leaves every other byte alone. It is
//! idempotent, keeps comments, annotations, and layout, leaves genus
//! `generis` fields untouched, and refuses a source it cannot parse.

use super::convert_static_to_const;
use crate::locale::LocalePack;

fn pack(locale: &str) -> LocalePack {
    LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path(locale))
        .unwrap_or_else(|err| panic!("installed {locale} pack should load: {err:?}"))
}

const EN: &str = "+++\nlocale = \"en\"\n+++\n# header comment\n@ public\nstatic int LIMIT = 4 * 1024\n\nstatic string NAME = \"ab\" +\n    \"cd\"\n# between\nstatic list<int> XS = [1, LIMIT, 3]\nstatic int SUM = comptime {\n    return 1 + 2\n}\nstatic string NUNTIUS = embed \"nuntius.txt\"\n\nclass Punctum {\n    static int Z = 3\n    var int x\n}\n\nfn f() → int {\n    const int local = 1\n    return local + LIMIT\n}\n";

#[test]
fn en_swaps_every_module_static_and_nothing_else() {
    let result = convert_static_to_const(&pack("en"), EN).expect("codemod");
    assert_eq!(result.converted, 5);
    assert!(
        result
            .source
            .contains("@ public\nconst int LIMIT = 4 * 1024\n"),
        "{}",
        result.source
    );
    assert!(
        result
            .source
            .contains("const string NAME = \"ab\" +\n    \"cd\"\n# between\n"),
        "{}",
        result.source
    );
    assert!(
        result
            .source
            .contains("const int SUM = comptime {\n    return 1 + 2\n}\n"),
        "{}",
        result.source
    );
    assert!(
        result
            .source
            .contains("const string NUNTIUS = embed \"nuntius.txt\"\n"),
        "{}",
        result.source
    );
    // The genus `generis` field is a field modifier, never a module static.
    assert!(
        result
            .source
            .contains("class Punctum {\n    static int Z = 3\n"),
        "{}",
        result.source
    );
    assert!(result.source.contains("    const int local = 1\n"), "{}", result.source);
}

#[test]
fn output_is_byte_identical_except_keyword_tokens() {
    let result = convert_static_to_const(&pack("en"), EN).expect("codemod");
    assert_eq!(result.source.len() + result.converted, EN.len());
    // Undo the swap at the five module sites: every other byte matches.
    let restored = result
        .source
        .replacen("const int LIMIT", "static int LIMIT", 1)
        .replacen("const string NAME", "static string NAME", 1)
        .replacen("const list<int> XS", "static list<int> XS", 1)
        .replacen("const int SUM", "static int SUM", 1)
        .replacen("const string NUNTIUS", "static string NUNTIUS", 1);
    assert_eq!(restored, EN);
}

#[test]
fn codemod_is_idempotent() {
    let once = convert_static_to_const(&pack("en"), EN).expect("codemod");
    let twice = convert_static_to_const(&pack("en"), &once.source).expect("codemod");
    assert_eq!(twice.converted, 0);
    assert_eq!(twice.source, once.source);
}

#[test]
fn latin_file_gets_fixum() {
    let source = "generis numerus LIMES = 4096\ngenus Punctum {\n    generis numerus Z = 3\n    varia numerus x\n}\n";
    let result = convert_static_to_const(&pack("la"), source).expect("codemod");
    assert_eq!(result.converted, 1);
    assert_eq!(
        result.source,
        "fixum numerus LIMES = 4096\ngenus Punctum {\n    generis numerus Z = 3\n    varia numerus x\n}\n"
    );
}

#[test]
fn frontmatter_bytes_are_preserved() {
    let result = convert_static_to_const(&pack("en"), EN).expect("codemod");
    assert!(
        result
            .source
            .starts_with("+++\nlocale = \"en\"\n+++\n# header comment\n")
    );
}

#[test]
fn source_without_statics_is_returned_unchanged() {
    let source = "const int A = 1\nfn f() → int { return A }\n";
    let result = convert_static_to_const(&pack("en"), source).expect("codemod");
    assert_eq!(result.converted, 0);
    assert_eq!(result.source, source);
}

#[test]
fn unparsable_source_is_refused_not_guessed() {
    assert!(convert_static_to_const(&pack("en"), "static int = 4\n").is_err());
    assert!(convert_static_to_const(&pack("en"), "static int X = 4\nfn (\n").is_err());
}
