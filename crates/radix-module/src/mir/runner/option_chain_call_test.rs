//! An optional chain call `f?(5)` runs on the MIR runner: when the optional
//! base holds a function value the callee is invoked, and when the base is
//! `none` the chain short-circuits to `none` before the callee is ever invoked
//! (need, campaign F6). A genus field marked `optional` is the
//! spellable optional-function value; a plain `T ∪ none` binding annotation
//! parses as a function with an optional *result*, not an optional function.

use super::{BufferHost, run_source};
use crate::codegen::Target;
use crate::driver::{Config, Session};

fn run_lines(source: &str) -> Vec<String> {
    let mut host = BufferHost::default();
    run_source(
        &Session::new(Config::default().with_target(Target::HirRust)),
        "option-chain-call.fab",
        source,
        &mut host,
    )
    .unwrap_or_else(|error| panic!("option chain call must run: {error:?}"));
    host.stdout_lines
}

#[test]
fn optional_chain_value_call_runs_and_short_circuits() {
    let source = r#"+++
locale = "en"
+++
class Holder {
    mut (int) → int cb optional
}
main {
    let Holder h ← Holder { cb = int x ∴ x * 2 }
    print h.cb?(5)
    let Holder g ← Holder { }
    print g.cb?(5)
}
"#;
    assert_eq!(run_lines(source), vec!["10", "none"]);
}

#[test]
fn optional_chain_method_call_runs_and_short_circuits() {
    let source = r#"+++
locale = "en"
+++
class Nomen {
    mut string n

    fn up() → string {
        return self.n.uppercase()
    }
}
main {
    let Nomen ∪ none a ← Nomen { n = "ab" }
    let Nomen ∪ none b ← null
    print a?.up()
    print b?.up()
}
"#;
    assert_eq!(run_lines(source), vec!["AB", "none"]);
}
