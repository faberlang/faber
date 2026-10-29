//! VEC-1: a vector literal returned into an optional vector slot.
//!
//! Typecheck types an expression by its destination, so the literal carries the
//! `vector<T, size N> ∪ nihil` slot type. The aggregate is the vector member; the
//! surrounding return widens it into the option.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, run_source};

#[test]
fn vector_literal_returned_into_optional_slot_lowers_as_the_vector() {
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_target(Target::HirTypeScript),
    );
    let mut host = BufferHost::default();
    let result = run_source(
        &session,
        "optional-vector.fab",
        "functio normalis(numerus codex) → vector<f32, magnitudo 3> ∪ nihil {\n    si codex ≡ 1 { redde [-1.0, 0.0, 0.0] ↦ vector<f32, magnitudo 3> }\n    redde nulla\n}\nincipit {\n    nota normalis(1)\n    nota normalis(2)\n}",
        &mut host,
    );
    assert!(result.is_ok(), "fixture failed: {:?}", result.err());
    assert_eq!(host.stdout_lines, ["[-1.0, 0.0, 0.0]", "nihil"]);
}
