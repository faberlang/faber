use crate::reference_parse::entry_from_exempla;

#[test]
fn missing_frontmatter_fails() {
    let source = r#"functio ping() → textus {
    redde "pong"
}
"#;
    let err = entry_from_exempla("bad.fab", source, "ping", "keyword").expect_err("missing fm");
    assert!(err.message.contains("missing +++ frontmatter"));
}

#[test]
fn unterminated_frontmatter_fails() {
    let source = r#"+++
term = "x"
kind = "keyword"
category = "t"
summary = "S."
syntax = "x"

functio ping() → textus {
    redde "pong"
}
"#;
    let err = entry_from_exempla("bad.fab", source, "x", "keyword").expect_err("unterminated");
    assert!(err.message.contains("unterminated frontmatter"));
}

#[test]
fn invalid_frontmatter_toml_fails() {
    let source = r#"+++
term = "x"
kind = [1, 2]
category = "t"
summary = "S."
syntax = "x"
+++

incipit {}
"#;
    let err = entry_from_exempla("bad.fab", source, "x", "keyword").expect_err("invalid toml");
    assert!(err.message.contains("bad.fab"));
}

#[test]
fn unknown_frontmatter_keys_are_allowed() {
    let source = r#"+++
term = "x"
kind = "keyword"
category = "t"
summary = "Smoke entry."
syntax = "x"
surprise = "extra"
+++

incipit {}
"#;
    let entry = entry_from_exempla("ok.fab", source, "x", "keyword").expect("extra keys ok");
    assert_eq!(entry.term, "x");
}
