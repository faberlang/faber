//! The `regex /body/` literal survives `faber format` idempotently: the
//! authored slash body round-trips verbatim, never respelled as a lambda.

use super::format_test::run_faber_format_stdout;
use std::fs;
use std::path::PathBuf;

const LA_SOURCE: &str = r#"+++
locale = "la"
+++
incipit {
    fixum regex r ← regex /\d+\/x/
    fixum regex s ← _ /ab+c/
}
"#;

fn temp_fixture(name: &str, source: &str) -> PathBuf {
    let path = std::env::temp_dir().join(name);
    fs::write(&path, source).expect("write regex literal fixture");
    path
}

/// Format `source` twice and require the second pass to change nothing.
fn format_idempotent(name: &str, source: &str) -> String {
    let fixture = temp_fixture(name, source);
    let first = run_faber_format_stdout(&fixture);
    fs::write(&fixture, &first).expect("rewrite first pass");
    let second = run_faber_format_stdout(&fixture);
    let _ = fs::remove_file(&fixture);
    assert_eq!(first, second, "format(format(x)) must equal format(x)");
    first
}

#[test]
fn format_la_regex_literal_is_idempotent() {
    let output = format_idempotent("faber-regex-literal-la.fab", LA_SOURCE);
    assert!(
        output.contains(r"regex /\d+\/x/"),
        "the regex literal must round trip verbatim:\n{output}"
    );
    assert!(
        !output.contains('↦'),
        "the regex literal must not be respelled with the lambda glyph:\n{output}"
    );
}
