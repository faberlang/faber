//! The words a program prints for `true`, `false` and `none` come from the
//! locale of the file holding the print statement.

use super::{BufferHost, run_source};
use crate::codegen::Target;
use crate::driver::{Config, Session};

fn run_lines(name: &str, source: &str) -> Vec<String> {
    let mut host = BufferHost::default();
    run_source(
        &Session::new(Config::default().with_target(Target::HirRust)),
        name,
        source,
        &mut host,
    )
    .unwrap_or_else(|error| panic!("{name} must run: {error:?}"));
    host.stdout_lines
}

#[test]
fn english_module_prints_true_false_none_through_the_runner() {
    let source = r#"+++
locale = "en"
+++
main {
    let bool yes ← true
    print yes
    print not yes
    let int ∪ none absent ← null
    print absent
}
"#;
    assert_eq!(run_lines("english-tokens.fab", source), vec!["true", "false", "none"]);
}

#[test]
fn latin_module_prints_verum_falsum_nihil_through_the_runner() {
    let source = r#"+++
locale = "la"
+++
incipit {
    sit bivalens ita ← verum
    nota ita
    sit bivalens non_ita ← falsum
    nota non_ita
    sit numerus ∪ nihil absens ← nulla
    nota absens
}
"#;
    assert_eq!(run_lines("latin-tokens.fab", source), vec!["verum", "falsum", "nihil"]);
}

#[test]
fn english_multi_argument_print_prints_none_through_the_tokens() {
    let source = r#"+++
locale = "en"
+++
main {
    let int ∪ none absent ← null
    print 1, absent, true
}
"#;
    assert_eq!(run_lines("english-multi.fab", source), vec!["1 none true"]);
}

#[test]
fn latin_multi_argument_print_prints_nihil() {
    let source = r#"+++
locale = "la"
+++
incipit {
    sit numerus ∪ nihil absens ← nulla
    nota 1, absens, verum
}
"#;
    assert_eq!(run_lines("latin-multi.fab", source), vec!["1 nihil verum"]);
}
