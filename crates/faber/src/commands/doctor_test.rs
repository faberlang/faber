use std::path::Path;
use std::process::Command;

use super::doctor::{doctor_report_for, render_doctor_report};
use super::format_test::faber_binary;
use faber_cli::library::LibraryHomeSource;

fn kv(report: &str, key: &str) -> String {
    let prefix = format!("{key}=");
    for line in report.lines() {
        if let Some(value) = line.strip_prefix(&prefix) {
            return value.to_owned();
        }
    }
    panic!("missing key {key} in:\n{report}");
}

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent");
    }
    std::fs::write(path, contents).expect("write file");
}

#[test]
fn doctor_report_emits_structural_availability_keys() {
    let root = tempfile::tempdir().expect("temp home");
    write_file(
        &root.path().join("norma/cista.toml"),
        "this is not toml {{{",
    );
    write_file(
        &root.path().join("norma/src/solum.fab"),
        "fn x() → int { return 1 }\n",
    );
    write_file(
        &root.path().join("triga/faber.toml"),
        "[package]\nname = \"triga\"\nversion = \"0.1.0\"\n\n[library]\nprovider = \"triga\"\n\n[paths]\nsource = \"src\"\n\n[build]\nkind = \"lib\"\ntargets = [\"rust\"]\n",
    );
    write_file(
        &root.path().join("triga/src/math.fab"),
        "+++\nlocale = \"en\"\n+++\n@ public\nfn one() → int {\n    return 1\n}\n",
    );

    let report = doctor_report_for(
        Path::new("/tmp/doctor-cwd").to_path_buf(),
        Some(root.path().to_path_buf()),
        Some(LibraryHomeSource::Env),
    );
    let rendered = render_doctor_report(&report);

    assert_eq!(kv(&rendered, "cwd"), "/tmp/doctor-cwd");
    assert_eq!(
        kv(&rendered, "library_home"),
        root.path().display().to_string()
    );
    assert_eq!(kv(&rendered, "library_home_source"), "env");
    assert_eq!(
        kv(&rendered, "library.norma.availability"),
        "cista-source-package"
    );
    assert_eq!(kv(&rendered, "library.norma.identity"), "builtin");
    assert_eq!(
        kv(&rendered, "library.norma.root"),
        root.path().join("norma/src").display().to_string()
    );
    assert_eq!(kv(&rendered, "library.triga.availability"), "faber-package");
    assert_eq!(kv(&rendered, "library.triga.identity"), "package");
    assert_eq!(kv(&rendered, "library.gradus.availability"), "not-found");
    assert_eq!(kv(&rendered, "library.tela.availability"), "not-found");
    assert!(
        !rendered.contains("library.gradus.root="),
        "not-found rows omit root:\n{rendered}"
    );
}

#[test]
fn doctor_cli_exits_zero_with_structural_keys() {
    let binary = faber_binary();
    let output = Command::new(&binary)
        .args(["doctor"])
        .env_remove("FABER_LIBRARY_HOME")
        .output()
        .expect("run faber doctor");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "faber doctor must exit 0:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(!kv(&stdout, "library_home").is_empty());
    let source = kv(&stdout, "library_home_source");
    assert!(
        source == "env" || source == "embedded",
        "library_home_source must be env or embedded, got {source}"
    );
    assert_eq!(kv(&stdout, "library.norma.identity"), "builtin");
    assert!(
        stdout.contains("library.norma.availability="),
        "doctor must report norma availability:\n{stdout}"
    );
}

#[test]
fn doctor_cli_honors_explicit_library_home() {
    let root = tempfile::tempdir().expect("temp home");
    write_file(&root.path().join("norma/cista.toml"), "not-toml");
    write_file(&root.path().join("norma/src/.keep"), "");

    let binary = faber_binary();
    let output = Command::new(&binary)
        .args(["doctor"])
        .env("FABER_LIBRARY_HOME", root.path())
        .output()
        .expect("run faber doctor");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "faber doctor must exit 0:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert_eq!(
        kv(&stdout, "library_home"),
        root.path().display().to_string()
    );
    assert_eq!(kv(&stdout, "library_home_source"), "env");
    assert_eq!(
        kv(&stdout, "library.norma.availability"),
        "cista-source-package"
    );
    assert_eq!(kv(&stdout, "library.norma.identity"), "builtin");
    assert_eq!(kv(&stdout, "library.triga.availability"), "not-found");
}
