//! D6.6 proof: cursors are lazy on the MIR runner.
//!
//! Each `itera ex <generator call>` step resumes the suspended generator until
//! its next `cede`; an early `rumpe` abandons it. The print order proves the
//! interleaving; the infinite producers prove termination.

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
    .unwrap_or_else(|error| panic!("{name} should run on the MIR runner: {error:?}"));
    host.stdout_lines
}

#[test]
fn sync_cursor_step_interleaves_producer_and_consumer() {
    let source = r#"+++
locale = "en"
+++
fn three() generator → int {
    for range 0 ‥ 3 const i {
        print "produce §"(i)
        yield i
    }
}

main {
    for from three() const n {
        print "consume §"(n)
    }
}
"#;
    assert_eq!(
        run_lines("lazy-sync.fab", source),
        [
            "produce 0",
            "consume 0",
            "produce 1",
            "consume 1",
            "produce 2",
            "consume 2"
        ]
    );
}

#[test]
fn infinite_sync_cursor_with_early_break_terminates() {
    let source = r#"+++
locale = "en"
+++
fn naturals() generator → int {
    mut int i ← 0
    while true {
        print "produce §"(i)
        yield i
        i ← i + 1
    }
}

main {
    for from naturals() const n {
        print "consume §"(n)
        if n ≥ 1 {
            break
        }
    }
    print "done"
}
"#;
    assert_eq!(
        run_lines("lazy-infinite.fab", source),
        ["produce 0", "consume 0", "produce 1", "consume 1", "done"]
    );
}

#[test]
fn infinite_async_cursor_with_early_break_terminates() {
    let source = r#"+++
locale = "en"
+++
fn naturals() async_generator → int {
    mut int i ← 0
    while true {
        print "produce §"(i)
        yield i
        i ← i + 1
    }
}

async_main {
    for from naturals() const n {
        print "consume §"(n)
        if n ≥ 1 {
            break
        }
    }
    print "done"
}
"#;
    assert_eq!(
        run_lines("lazy-async-infinite.fab", source),
        ["produce 0", "consume 0", "produce 1", "consume 1", "done"]
    );
}

#[test]
fn generator_consuming_an_infinite_generator_stays_lazy() {
    let source = r#"+++
locale = "en"
+++
fn naturals() generator → int {
    mut int i ← 0
    while true {
        yield i
        i ← i + 1
    }
}

fn evens() generator → int {
    for from naturals() const n {
        if n % 2 ≡ 0 {
            yield n
        }
    }
}

main {
    for from evens() const n {
        print n
        if n ≥ 4 {
            break
        }
    }
}
"#;
    assert_eq!(run_lines("lazy-nested.fab", source), ["0", "2", "4"]);
}

#[test]
fn materialized_cursor_still_collects_every_yield() {
    let source = r#"+++
locale = "en"
+++
fn pair() generator → int {
    yield 1
    yield 2
}

main {
    print pair()
    let list<int> all ← pair() ↦ list<int>
    print all.length()
    mut int total ← 0
    for from pair() const n {
        total ← total + n
    }
    print total
}
"#;
    assert_eq!(run_lines("lazy-materialize.fab", source), ["[1, 2]", "2", "3"]);
}
