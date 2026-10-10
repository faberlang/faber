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

/// A bodyless function in an `en` file (need, items 3 and 4).
///
/// Plain mode is terse by design for every coded diagnostic: code, location
/// and source line, with no prose (the prose lives in the reader pack and
/// shows under `--diagnostics`). The `--diagnostics` text names the `en`
/// keyword `fn`, never the Latin `functio`.
#[test]
fn check_bodyless_function_in_en_file_plain_is_terse_and_diagnostics_prints_en_keyword() {
    let root = tempfile::tempdir().expect("create check fixture root");
    let file = root.path().join("bodyless.fab");
    std::fs::write(
        &file,
        "+++\nlocale = \"en\"\n+++\nfn parse(string s) -> string\n",
    )
    .expect("write source");
    let binary = super::format_test::faber_binary()
        .canonicalize()
        .expect("locate faber binary");

    let plain = Command::new(&binary)
        .args(["check"])
        .arg(&file)
        .output()
        .expect("run faber check");
    let plain = String::from_utf8_lossy(&plain.stderr);
    assert!(
        plain.contains("error[PARSE012]"),
        "plain mode keeps the code header:\n{plain}"
    );
    assert!(
        !plain.contains("expected function body"),
        "plain mode is terse by design: no message prose:\n{plain}"
    );

    let expanded = Command::new(&binary)
        .args(["check", "--diagnostics"])
        .arg(&file)
        .output()
        .expect("run faber check --diagnostics");
    let expanded = String::from_utf8_lossy(&expanded.stderr);
    assert!(
        expanded.contains("expected function body for 'fn' declaration"),
        "--diagnostics names the en keyword:\n{expanded}"
    );
    assert!(
        !expanded.contains("'functio'"),
        "an en diagnostic must not print the Latin keyword:\n{expanded}"
    );
}
