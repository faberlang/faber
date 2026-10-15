//! Runner proof for the ordering contract (D1.4): glyphs on a conforming
//! genus call `compare`, built-ins satisfy an ordering bound (floats by
//! IEEE 754 totalOrder), and tuples order lexicographically.

use super::*;
use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::run_source;

fn run_lines(source: &str) -> Vec<String> {
    let mut host = BufferHost::default();
    let session = Session::new(Config::default().with_target(Target::HirTypeScript));
    if let Err(error) = run_source(&session, "test.fab", source, &mut host) {
        panic!("ordering fixture should run: {error:?}");
    }
    host.stdout_lines
}

const ORDERABLE: &str = r#"
@ radix contract "ordering"
implendum Orderable<T> {
    functio compare(T other) → numerus
}

genus Persona implet Orderable<Persona> {
    varia numerus anni

    functio compare(Persona other) → numerus { redde ego.anni - other.anni }
}

functio maior<T implet Orderable<T>>(T a, T b) → T {
    si a ≻ b { redde a }
    redde b
}
"#;

#[test]
fn genus_glyphs_call_compare() {
    let lines = run_lines(&format!(
        "{ORDERABLE}
incipit {{
    sit Persona a ← Persona {{ anni = 3 }}
    sit Persona b ← Persona {{ anni = 5 }}
    nota a ≺ b
    nota a ≥ b
    nota maior(a, b).anni
}}
"
    ));
    assert_eq!(lines, vec!["verum", "falsum", "5"]);
}

#[test]
fn builtins_order_through_the_bound() {
    let lines = run_lines(&format!(
        "{ORDERABLE}
incipit {{
    nota maior(3, 9)
    nota maior(\"pirum\", \"malum\")
    sit fractus top ← maior(1.5, nonnumerus)
    nota top ≢ top
    nota maior(2.5, -1.0)
}}
"
    ));
    assert_eq!(lines, vec!["9", "pirum", "verum", "2.5"]);
}

#[test]
fn tuples_order_lexicographically() {
    let lines = run_lines(&format!(
        "{ORDERABLE}
incipit {{
    sit iuncta<numerus, textus> x ← iuncta<numerus, textus>[2, \"b\"]
    sit iuncta<numerus, textus> y ← iuncta<numerus, textus>[2, \"c\"]
    sit iuncta<numerus, textus> z ← iuncta<numerus, textus>[10, \"a\"]
    nota x ≺ y
    nota z ≤ x
    nota maior(x, y)[1]
}}
"
    ));
    assert_eq!(lines, vec!["verum", "falsum", "c"]);
}
