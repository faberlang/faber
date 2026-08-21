use std::process::Command;

#[test]
fn check_resolves_parent_relative_package_from_nested_cwd() {
    let root = tempfile::tempdir().expect("create check fixture root");
    let package = root.path().join("sibling");
    let nested = root.path().join("nested");
    std::fs::create_dir_all(package.join("src")).expect("create package source");
    std::fs::create_dir_all(&nested).expect("create nested cwd");
    std::fs::write(
        package.join("faber.toml"),
        r#"
[package]
name = "relative-check"
version = "0.1.0"
edition = "2026"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("write manifest");
    std::fs::write(package.join("src/main.fab"), "main {}\n").expect("write source");
    let package = package.canonicalize().expect("canonicalize package root");

    let binary = super::format_test::faber_binary()
        .canonicalize()
        .expect("locate faber binary");
    let output = Command::new(binary)
        .current_dir(&nested)
        .args(["check", "../sibling"])
        .output()
        .expect("run faber check");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "parent-relative package check must succeed:\nstdout: {}\nstderr: {stderr}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        stderr.contains(&format!("ok: {}", package.display())),
        "successful check must report that analysis ran for the resolved package:\n{stderr}"
    );
    assert!(
        !stderr.contains("No such file or directory"),
        "an existing parent-relative package must not report ENOENT:\n{stderr}"
    );
}
