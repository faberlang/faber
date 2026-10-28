//! Runner proofs for the pr4 print rule: inside a composite a `littera` (char)
//! prints single-quoted and every `ascii` / `ascii<N>` value prints
//! double-quoted (D10.5 makes `ascii<1>` a string, not the character type), so
//! the quotes follow the STATIC type, never the value's width. A top-level
//! scalar still prints its bare value (D2.8).

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, RunSourceError, run_source};

fn run_en(body: &str) -> Vec<String> {
    let source = format!("+++\nlocale = \"en\"\n+++\n{body}");
    let session = Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_dev_locale_root(),
    );
    let mut host = BufferHost::default();
    match run_source(&session, "test.fab", &source, &mut host) {
        Ok(()) => host.stdout_lines,
        Err(RunSourceError::Stepper(errors)) => panic!("runner failed: {errors:?}"),
        Err(RunSourceError::Frontend(diagnostics)) => panic!("frontend failed: {diagnostics:?}"),
        Err(RunSourceError::Mir(error)) => panic!("MIR lowering failed: {error:?}"),
    }
}

const RECORD: &str = "class Rec {
    mut char c
    mut ascii<size 3> a3
    mut ascii<size 1> a1
    mut ascii u
    mut string t
    mut list<char> cs
    mut list<ascii> us
    mut list<string> ts
}
";

#[test]
fn genus_fields_quote_by_static_type() {
    let out = run_en(&format!(
        "{RECORD}
main {{
    let ascii<size 3> a3 ← 'abc'
    let ascii<size 1> a1 ← 'z'
    let Rec r ← Rec {{ c = 'q', a3 = a3, a1 = a1, u = 'q', t = \"q\", cs = ['x'], us = ['q', 'abc'], ts = [\"q\"] }}
    print r
}}
"
    ));
    assert_eq!(
        out,
        [r#"Rec { c = 'q', a3 = "abc", a1 = "z", u = "q", t = "q", cs = ['x'], us = ["q", "abc"], ts = ["q"] }"#]
    );
}

#[test]
fn lists_quote_by_static_type() {
    let out = run_en(
        "main {
    let list<char> cs ← ['x', 'y']
    print cs
    let list<ascii> us ← ['q', 'abc']
    print us
    let ascii<size 1> b1 ← 'z'
    let ascii<size 1> b2 ← 'y'
    let list<ascii<size 1>> a1s ← [b1, b2]
    print a1s
    let ascii<size 3> c1 ← 'abc'
    let ascii<size 3> c2 ← 'def'
    let list<ascii<size 3>> a3s ← [c1, c2]
    print a3s
    let list<string> ts ← ['q', \"r\"]
    print ts
}
",
    );
    assert_eq!(
        out,
        [
            "['x', 'y']",
            r#"["q", "abc"]"#,
            r#"["z", "y"]"#,
            r#"["abc", "def"]"#,
            r#"["q", "r"]"#,
        ]
    );
}

#[test]
fn top_level_scalars_print_bare() {
    let out = run_en(
        "main {
    let char c ← 'q'
    print c
    let ascii u ← 'q'
    print u
    let ascii<size 1> a1 ← 'q'
    print a1
    let ascii<size 3> a3 ← 'abc'
    print a3
}
",
    );
    assert_eq!(out, ["q", "q", "q", "abc"]);
}

#[test]
fn interpolated_lists_quote_like_print() {
    let out = run_en(
        "main {
    let list<char> cs ← ['x']
    let list<ascii> us ← ['q']
    print \"§ §\"(cs, us)
}
",
    );
    assert_eq!(out, [r#"['x'] ["q"]"#]);
}
