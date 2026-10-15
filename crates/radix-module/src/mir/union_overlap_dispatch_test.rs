//! D9.2 per-member dispatch on the MIR runner: a method call, field read, or
//! field write through a `∪` of nominal members lowers to a runtime type-test
//! chain that narrows the receiver to each member and runs that member's own
//! method / field access.

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
    match run_source(&session(), "union-overlap.fab", source, &mut host) {
        Ok(()) => host.stdout_lines,
        Err(RunSourceError::Frontend(errors)) => panic!("fixture frontend errors: {errors:?}"),
        Err(RunSourceError::Mir(errors)) => panic!("fixture MIR errors: {errors:?}"),
        Err(RunSourceError::Stepper(errors)) => panic!("fixture runner errors: {errors:?}"),
    }
}

const FIGURA: &str = r"
genus Circulus {
    varia textus nomen
    varia numerus radius

    functio area() → numerus { redde 3 * ego.radius * ego.radius }
    functio scala(numerus k) → numerus { redde k * ego.radius }
}

genus Quadratum {
    varia textus nomen
    varia numerus latus

    functio area() → numerus { redde ego.latus * ego.latus }
    functio scala(numerus k) → numerus { redde k + ego.latus }
}

typus Figura = Circulus ∪ Quadratum
";

#[test]
fn overlap_method_call_dispatches_to_each_member() {
    let source = format!(
        "{FIGURA}
functio area_of(Figura f) → numerus {{ redde f.area() }}

incipit {{
    sit Figura c ← Circulus {{ nomen = \"c\", radius = 2 }}
    sit Figura q ← Quadratum {{ nomen = \"q\", latus = 3 }}
    nota area_of(c)
    nota area_of(q)
    nota c.scala(10)
    nota q.scala(10)
}}
"
    );
    assert_eq!(run(&source), ["12", "9", "20", "13"]);
}

#[test]
fn overlap_field_read_projects_each_member() {
    let source = format!(
        "{FIGURA}
functio nomen_of(Figura f) → textus {{ redde f.nomen }}

incipit {{
    nota nomen_of(Circulus {{ nomen = \"c\", radius = 2 }})
    nota nomen_of(Quadratum {{ nomen = \"q\", latus = 3 }})
}}
"
    );
    assert_eq!(run(&source), ["c", "q"]);
}

#[test]
fn overlap_field_write_updates_the_live_member() {
    let source = format!(
        "{FIGURA}
incipit {{
    varia Figura f ← Quadratum {{ nomen = \"q\", latus = 3 }}
    f.nomen ← \"quadratum\"
    nota f.nomen
    nota f.area()
    f ← Circulus {{ nomen = \"c\", radius = 1 }}
    f.nomen ← \"circulus\"
    nota f.nomen
    nota f.area()
}}
"
    );
    assert_eq!(run(&source), ["quadratum", "9", "circulus", "3"]);
}
