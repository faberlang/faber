//! An explicitly parameterised constructor (`Cista<int> { item = 7 }`) lowers
//! to the same struct aggregate the inferred form (`Cista { item = 7 }`) does
//!.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, RunSourceError, run_source};

fn run(name: &str, source: &str) -> Vec<String> {
    let session = Session::new(Config::default().with_target(Target::HirTypeScript));
    let mut host = BufferHost::default();
    match run_source(&session, name, source, &mut host) {
        Ok(()) => host.stdout_lines,
        Err(RunSourceError::Frontend(errors)) => panic!("fixture frontend errors: {errors:?}"),
        Err(RunSourceError::Mir(errors)) => panic!("fixture MIR errors: {errors:?}"),
        Err(RunSourceError::Stepper(errors)) => panic!("fixture runner errors: {errors:?}"),
    }
}

const SINGLE_PARAMETER: &str = r#"+++
locale = "en"
+++
class Cista<T> {
    mut T item

    fn get() → T {
        return self.item
    }
}

main {
    let Cista<int> b ← Cista<int> { item = 7 }
    print b.item
    print b.get()
    let Cista<int> c ← Cista { item = 7 }
    print c.item
    print c.get()
}
"#;

#[test]
fn explicit_single_parameter_constructor_matches_inferred_form() {
    assert_eq!(run("cista-explicit.fab", SINGLE_PARAMETER), vec!["7", "7", "7", "7"]);
}

const TWO_PARAMETERS: &str = r#"+++
locale = "en"
+++
class Pair<A, B> {
    mut A first
    mut B second
}

main {
    let Pair<int, string> p ← Pair<int, string> { first = 1, second = "x" }
    print p.first
    print p.second
}
"#;

#[test]
fn explicit_two_parameter_constructor_runs() {
    assert_eq!(run("pair-explicit.fab", TWO_PARAMETERS), vec!["1", "x"]);
}

const SIZE_PARAMETER: &str = r#"+++
locale = "en"
+++
class Box<size N> {
    mut vector<f32, size N> minima

    fn scalar() → f32 {
        return 1.5
    }
}

main {
    let vector<f32, size 3> v ← [1.0, 2.0, 3.0] ↦ vector<f32, size 3>
    let Box<3> b ← Box<3> { minima = v }
    print b.scalar()
}
"#;

#[test]
fn explicit_size_parameter_constructor_runs() {
    assert_eq!(run("box-explicit.fab", SIZE_PARAMETER), vec!["1.5"]);
}
