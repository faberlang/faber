//! `target ⇇ value` through the MIR runner (the oracle for the operator):
//! the value's logical values land in the target's existing storage.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, RunSourceError, run_source};

fn run(source: &str) -> (Vec<String>, Vec<(String, String)>) {
    let session = Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_latin_locale(),
    );
    let mut host = BufferHost::default();
    let errors = match run_source(&session, "copy-into.fab", source, &mut host) {
        Ok(()) => Vec::new(),
        Err(RunSourceError::Stepper(errors)) => errors.into_iter().map(|e| (e.issue, e.message)).collect(),
        Err(other) => panic!("{source}: {other:?}"),
    };
    (host.stdout_lines, errors)
}

fn ok(source: &str) -> Vec<String> {
    let (out, errors) = run(source);
    assert!(errors.is_empty(), "{source}: {errors:?}");
    out
}

#[test]
fn a_value_is_copied_into_a_var_tensor() {
    let out = ok("incipit {
sit tensor<f32, [2, 2]> a ← [[1.0, 2.0], [3.0, 4.0]] ↦ tensor<f32, [2, 2]>
varia tensor<f32, [2, 2]> b ← [[0.0, 0.0], [0.0, 0.0]] ↦ tensor<f32, [2, 2]>
b ⇇ a + a
nota b.planata()
}");
    assert_eq!(out, ["[2.0, 4.0, 6.0, 8.0]"]);
}

#[test]
fn the_target_keeps_its_storage_identity_for_every_alias() {
    // `c` aliases `b`'s storage; a copy-into through `b` is visible through
    // `c`, which a rebind (`b ← …`) would not be.
    let out = ok("incipit {
sit tensor<f32, [2, 2]> a ← [[1.0, 2.0], [3.0, 4.0]] ↦ tensor<f32, [2, 2]>
varia tensor<f32, [2, 2]> b ← [[0.0, 0.0], [0.0, 0.0]] ↦ tensor<f32, [2, 2]>
varia tensor<f32, [2, 2]> c ← b
b ⇇ a
nota c.planata()
}");
    assert_eq!(out, ["[1.0, 2.0, 3.0, 4.0]"]);
}

#[test]
fn a_view_source_is_written_into_a_fresh_buffer() {
    let out = ok("incipit {
sit tensor<f32, [2, 3]> a ← [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]] ↦ tensor<f32, [2, 3]>
varia tensor<f32, [3, 2]> t ← [[0.0, 0.0], [0.0, 0.0], [0.0, 0.0]] ↦ tensor<f32, [3, 2]>
t ⇇ a.transpone()
nota t.planata()
}");
    assert_eq!(out, ["[1.0, 4.0, 2.0, 5.0, 3.0, 6.0]"]);
}

#[test]
fn a_mut_parameter_is_written_in_place_and_visible_to_the_caller() {
    let out = ok(
        "functio double(tensor<f32, [2, 2]> source, in tensor<f32, [2, 2]> out) → vacuum {
    out ⇇ source + source
}

incipit {
sit tensor<f32, [2, 2]> a ← [[1.0, 2.0], [3.0, 4.0]] ↦ tensor<f32, [2, 2]>
varia tensor<f32, [2, 2]> b ← [[0.0, 0.0], [0.0, 0.0]] ↦ tensor<f32, [2, 2]>
double(a, in b)
nota b.planata()
}",
    );
    assert_eq!(out, ["[2.0, 4.0, 6.0, 8.0]"]);
}

#[test]
fn a_writable_field_target_is_written_in_place() {
    let out = ok("genus Holder {
    varia tensor<f32, [2, 2]> buf
}

functio fill(in Holder holder, tensor<f32, [2, 2]> source) → vacuum {
    holder.buf ⇇ source
}

incipit {
sit tensor<f32, [2, 2]> a ← [[1.0, 2.0], [3.0, 4.0]] ↦ tensor<f32, [2, 2]>
varia Holder h ← Holder { buf = [[0.0, 0.0], [0.0, 0.0]] ↦ tensor<f32, [2, 2]> }
fill(in h, a)
nota h.buf.planata()
}");
    assert_eq!(out, ["[1.0, 2.0, 3.0, 4.0]"]);
}
