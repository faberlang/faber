//! C4 access-law proofs for queue/stack `itera ex` order.
//!
//! Queue iteration follows insertion order (FIFO). Stack iteration follows
//! removal order (top-first LIFO), matching the settled access law.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{run_source, BufferHost, RunSourceError};

fn session() -> Session {
    Session::new(Config::default().with_target(Target::HirTypeScript))
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
    let source = r#"
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
"#;

    assert_eq!(
        run(source),
        ["1", "2", "3", "3", "2", "1"],
        "queue is FIFO; stack is top-first LIFO",
    );
}
