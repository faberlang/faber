//! Runner proof that a generic function called with a concrete list type runs:
//! the monomorphizer defers substitution of body types whose concrete
//! instantiation is not yet interned and the interning finalize pass completes
//! them, so a call such as `first_or(list<int>, int)` is not reported as
//! `invalid MIR: instantiated generic type is missing semantic metadata`.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, RunSourceError, run_source};

fn run(source: &str) -> (Result<(), String>, Vec<String>) {
    let mut host = BufferHost::default();
    let session = Session::new(Config::default().with_target(Target::HirTypeScript));
    let result = run_source(&session, "generic-call.fab", source, &mut host).map_err(|error| match error {
        RunSourceError::Frontend(errors) => format!("frontend errors: {errors:?}"),
        RunSourceError::Mir(errors) => format!("MIR errors: {errors:?}"),
        RunSourceError::Stepper(errors) => format!("runner errors: {errors:?}"),
    });
    (result, host.stdout_lines)
}

#[test]
fn generic_call_with_list_type_param_runs() {
    let (result, lines) = run(r#"+++
locale = "en"
+++
fn first_or<T>(list<T> xs, T fallback) → T {
    return xs.first() coalesce fallback
}

main {
    let list<int> xs ← [1, 2, 3]
    print first_or(xs, 9)
    let list<string> ys ← empty
    print first_or(ys, "vacua")
}
"#);
    result.expect("generic call with a concrete list type runs");
    assert_eq!(lines, vec!["1", "vacua"]);
}
