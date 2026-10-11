use super::{discover_build_layout, is_manifestless_single_file};
use crate::package::cargo::{RustRuntimePlan, emit_generated_crate_with_runtime_plan};
use crate::package::test_support::test_temp_dir;
use std::collections::BTreeSet;
use std::fs;

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
