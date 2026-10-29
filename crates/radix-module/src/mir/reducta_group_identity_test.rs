//! Need: the `coalesce` (`vel`) identity of a grouped `max`/`min`
//! reduction binds to the reduction, so an empty bucket yields the identity on
//! the MIR runner. The bodiless form, the body form and the Latin spelling.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, run_source};

fn run(config: Config, source: &str) -> Vec<String> {
    let session = Session::new(config.with_target(Target::HirTypeScript));
    let mut host = BufferHost::default();
    match run_source(&session, "group-identity.fab", source, &mut host) {
        Ok(()) => host.stdout_lines,
        Err(other) => panic!("{source}: {other:?}"),
    }
}

fn en(function_body: &str) -> Vec<String> {
    let source = format!(
        "+++\nlocale = \"en\"\n+++\n\
         fn biggest(tensor<f32, [4]> w, tensor<int, [4]> k) → tf32[3] {{\n    {function_body}\n}}\n\
         main {{\n\
             let list<f32> wl ← [1.0, 3.0, 2.0, 0.25]\n\
             let list<int> kl ← [0, 0, 2, 2]\n\
             let tf32[4] w ← wl ↦ tf32[4]\n\
             let tensor<int, [4]> k ← kl ↦ tensor<int, [4]>\n\
             print biggest(w, k)\n\
         }}\n"
    );
    run(Config::default(), &source)
}

#[test]
fn bodiless_grouped_max_identity_fills_the_empty_bucket() {
    let out = en("return reduce via max from w at [i] per [k[i]] coalesce 0.5");
    assert_eq!(out, ["[3.0, 0.5, 2.0]"]);
}

#[test]
fn grouped_min_identity_fills_the_empty_bucket() {
    let out = en("return reduce via min from w at [i] per [k[i]] coalesce 0.5");
    assert_eq!(out, ["[1.0, 0.5, 0.25]"]);
}

#[test]
fn body_form_binds_the_identity_to_the_reduction() {
    let out = en("return reduce via max from w at [i] per [k[i]] let s { return s } coalesce 0.5");
    assert_eq!(out, ["[3.0, 0.5, 2.0]"]);
}

#[test]
fn latin_vel_binds_the_identity_to_the_reduction() {
    let source = "functio biggest(tensor<fractus, [4]> w, tensor<numerus, [4]> k) → tensor<fractus, [3]> {\n  redde reducta via maxima ex w apud [i] pro [k[i]] vel 0.5\n}\n\
                  incipit {\nsit tensor<fractus, [4]> w ← [1.0, 3.0, 2.0, 0.25] ↦ tensor<fractus, [4]>\nsit tensor<numerus, [4]> k ← [0, 0, 2, 2] ↦ tensor<numerus, [4]>\nnota biggest(w, k)\n}\n";
    assert_eq!(run(Config::default().with_latin_locale(), source), ["[3.0, 0.5, 2.0]"]);
}
