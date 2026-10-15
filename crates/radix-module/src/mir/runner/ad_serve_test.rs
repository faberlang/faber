//! A4/U9 proof: the MIR runner answers `@ ad` routes from the program's
//! static table (tier 1), then builtins, then the host, and fails closed.
//!
//! A plain handler answers one item then `done`; a cursor handler is a
//! cooperative task resumed once per consumed frame, so it interleaves with
//! its consumer; `fini` on an unread stream cancels it; the opener crosses as
//! a deep copy (D6.14); a failed handler sends an `error` terminal.

use super::{BufferHost, RunSourceError, RunnerError, run_source};
use crate::codegen::Target;
use crate::driver::{Config, Session};

fn run(name: &str, source: &str) -> Result<Vec<String>, Vec<RunnerError>> {
    let mut host = BufferHost::default();
    match run_source(
        &Session::new(Config::default().with_target(Target::HirRust)),
        name,
        source,
        &mut host,
    ) {
        Ok(()) => Ok(host.stdout_lines),
        Err(RunSourceError::Stepper(errors)) => Err(errors),
        Err(error) => panic!("{name} should reach the MIR runner: {error:?}"),
    }
}

fn run_lines(name: &str, source: &str) -> Vec<String> {
    run(name, source).unwrap_or_else(|errors| panic!("{name} should run on the MIR runner: {errors:?}"))
}

fn run_error(name: &str, source: &str) -> RunnerError {
    let mut errors = run(name, source).expect_err("the run should fail");
    assert_eq!(errors.len(), 1, "one runner error expected: {errors:?}");
    errors.remove(0)
}

const COUNTER: &str = r#"@ ad 'numeri:omnes'
functio omnes() fiunt → numerus {
    varia numerus i ← 0
    dum verum {
        nota "produce §"(i)
        cede i
        i ← i + 1
    }
}
"#;

#[test]
fn plain_handler_answers_one_item() {
    let source = r#"+++
locale = "la"
+++
@ ad 'salutatio:saluta'
functio saluta(textus nomen) → textus {
    redde "salve, " + nomen
}

incipit {
    nota ad 'salutatio:saluta' ("munde") ↦ textus
}
"#;
    assert_eq!(run_lines("ad-serve-plain.fab", source), ["salve, munde"]);
}

#[test]
fn cursor_handler_interleaves_with_consumer() {
    let source = format!(
        r#"+++
locale = "la"
+++
{COUNTER}
incipit {{
    sit sermo s ← ad 'numeri:omnes'
    itera ex s.tuus<numerus>().cursor() fixum item {{
        nota "consume §"(item.data)
        si item.data ≥ 1 {{
            rumpe
        }}
    }}
    nota "finis"
}}
"#
    );
    assert_eq!(
        run_lines("ad-serve-interleave.fab", &source),
        ["produce 0", "consume 0", "produce 1", "consume 1", "finis"]
    );
}

#[test]
fn fini_on_unread_stream_cancels_cursor_handler() {
    let source = format!(
        r#"+++
locale = "la"
+++
{COUNTER}
incipit {{
    sit sermo s ← ad 'numeri:omnes'
    sit tuus<numerus> t ← s.tuus<numerus>()
    sit scrinium<numerus> ∪ nihil primum ← t.accipe()
    nota primum?.data
    sit scrinium<numerus> ∪ nihil secundum ← t.accipe()
    nota secundum?.data
    sit status clausum ← t.fini()
    sit scrinium<numerus> ∪ nihil post ← t.accipe()
    nota post ≡ nulla
}}
"#
    );
    // No `produce 2`: the cancelled handler is never resumed again.
    assert_eq!(
        run_lines("ad-serve-cancel.fab", &source),
        ["produce 0", "0", "produce 1", "1", "verum"]
    );
}

#[test]
fn opener_crosses_as_a_deep_copy() {
    let source = r#"+++
locale = "la"
+++
@ ad 'lista:auge'
functio auge(lista<numerus> xs) → numerus {
    varia lista<numerus> local ← copy xs
    local.appende(99)
    redde local.longitudo()
}

incipit {
    sit lista<numerus> meae ← [1, 2, 3]
    nota ad 'lista:auge' (meae) ↦ numerus
    nota meae.longitudo()
}
"#;
    assert_eq!(run_lines("ad-serve-copy.fab", source), ["4", "3"]);
}

#[test]
fn vacuum_and_cursor_handlers_materialize() {
    let source = r#"+++
locale = "la"
+++
@ ad 'nota:scribe'
functio scribe(textus t) → vacuum {
    nota t
}

@ ad 'numeri:tres'
functio tres() fiunt → numerus {
    cede 1
    cede 2
    cede 3
}

incipit {
    sit vacuum v ← ad 'nota:scribe' ("hic") ↦ vacuum
    sit lista<numerus> xs ← ad 'numeri:tres' ↦ lista<numerus>
    nota xs.longitudo()
}
"#;
    assert_eq!(run_lines("ad-serve-shapes.fab", source), ["hic", "3"]);
}

const DIVIDE: &str = r#"@ ad 'numeri:divide'
functio divide(numerus x) → numerus ⇥ textus {
    si x ≡ 0 { iace "nulla" }
    redde 10 / x
}
"#;

#[test]
fn failed_handler_error_terminal_is_recoverable() {
    let source = format!(
        r#"+++
locale = "la"
+++
{DIVIDE}
incipit {{
    nota ad 'numeri:divide' (2) ↦ numerus
    nota ad 'numeri:divide' (0) ↦ numerus ⊥ -1
}}
"#
    );
    assert_eq!(run_lines("ad-serve-recover.fab", &source), ["5", "-1"]);
}

#[test]
fn failed_handler_error_terminal_fails_materialization() {
    let source = format!(
        r#"+++
locale = "la"
+++
{DIVIDE}
incipit {{
    nota ad 'numeri:divide' (0) ↦ numerus
}}
"#
    );
    let error = run_error("ad-serve-failable.fab", &source);
    assert_eq!(error.issue, "runner_sermo_reply_conversion_failed");
}

#[test]
fn unrouted_conversation_fails_closed() {
    let source = r#"+++
locale = "la"
+++
incipit {
    nota ad 'nusquam:via' ("x") ↦ textus
}
"#;
    let error = run_error("ad-unrouted.fab", source);
    assert_eq!(error.code, "runner_unsupported");
    assert_eq!(
        error.args,
        [(
            "feature".to_owned(),
            "sermo route nusquam:via before runner provider runtime".to_owned()
        )]
    );
}
