use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn test_temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("faber-install-path-{label}-{nonce}"));
    fs::create_dir_all(&root).expect("create temp dir");
    root
}

fn write_cista_package(root: &Path) {
    fs::create_dir_all(root.join("interfaces")).expect("create interfaces");
    fs::write(
        root.join("cista.toml"),
        r#"[source]
package = "mathesis"
version = "0.1.0"
faber_min = "0.38.0"
kind = "source"
interfaces = "interfaces"

[target]
language = "rust"
mode = "compile"
binding_policy = "generated"
crate = "mathesis"
"#,
    )
    .expect("write cista manifest");
    fs::write(
        root.join("interfaces/mathesis.fab"),
        "functio quadratum(numerus n) → numerus { redde n * n }\n",
    )
    .expect("write interface");
}

fn write_project(root: &Path) {
    fs::create_dir_all(root.join("src")).expect("create project src");
    fs::write(
        root.join("faber.toml"),
        r#"[package]
name = "install-path-demo"
version = "0.1.0"
edition = "2026"

[paths]
source = "src"
entry = "main.fab"

[dependencies]
mathesis = "0.1.0"
"#,
    )
    .expect("write faber manifest");
    fs::write(root.join("src/main.fab"), "incipit { nota 1 }\n").expect("write main");
}

#[test]
fn install_path_installs_cista_package_and_rewrites_project_lock() {
    let root = test_temp_dir("cista-store");
    let package = root.join("source/mathesis");
    let project = root.join("demo");
    let store = root.join("store");
    write_cista_package(&package);
    write_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_faber"))
        .args([
            "install",
            "--path",
            package.to_str().expect("package path"),
            "--store",
            store.to_str().expect("store path"),
            "--project",
            project.to_str().expect("project path"),
        ])
        .output()
        .expect("run faber install --path");

    assert!(
        output.status.success(),
        "faber install --path failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(store
        .join("mathesis/0.1.0/interfaces/mathesis.fab")
        .is_file());

    let lock = fs::read_to_string(project.join("faber.lock")).expect("read faber.lock");
    assert!(lock.contains("name = \"mathesis\""), "lock was {lock}");
    assert!(lock.contains("version = \"0.1.0\""), "lock was {lock}");
    assert!(
        lock.contains("mathesis/0.1.0/interfaces"),
        "lock was {lock}"
    );
}
