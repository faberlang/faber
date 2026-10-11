//! Runner proof for the `ordina` key selector (D1.7): the
//! keyed sort is stable and orders `textus` keys by code point.

use super::*;
use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::run_source;

fn run_lines(source: &str) -> Vec<String> {
    let mut host = BufferHost::default();
    let session = Session::new(Config::default().with_target(Target::HirTypeScript));
    if let Err(error) = run_source(&session, "test.fab", source, &mut host) {
        panic!("keyed sort fixture should run: {error:?}");
    }
    host.stdout_lines
}

const PERSONAE: &str = r"
genus Persona {
    varia textus nomen
    varia numerus aetas
}

functio nomina(lista<Persona> ps) → lista<textus> {
    redde ps.mappata(Persona p ∴ p.nomen)
}
";

#[test]
fn ordina_numeric_key_is_stable() {
    let lines = run_lines(&format!(
        "{PERSONAE}
incipit {{
    varia lista<Persona> ps ← [Persona {{ nomen = \"c\", aetas = 2 }}, Persona {{ nomen = \"a\", aetas = 1 }}, Persona {{ nomen = \"b\", aetas = 2 }}, Persona {{ nomen = \"d\", aetas = 1 }}]
    ps.ordina(Persona p ∴ p.aetas)
    nota nomina(ps)
}}
"
    ));
    assert_eq!(lines, vec![r#"["a", "d", "c", "b"]"#]);
}

#[test]
fn ordina_text_key_sorts_in_place_by_code_point() {
    let lines = run_lines(&format!(
        "{PERSONAE}
incipit {{
    varia lista<Persona> ps ← [Persona {{ nomen = \"é\", aetas = 1 }}, Persona {{ nomen = \"z\", aetas = 2 }}, Persona {{ nomen = \"B\", aetas = 3 }}]
    ps.ordina(Persona p ∴ p.nomen)
    nota nomina(ps)
}}
"
    ));
    assert_eq!(lines, vec![r#"["B", "z", "é"]"#]);
}

#[test]
fn ordina_key_sorts_a_copy_bound_list_and_leaves_the_source() {
    let lines = run_lines(&format!(
        "{PERSONAE}
incipit {{
    varia lista<Persona> ps ← [Persona {{ nomen = \"c\", aetas = 2 }}, Persona {{ nomen = \"a\", aetas = 1 }}, Persona {{ nomen = \"b\", aetas = 2 }}, Persona {{ nomen = \"d\", aetas = 1 }}]
    varia lista<Persona> ordinata ← copy ps
    ordinata.ordina(Persona p ∴ p.aetas)
    nota nomina(ordinata)
    nota nomina(ps)
}}
"
    ));
    assert_eq!(lines, vec![r#"["a", "d", "c", "b"]"#, r#"["c", "a", "b", "d"]"#]);
}
