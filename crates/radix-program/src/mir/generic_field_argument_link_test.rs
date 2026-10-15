//! A field of an entry-owned class instance is a typed generic call argument.
//!
//! The package generic instantiator binds a library kernel's size witnesses
//! from the call's argument types. A field projection (`w.a`) types through
//! the merged struct metadata, which must include the entry unit's own
//! nominals; without them the witnesses stay unbound and the link fails with
//! `generic call signature has unresolved type parameter`, while binding the
//! field to a local first links. The callee returns void so there is no
//! destination type to bind the witnesses from (`decode_token.embed_step`).

use super::*;

const TWIN_LIB: &str = r#"+++
locale = "en"
+++
@ public
fn note_shape<size M, size N>(tensor<f32, [M, N]> a) → void {
    print "shape"
}
"#;

const MAIN: &str = r#"+++
locale = "en"
+++
import from "twinslib:twins" twins

class Pair {
    mut tensor<f32, [2, 2]> a
    mut tensor<f32, [2, 2]> b
}

fn make() → Pair {
    mut tensor<f32, [2, 2]> a ← [[1.0, 2.0], [3.0, 4.0]] ↦ tensor<f32, [2, 2]>
    mut tensor<f32, [2, 2]> b ← [[10.0, 20.0], [30.0, 40.0]] ↦ tensor<f32, [2, 2]>
    return Pair { a = a, b = b }
}

fn add(Pair p) → void {
    twins.note_shape(p.a)
    print "ok"
}

main {
    let Pair p ← make()
    add(p)
}
"#;

fn write_package(dir: &Path) -> PathBuf {
    let src = dir.join("src");
    let library_root = dir.join("libhome").join("twinslib");
    fs::create_dir_all(&src).expect("create package src");
    fs::create_dir_all(library_root.join("src")).expect("create library src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "field-argument"
version = "1.0.0"
edition = "2026"

[paths]
source = "src"
entry = "main.fab"

[build]
kind = "bin"

[dependencies]
twinslib = "1.0.0"
"#,
    )
    .expect("write faber.toml");
    let library_display = library_root.display();
    let lock = format!(
        "[[package]]\nname = \"twinslib\"\nversion = \"1.0.0\"\nsource = \"path:{library_display}\"\npackage_root = \"{library_display}\"\nkind = \"source\"\ntarget_language = \"\"\ntarget_triple = \"\"\ntarget_manifest = \"\"\ninterface_root = \"{library_display}/src\"\nartifact = \"\"\ncrate = \"\"\nrustc = \"\"\n"
    );
    fs::write(dir.join("faber.lock"), lock).expect("write faber.lock");
    fs::write(library_root.join("src/twins.fab"), TWIN_LIB).expect("write library module");
    fs::write(src.join("main.fab"), MAIN).expect("write main.fab");
    src.join("main.fab")
}

#[test]
fn entry_class_field_argument_binds_generic_witnesses() {
    let dir = crate::test_support::test_temp_dir("field-argument");
    let entry = write_package(dir.as_ref());
    let config = Config::default().with_library_home(dir.join("libhome"));

    with_interpreted_lowered_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        |lowered| {
            assert!(
                !lowered.monomorphizations.is_empty(),
                "note_shape must instantiate for the field-argument call"
            );
        },
    )
    .unwrap_or_else(|errors| panic!("package MIR must link, got {errors:?}"));
}
