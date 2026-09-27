//! The `faber field-modifier-codemod` seam resolves each file's reader pack.

use super::codemod_field_modifiers;
use std::path::Path;

#[test]
fn en_frontmatter_file_gets_var() {
    let source = "+++\nlocale = \"en\"\n+++\nclass Punctum {\n    int x\n}\n";
    let result =
        codemod_field_modifiers(Path::new("punctum.fab"), source).expect("codemod accepted");
    assert_eq!(result.inserted, 1);
    assert_eq!(
        result.source,
        "+++\nlocale = \"en\"\n+++\nclass Punctum {\n    var int x\n}\n"
    );
}

#[test]
fn la_frontmatter_file_gets_varia() {
    let source = "+++\nlocale = \"la\"\n+++\ngenus Punctum {\n    numerus x\n}\n";
    let result =
        codemod_field_modifiers(Path::new("punctum.fab"), source).expect("codemod accepted");
    assert_eq!(result.inserted, 1);
    assert!(
        result.source.contains("    varia numerus x\n"),
        "{}",
        result.source
    );
}
