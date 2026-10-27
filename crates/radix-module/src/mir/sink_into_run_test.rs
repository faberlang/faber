//! `sink ⇜ item` through the MIR runner: a class that implements the marking
//! contract on an in-test `Sink<string>` sees the operator write exactly the
//! item, in order, next to the method form.
//!
//! The contract is declared locally on the Latin surface, so no Norma import
//! surface is in play: the fixture proves the runner, not package resolution.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, RunSourceError, run_source};

fn run(source: &str) -> (Vec<String>, Vec<(String, String)>) {
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_target(Target::HirTypeScript),
    );
    let mut host = BufferHost::default();
    let errors = match run_source(&session, "sink-into.fab", source, &mut host) {
        Ok(()) => Vec::new(),
        Err(RunSourceError::Stepper(errors)) => errors.into_iter().map(|e| (e.issue, e.message)).collect(),
        Err(other) => panic!("{source}: {other:?}"),
    };
    (host.stdout_lines, errors)
}

fn ok(source: &str) -> Vec<String> {
    let (out, errors) = run(source);
    assert!(errors.is_empty(), "{source}: {errors:?}");
    out
}

#[test]
fn sink_into_runner_takes_method_and_operator() {
    let out = ok(r#"@ radix contract "sinking"
implendum Sink<T> {
    functio sink(T res) → vacuum ⇥ textus
    functio flush() → vacuum ⇥ textus
}

genus Crater implet Sink<textus> {
    varia lista<textus> res

    functio sink(textus item) → vacuum ⇥ textus {
        ego.res.appende(item)
    }

    functio flush() → vacuum ⇥ textus {
    }
}

functio effunde(varia Crater c) → vacuum ⇥ textus {
    c.sink("primus")
    c ⇜ "secundus"
    c.flush()
}

incipit {
    varia Crater c ← Crater { res = vacua }
    fac {
        effunde(varia c)
    } cape err { }
    nota c.res
}"#);
    assert_eq!(out, ["[\"primus\", \"secundus\"]"]);
}
