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
    const bool yes ← true
    print yes
    print not yes
    const int ∪ none absent ← null
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
    fixum bivalens ita ← verum
    nota ita
    fixum bivalens non_ita ← falsum
    nota non_ita
    fixum numerus ∪ nihil absens ← nulla
    nota absens
}
"#;
    assert_eq!(run_lines("latin-tokens.fab", source), vec!["verum", "falsum", "nihil"]);
}
