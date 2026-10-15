//! Runner proof for the list window `xs[lo‥hi]` (ruling B10 of need
//! 6a13d711): a new list of the range, fully in bounds or a failure, and the
//! existing octeti window keeps working.

use super::*;
use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::run_source;

fn run(source: &str) -> (Result<(), String>, Vec<String>) {
    let mut host = BufferHost::default();
    let session = Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_latin_locale(),
    );
    let result = run_source(&session, "test.fab", source, &mut host).map_err(|error| format!("{error:?}"));
    (result, host.stdout_lines)
}

#[test]
fn list_window_copies_the_range_for_numerus_textus_and_bits() {
    let (result, lines) = run("incipit {
    sit lista<numerus> xs ← [10, 11, 12, 13, 14]
    nota xs[1‥4]
    nota xs[1…3]
    sit lista<textus> ts ← [\"a\", \"b\", \"c\"]
    nota ts[0‥2]
    sit lista<b8> bs ← [0xde, 0xad, 0xbe, 0xef]
    nota bs[1‥3]
}
");
    result.expect("windows run");
    assert_eq!(lines, vec!["[11, 12, 13]", "[11, 12, 13]", r#"["a", "b"]"#, "[0xad, 0xbe]"]);
}

#[test]
fn list_window_empty_range_is_an_empty_list() {
    let (result, lines) = run("incipit {
    sit lista<numerus> xs ← [1, 2, 3]
    sit lista<numerus> w ← xs[2‥2]
    nota w.longitudo()
}
");
    result.expect("empty window runs");
    assert_eq!(lines, vec!["0"]);
}

#[test]
fn list_window_past_the_end_fails() {
    let (result, lines) = run("incipit {
    sit lista<numerus> xs ← [1, 2, 3]
    nota xs[1‥4]
}
");
    assert!(result.is_err(), "a window past the end must fail: {lines:?}");
    assert!(lines.is_empty(), "no short slice is printed: {lines:?}");
}
