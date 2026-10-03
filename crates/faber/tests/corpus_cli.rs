//! The corpus command must judge the same file and package routes as `faber run`.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn smoke_case() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/incipit/salve-munde.fab")
}

#[test]
fn corpus_accepts_existing_file_golden_and_rejects_mismatch() {
    let input = smoke_case();
    let pass = Command::new(env!("CARGO_BIN_EXE_faber"))
        .arg("corpus")
        .arg(&input)
        .output()
        .expect("run corpus file");
    assert!(
        pass.status.success(),
        "{}",
        String::from_utf8_lossy(&pass.stderr)
    );

    let temp = tempfile::tempdir().expect("temporary golden directory");
    let wrong = temp.path().join("wrong.expected");
    fs::write(&wrong, "wrong output\n").expect("write wrong golden");
    let fail = Command::new(env!("CARGO_BIN_EXE_faber"))
        .arg("corpus")
        .arg(&input)
        .arg("--expected")
        .arg(&wrong)
        .output()
        .expect("run corpus mismatch");
    assert!(!fail.status.success(), "a wrong golden must fail");
    assert!(
        String::from_utf8_lossy(&fail.stderr).contains("differs from"),
        "{}",
        String::from_utf8_lossy(&fail.stderr)
    );
}

#[test]
fn corpus_accepts_package_directory_with_entry_golden() {
    let temp = tempfile::tempdir().expect("temporary package directory");
    let src = temp.path().join("src");
    fs::create_dir(&src).expect("create source directory");
    fs::copy(smoke_case(), src.join("main.fab")).expect("copy corpus source");
    fs::copy(
        smoke_case().with_extension("expected"),
        src.join("main.expected"),
    )
    .expect("copy corpus golden");
    fs::write(
        temp.path().join("faber.toml"),
        "[package]\nname = \"corpus-smoke\"\nversion = \"0.1.0\"\nedition = \"2026\"\n\n[paths]\nsource = \"src\"\nentry = \"main.fab\"\n",
    )
    .expect("write package manifest");

    let output = Command::new(env!("CARGO_BIN_EXE_faber"))
        .arg("corpus")
        .arg(temp.path())
        .output()
        .expect("run corpus package");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
