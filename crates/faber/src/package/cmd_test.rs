#[cfg(feature = "hir-rust")]
use super::{discover_build_layout, is_manifestless_single_file};
#[cfg(feature = "hir-rust")]
use crate::package::cargo::{RustRuntimePlan, emit_generated_crate_with_runtime_plan};
#[cfg(feature = "hir-rust")]
use crate::package::test_support::test_temp_dir;
use radix::module::codegen::Target;
#[cfg(feature = "hir-rust")]
use std::collections::BTreeSet;
#[cfg(feature = "hir-rust")]
use std::fs;

#[cfg(feature = "hir-rust")]
fn dir_entries(dir: &std::path::Path) -> BTreeSet<String> {
    fs::read_dir(dir)
        .expect("read dir")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

#[cfg(feature = "hir-rust")]
#[test]
fn single_file_build_layout_lives_under_out_dir_not_beside_source() {
    let source_dir = test_temp_dir("single-src");
    let out_dir = test_temp_dir("single-out");
    let source = source_dir.join("only.fab");
    fs::write(&source, "main { }\n").expect("write source");

    assert!(is_manifestless_single_file(&source));
    let layout = discover_build_layout(&source, &out_dir).expect("layout");
    let out_root = out_dir.path().to_path_buf();
    assert!(layout.generated_crate_root.starts_with(&out_root));
    assert!(layout.cargo_target_dir.starts_with(&out_root));
    assert!(layout.debug_binary.starts_with(&out_root));

    emit_generated_crate_with_runtime_plan(
        &layout,
        "fn main() {}\n",
        None,
        &RustRuntimePlan::default_generated_crate_plan(),
    )
    .expect("emit generated crate");

    assert!(layout.generated_cargo_manifest.is_file());
    assert_eq!(
        dir_entries(&source_dir),
        BTreeSet::from(["only.fab".to_owned()]),
        "a single-file build must leave the source directory unchanged"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn manifest_backed_package_keeps_its_own_target_tree() {
    let package = test_temp_dir("single-pkg");
    let out_dir = test_temp_dir("single-pkg-out");
    fs::write(
        package.join("faber.toml"),
        "[package]\nname = \"keeps-target\"\nversion = \"0.1.0\"\n",
    )
    .expect("manifest");
    let source = package.join("main.fab");
    fs::write(&source, "main { }\n").expect("write source");

    assert!(!is_manifestless_single_file(&source));
    let layout = discover_build_layout(&source, &out_dir).expect("layout");
    assert!(
        layout
            .generated_crate_root
            .starts_with(layout.package_root.join("target"))
    );
}

#[test]
fn target_not_in_this_build_uses_canonical_spelling_for_every_gated_target() {
    for target in [
        Target::HirFhir,
        Target::MirLlvmHost,
        Target::MirWasm,
        Target::MirMetal,
        Target::HirGo,
        Target::HirRust,
        Target::HirTypeScript,
    ] {
        let diag = super::super::target_gate::target_not_in_this_build(target);
        assert_eq!(diag.issue(), Some("target_not_in_this_build"));
        let reported = diag
            .args
            .iter()
            .find(|arg| arg.name == "target")
            .map(|arg| arg.value.as_str());
        assert_eq!(reported, Some(target.as_str()));
    }
}
