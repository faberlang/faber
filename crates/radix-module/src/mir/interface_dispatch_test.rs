//! Dynamic dispatch through a contract-typed value on the MIR runner: a method
//! call on an `implendum`-typed local or parameter lowers to a runtime
//! type-test chain over the local `genus` types that `implet` the contract,
//! the same shape a `∪` receiver takes (D9.2).

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
    match run_source(&session(), "interface-dispatch.fab", source, &mut host) {
        Ok(()) => host.stdout_lines,
        Err(RunSourceError::Frontend(errors)) => panic!("fixture frontend errors: {errors:?}"),
        Err(RunSourceError::Mir(errors)) => panic!("fixture MIR errors: {errors:?}"),
        Err(RunSourceError::Stepper(errors)) => panic!("fixture runner errors: {errors:?}"),
    }
}

const FORMAE: &str = r"
implendum Crescens {
    functio cresce(numerus per) → vacuum
    functio adde(varia numerus k) → vacuum
    functio mensura() → numerus
}

genus Quadratum implet Crescens {
    varia numerus latus

    functio cresce(numerus per) → vacuum { ego.latus ← ego.latus + per }
    functio adde(varia numerus k) → vacuum {
        ego.latus ← ego.latus + k
        k ← k + 100
    }
    functio mensura() → numerus { redde ego.latus * ego.latus }
}

genus Rectum implet Crescens {
    varia numerus lat
    varia numerus alt

    functio cresce(numerus per) → vacuum {
        ego.lat ← ego.lat + per
        ego.alt ← ego.alt + per
    }
    functio adde(varia numerus k) → vacuum { ego.lat ← ego.lat + k }
    functio mensura() → numerus { redde ego.lat * ego.alt }
}
";

#[test]
fn contract_typed_parameter_dispatches_to_each_implementor() {
    let source = format!(
        "{FORMAE}
functio summa(Crescens c) → numerus {{
    c.cresce(1)
    redde c.mensura()
}}

incipit {{
    nota summa(Quadratum {{ latus = 2 }})
    nota summa(Rectum {{ lat = 2, alt = 3 }})
}}
"
    );
    assert_eq!(run(&source), ["9", "12"]);
}

#[test]
fn contract_typed_local_mutates_the_live_value_through_self_and_mut_param() {
    let source = format!(
        "{FORMAE}
incipit {{
    varia Quadratum q ← Quadratum {{ latus = 1 }}
    sit Crescens c ← q
    c.cresce(1)
    varia numerus k ← 5
    c.adde(varia k)
    nota k
    nota c.mensura()
    nota q.mensura()
}}
"
    );
    assert_eq!(run(&source), ["105", "49", "49"]);
}
