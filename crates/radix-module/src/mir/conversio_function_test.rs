//! D9.3 runner-route pins: an explicit `↦` through a registered
//! `@ conversio` pair calls the function on the MIR runner; a failable
//! function propagates or yields the `⊥` default.

use super::*;
use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::run_source;

fn stdout(source: &str) -> Vec<String> {
    let session = Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_latin_locale(),
    );
    let mut host = BufferHost::default();
    if let Err(error) = run_source(&session, "test.fab", source, &mut host) {
        match error {
            RunSourceError::Stepper(errors) => panic!("runner failed: {errors:?}"),
            RunSourceError::Frontend(diagnostics) => panic!("analysis failed: {diagnostics:?}"),
            RunSourceError::Mir(error) => panic!("lowering failed: {error:?}"),
        }
    }
    host.stdout_lines
}

const DECLS: &str = "
genus Celsius { varia fractus gradus }
genus Fahrenheit { varia fractus gradus }
genus Lectio { varia numerus valor }
genus Mensura { varia numerus valor }

@ conversio
functio adFahrenheit(Celsius c) → Fahrenheit {
    redde Fahrenheit { gradus = c.gradus * 9.0 / 5.0 + 32.0 }
}

@ conversio
functio adMensuram(Lectio l) → Mensura ⇥ textus {
    si l.valor ≺ 0 { iace \"lectio negativa\" }
    redde Mensura { valor = l.valor }
}
";

#[test]
fn infallible_conversio_function_serves_arrow() {
    let lines = stdout(&format!(
        "{DECLS}
incipit {{
    fixum Fahrenheit f ← Celsius {{ gradus = 100.0 }} ↦ Fahrenheit
    nota f.gradus
}}
"
    ));
    assert_eq!(lines, vec!["212.0"]);
}

#[test]
fn failable_conversio_function_takes_the_default_only_on_failure() {
    let lines = stdout(&format!(
        "{DECLS}
incipit {{
    fixum Mensura bona ← Lectio {{ valor = 7 }} ↦ Mensura ⊥ Mensura {{ valor = 0 }}
    nota bona.valor
    fixum Mensura mala ← Lectio {{ valor = -4 }} ↦ Mensura ⊥ Mensura {{ valor = 0 }}
    nota mala.valor
}}
"
    ));
    assert_eq!(lines, vec!["7", "0"]);
}

#[test]
fn failable_conversio_function_propagates_through_the_channel() {
    let lines = stdout(&format!(
        "{DECLS}
functio mensura(Lectio l) → Mensura ⇥ textus {{
    redde l ↦ Mensura
}}

functio descriptio(Lectio l) → textus {{
    fac {{
        fixum Mensura m ← mensura(l)
        redde \"mensura\"
    }}
    cape err {{
        redde err
    }}
}}

incipit {{
    nota descriptio(Lectio {{ valor = 3 }})
    nota descriptio(Lectio {{ valor = -3 }})
}}
"
    ));
    assert_eq!(lines, vec!["mensura", "lectio negativa"]);
}
