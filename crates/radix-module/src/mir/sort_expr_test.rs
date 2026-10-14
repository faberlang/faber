//! Runner proof for the sort expression `ordinata via ascendens|descendens
//! ex|de <source> [fixum p { redde key }]`: always a new
//! list, stable in both directions, keys or values by the source word.

use super::*;
use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::run_source;

fn run_lines(source: &str) -> Vec<String> {
    let mut host = BufferHost::default();
    let session = Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_latin_locale(),
    );
    if let Err(error) = run_source(&session, "test.fab", source, &mut host) {
        panic!("sort expression fixture should run: {error:?}");
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
fn sort_expression_orders_a_list_both_directions_and_keeps_the_source() {
    let lines = run_lines(
        "incipit {
    fixum lista<numerus> xs ← [3, 1, 2, 1]
    nota ordinata via ascendens ex xs
    nota ordinata via descendens ex xs
    nota xs
}
",
    );
    assert_eq!(lines, vec!["[1, 1, 2, 3]", "[3, 2, 1, 1]", "[3, 1, 2, 1]"]);
}

#[test]
fn sort_expression_over_a_set_and_a_map_values_and_keys() {
    let lines = run_lines(
        "incipit {
    varia copia<numerus> s ← vacua
    s.adde(5)
    s.adde(2)
    s.adde(9)
    nota ordinata via ascendens ex s
    nota ordinata via descendens ex s
    varia tabula<textus, numerus> m ← vacua
    m[\"b\"] ← 20
    m[\"a\"] ← 30
    m[\"c\"] ← 10
    nota ordinata via ascendens ex m
    nota ordinata via descendens de m
}
",
    );
    assert_eq!(lines, vec!["[2, 5, 9]", "[9, 5, 2]", "[10, 20, 30]", r#"["c", "b", "a"]"#]);
}

#[test]
fn sort_expression_body_key_is_stable_in_both_directions() {
    let lines = run_lines(&format!(
        "{PERSONAE}
incipit {{
    fixum lista<Persona> ps ← [Persona {{ nomen = \"l\", aetas = 30 }}, Persona {{ nomen = \"m\", aetas = 30 }}, Persona {{ nomen = \"a\", aetas = 25 }}, Persona {{ nomen = \"c\", aetas = 25 }}]
    nota nomina(ordinata via ascendens ex ps fixum p {{ redde p.aetas }})
    nota nomina(ordinata via descendens ex ps fixum p {{ redde p.aetas }})
    fixum lista<numerus> e ← []
    nota ordinata via ascendens ex e
}}
"
    ));
    assert_eq!(lines, vec![r#"["a", "c", "l", "m"]"#, r#"["l", "m", "a", "c"]"#, "[]"]);
}
