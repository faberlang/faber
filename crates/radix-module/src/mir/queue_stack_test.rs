//! C4 access-law proofs for queue/stack `itera ex` order.
//!
//! Queue iteration follows insertion order (FIFO). Stack iteration follows
//! removal order (top-first LIFO), matching the settled access law.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, RunSourceError, run_source};

fn session() -> Session {
    Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_latin_locale(),
    )
}

fn run(source: &str) -> Vec<String> {
    let mut host = BufferHost::default();
    match run_source(&session(), "queue-stack-access.fab", source, &mut host) {
        Ok(()) => host.stdout_lines,
        Err(RunSourceError::Frontend(errors)) => panic!("fixture frontend errors: {errors:?}"),
        Err(RunSourceError::Mir(errors)) => panic!("fixture MIR errors: {errors:?}"),
        Err(RunSourceError::Stepper(errors)) => panic!("fixture runner errors: {errors:?}"),
    }
}

#[test]
fn queue_iteration_is_fifo_and_stack_iteration_is_top_first_lifo() {
    let source = r"
incipit {
    varia queue<numerus> q ← vacua
    q.appende(1)
    q.appende(2)
    q.appende(3)
    itera ex q fixum item {
        nota item
    }

    varia stack<numerus> s ← vacua
    s.appende(1)
    s.appende(2)
    s.appende(3)
    itera ex s fixum item {
        nota item
    }
}
";

    assert_eq!(
        run(source),
        ["1", "2", "3", "3", "2", "1"],
        "queue is FIFO; stack is top-first LIFO",
    );
}

#[test]
fn queue_stack_elementwise_uses_live_window_kind_and_both_broadcast_directions() {
    let source = r"
incipit {
    varia queue<numerus> q ← vacua
    q.appende(1)
    q.appende(2)
    varia queue<numerus> r ← vacua
    r.appende(10)
    r.appende(20)
    sit queue<numerus> sum ← q + r
    sit queue<numerus> scaled ← q * 3
    itera ex sum fixum item {
        nota item
    }
    itera ex (3 - q) fixum item {
        nota item
    }
    itera ex scaled fixum item {
        nota item
    }

    varia stack<numerus> a ← vacua
    a.appende(2)
    a.appende(4)
    varia stack<numerus> b ← vacua
    b.appende(1)
    b.appende(3)
    sit stack<numerus> products ← a ⊙ b
    itera ex products fixum item {
        nota item
    }
}
";

    assert_eq!(
        run(source),
        ["11", "22", "2", "1", "3", "6", "12", "2"],
        "queue/stack arithmetic preserves live window, kind, and operand order",
    );
}

#[test]
fn queue_stack_elementwise_live_length_mismatch_is_a_hard_runner_error() {
    let source = r"
incipit {
    varia queue<numerus> q ← vacua
    q.appende(1)
    q.appende(2)
    varia queue<numerus> r ← vacua
    r.appende(10)
    sit queue<numerus> _ ← q + r
}
";
    let mut host = BufferHost::default();
    let error = run_source(&session(), "queue-stack-arithmetic.fab", source, &mut host)
        .expect_err("different live queue lengths must reject");
    match error {
        RunSourceError::Stepper(errors) => {
            assert!(
                errors
                    .iter()
                    .any(|error| error.issue == "runner_lista_elementwise_length_mismatch"),
                "expected length mismatch, got {errors:?}"
            );
        }
        other => panic!("expected runner length error, got {other:?}"),
    }
}
