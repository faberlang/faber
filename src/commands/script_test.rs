use super::*;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("faber-script-{label}-{nanos}"));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

#[test]
fn single_fab_file_detection() {
    let fab = PathBuf::from("script.fab");
    assert!(is_single_fab_file(&fab) || !fab.exists());
    assert!(!is_single_fab_file(Path::new(".")));
}

#[test]
fn salve_munde_is_single_fab_file() {
    let fab = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../examples/corpus/incipit/salve-munde.fab");
    assert!(is_single_fab_file(&fab));
}

#[test]
fn interpret_package_input_detects_directory_manifest_and_manifest_entry() {
    let dir = temp_dir("package-input");
    let src = dir.join("src");
    std::fs::create_dir_all(&src).expect("create src");
    std::fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "run-policy"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("write manifest");
    let entry = src.join("main.fab");
    std::fs::write(&entry, "incipit { nota 1 }").expect("write entry");

    assert!(is_package_interpret_input(&dir));
    assert!(is_package_interpret_input(&dir.join("faber.toml")));
    assert!(is_package_interpret_input(&entry));
}

#[test]
fn interpret_package_input_detects_manifestless_import_entry() {
    let dir = temp_dir("manifestless-package-input");
    let entry = dir.join("main.fab");
    std::fs::write(
        &entry,
        r#"importa ex "./thing" privata thing

incipit { nota thing.label() }
"#,
    )
    .expect("write entry");

    assert!(is_package_interpret_input(&entry));
}

#[test]
fn interpret_package_input_leaves_manifestless_file_as_single_source() {
    let dir = temp_dir("single-file-input");
    let entry = dir.join("script.fab");
    std::fs::write(&entry, "incipit { nota 1 }").expect("write entry");

    assert!(!is_package_interpret_input(&entry));
}
