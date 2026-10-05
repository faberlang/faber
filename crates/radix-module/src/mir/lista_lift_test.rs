//! Elementwise list operators on the MIR runner (2026-10-04 aggregate
//! operator rulings): the bounded-list length defect, the exact-then-store
//! element width, and the list-only forms `% ÷ ⊙ ⊘` and unary `-`.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, RunSourceError, run_source};

fn session() -> Session {
    Session::new(Config::default().with_target(Target::HirTypeScript))
}

fn run(source: &str) -> Vec<String> {
    let mut host = BufferHost::default();
    match run_source(&session(), "lista-lift.fab", source, &mut host) {
        Ok(()) => host.stdout_lines,
        Err(RunSourceError::Frontend(errors)) => panic!("fixture frontend errors: {errors:?}"),
        Err(RunSourceError::Mir(errors)) => panic!("fixture MIR errors: {errors:?}"),
        Err(RunSourceError::Stepper(errors)) => panic!("fixture runner errors: {errors:?}"),
    }
}

/// The runner failure of a program that must stop with one.
fn runner_failure(source: &str) -> String {
    let mut host = BufferHost::default();
    match run_source(&session(), "lista-lift.fab", source, &mut host) {
        Err(RunSourceError::Stepper(errors)) => errors
            .iter()
            .map(|error| format!("{} {}", error.issue, error.message))
            .collect::<Vec<_>>()
            .join("; "),
        other => panic!("expected a runner failure, got {other:?}"),
    }
}

#[test]
fn bounded_lista_pairs_the_live_length_not_the_capacity() {
    // Both lists are shorter than their capacity of 4: the result has the live
    // length of 2 (the capacity used to be iterated, reading out of bounds).
    let source = r"
incipit {
    varia lista<f32, 4> a ← vacua
    a.appende(1.0)
    a.appende(2.0)
    varia lista<f32, 4> b ← vacua
    b.appende(4.0)
    b.appende(5.0)
    fixum lista<f32, 4> sum ← a + b
    nota sum
    nota sum.longitudo()
    fixum lista<f32, 4> scaled ← 10.0 - a
    nota scaled
    nota scaled.longitudo()
}
";
    assert_eq!(run(source), ["[5.0, 7.0]", "2", "[9.0, 8.0]", "2"]);
}

#[test]
fn bounded_lista_length_mismatch_is_recoverable() {
    // Two bounded lists of one type with different live lengths leave through
    // the recoverable runtime-failure path (D6), like a push at capacity.
    let source = r#"
functio pair(lista<f32, 4> x, lista<f32, 4> y) → numerus ⇥ textus {
    fixum lista<f32, 4> total ← x + y
    redde total.longitudo()
}

incipit {
    varia lista<f32, 4> a ← vacua
    a.appende(1.0)
    a.appende(2.0)
    varia lista<f32, 4> c ← vacua
    c.appende(1.0)
    c.appende(2.0)
    c.appende(3.0)
    fac {
        nota pair(a, a)
        nota pair(a, c)
        nota "not reached"
    }
    cape err {
        nota "caught"
    }
    nota "after"
}
"#;
    assert_eq!(run(source), ["2", "caught", "after"]);
}

#[test]
fn unbounded_lista_length_mismatch_stays_a_hard_runner_error() {
    let source = r"
incipit {
    fixum lista<numerus> a ← [1, 2]
    fixum lista<numerus> b ← [3]
    fixum lista<numerus> c ← a + b
    nota c
}
";
    let failure = runner_failure(source);
    assert!(failure.contains("runner_lista_elementwise_length_mismatch"), "{failure}");
}

#[test]
fn lista_sum_widens_losslessly_and_traps_only_at_the_store() {
    // F9 19(g): the sum of two `u8` lists is exact; storing it into a `u16`
    // list is fine and storing it into a `u8` list traps per element.
    let wide = r"
incipit {
    fixum lista<u8> a ← [200, 100, 50]
    fixum lista<u8> b ← [100, 100, 100]
    fixum lista<u16> c ← a + b
    nota c
    fixum lista<u16> d ← a ⊙ b
    nota d
    fixum lista<u16> e ← a + 300
    nota e
}
";
    assert_eq!(run(wide), ["[300, 200, 150]", "[20000, 10000, 5000]", "[500, 400, 350]"]);
    let narrow = r"
incipit {
    fixum lista<u8> a ← [200, 100, 50]
    fixum lista<u8> b ← [100, 100, 100]
    fixum lista<u8> c ← a + b
    nota c
}
";
    let failure = runner_failure(narrow);
    assert!(failure.contains("300 does not fit in `u8`"), "{failure}");
}

#[test]
fn lista_forms_added_by_the_aggregate_operator_rulings() {
    let source = r"
incipit {
    fixum lista<f32> a ← [1.5, 2.5, 4.0]
    fixum lista<f32> b ← [2.0, 4.0, 8.0]
    nota a * 2.0
    nota 0.5 - a
    nota a % 2.0
    nota 9.0 % b
    nota a ÷ b
    nota 3.0 ÷ b
    nota a ⊙ b
    nota a ⊘ b
    nota -a
    fixum lista<numerus> n ← [7, 8, -9]
    fixum lista<numerus> m ← [2, 3, 4]
    nota n % 4
    nota n ⊘ m
    nota n ÷ m
    nota -n
}
";
    assert_eq!(
        run(source),
        [
            "[3.0, 5.0, 8.0]",
            "[-1.0, -2.0, -3.5]",
            "[1.5, 0.5, 0.0]",
            "[1.0, 1.0, 1.0]",
            "[0.75, 0.625, 0.5]",
            "[1.5, 0.75, 0.375]",
            "[3.0, 10.0, 32.0]",
            "[0.75, 0.625, 0.5]",
            "[-1.5, -2.5, -4.0]",
            "[3, 0, 3]",
            "[3, 2, -3]",
            "[3.5, 2.6666666666666665, -2.25]",
            "[-7, -8, 9]",
        ]
    );
}
