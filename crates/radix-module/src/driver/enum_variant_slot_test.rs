//! Runner proofs for variant values stored into optional or field slots and
//! for payload variants of a generic union. The variant aggregate carries the
//! enum type (not the slot's `T ∪ none`), and a generic variant's payload
//! field instantiates at the aggregate's applied arguments.

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

const DECLS: &str = "enum Color {
    Red,
    Green
}

union Shape {
    Dot,
    Circ { f64 r }
}

union Opt<T> {
    Nada,
    Has { T v }
}

class Box {
    mut Color ∪ none k
    mut Shape ∪ none s
}
";

#[test]
fn enum_value_into_optional_local_prints_qualified_variant() {
    let out = run_en(&format!(
        "{DECLS}
main {{
    let Color ∪ none o ← Color.Green
    print o
}}
"
    ));
    assert_eq!(out, ["Color.Green"]);
}

#[test]
fn enum_value_into_optional_field_prints_qualified_variant() {
    let out = run_en(&format!(
        "{DECLS}
main {{
    let Box b ← Box {{ k = Color.Red, s = variant Circ {{ r = 1.5 }} ∷ Shape }}
    print b
}}
"
    ));
    assert_eq!(out, ["Box { k = Color.Red, s = Shape.Circ { r = 1.5 } }"]);
}

#[test]
fn payload_variant_into_optional_local_prints_qualified_variant() {
    let out = run_en(&format!(
        "{DECLS}
main {{
    let Shape ∪ none s ← variant Circ {{ r = 2.5 }} ∷ Shape
    print s
}}
"
    ));
    assert_eq!(out, ["Shape.Circ { r = 2.5 }"]);
}

#[test]
fn generic_union_variant_with_payload_runs() {
    let out = run_en(&format!(
        "{DECLS}
main {{
    let Opt<int> a ← variant Has {{ v = 3 }} ∷ Opt<int>
    print a
    let Opt<string> b ← variant Has {{ v = \"hi\" }} ∷ Opt<string>
    print b
    let Opt<int> c ← variant Nada ∷ Opt<int>
    print c
}}
"
    ));
    assert_eq!(out, ["Opt.Has { v = 3 }", "Opt.Has { v = \"hi\" }", "Opt.Nada"]);
}
