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

/// A bodyless function in an `en` file: expanded check output is the default,
/// and `--no-diagnostics` preserves the compact form.
#[test]
fn check_default_prints_help_and_no_diagnostics_keeps_the_compact_form() {
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
        .args(["check", "--no-diagnostics"])
        .arg(&file)
        .output()
        .expect("run faber check");
    let plain = String::from_utf8_lossy(&plain.stderr);
    assert!(
        plain.contains("error[PARSE012]"),
        "--no-diagnostics keeps the code header:\n{plain}"
    );
    assert!(
        !plain.contains("expected function body"),
        "--no-diagnostics omits message prose:\n{plain}"
    );
    assert!(
        !plain.contains("help:"),
        "compact output omits help:\n{plain}"
    );

    let expanded = Command::new(&binary)
        .args(["check"])
        .arg(&file)
        .output()
        .expect("run faber check");
    let expanded = String::from_utf8_lossy(&expanded.stderr);
    assert!(
        expanded.contains("expected function body for 'fn' declaration"),
        "default diagnostics names the en keyword:\n{expanded}"
    );
    assert!(
        expanded.contains("\nhelp:"),
        "default diagnostics shows the help text:\n{expanded}"
    );
    assert!(
        !expanded.contains("'functio'"),
        "the default diagnostic must not print the Latin keyword:\n{expanded}"
    );
}

fn run_check(args: &[&str], path: &std::path::Path) -> (bool, String) {
    let binary = super::format_test::faber_binary()
        .canonicalize()
        .expect("locate faber binary");
    let output = Command::new(binary)
        .arg("check")
        .args(args)
        .arg(path)
        .output()
        .expect("run faber check");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// A subcommand CLI whose root carries an exit expression passes semantic
/// analysis, but the MIR package lowering refuses that surface
/// (`package_mir_cli_surface_unsupported`). Plain `check` never lowers. This
/// is the stable fixture for "checks but does not lower": the plumbing under
/// test is the opt-in flag, the located named issue and the final line, not
/// which construct is unsupported today. If lowering learns this surface,
/// pick another checks-but-does-not-lower construct from
/// `scripta/check-lower-sweep.known`.
const CHECKS_BUT_DOES_NOT_LOWER_SOURCE: &str = r#"+++
locale = "en"
+++
@ cli "tool"
main args a exit 3 { }
@ command "greet"
fn greet() { }
"#;

#[test]
fn check_lower_reports_a_program_that_checks_but_does_not_lower() {
    let root = tempfile::tempdir().expect("create fixture root");
    let file = root.path().join("cli_surface.fab");
    std::fs::write(&file, CHECKS_BUT_DOES_NOT_LOWER_SOURCE).expect("write source");

    let (plain_ok, plain) = run_check(&[], &file);
    assert!(
        plain_ok,
        "plain check stays semantic analysis only:\n{plain}"
    );
    assert!(
        !plain.contains("PKG001"),
        "plain check never lowers:\n{plain}"
    );

    let (lower_ok, lower) = run_check(&["--lower"], &file);
    assert!(
        !lower_ok,
        "--lower must fail a program that does not lower:\n{lower}"
    );
    assert!(
        lower.contains("error[PKG001:package_mir_cli_surface_unsupported]"),
        "the lowering failure keeps its named issue:\n{lower}"
    );
    assert!(
        lower.contains(&format!(
            "{}: {}",
            file.display(),
            faber_cli::package::CHECKS_BUT_DOES_NOT_LOWER
        )),
        "the run ends on the checks-but-does-not-lower line:\n{lower}"
    );
    assert!(
        !lower.contains("ok: "),
        "no ok line after a lowering failure:\n{lower}"
    );
}

#[test]
fn check_lower_accepts_a_program_that_lowers() {
    let root = tempfile::tempdir().expect("create fixture root");
    let file = root.path().join("lowers.fab");
    std::fs::write(
        &file,
        "+++\nlocale = \"en\"\n+++\nfn f() → int { while true { return 3 } }\nmain { print f() }\n",
    )
    .expect("write source");
    let (ok, stderr) = run_check(&["--lower"], &file);
    assert!(ok, "a program that lowers passes --lower:\n{stderr}");
    assert!(
        stderr.contains(&format!("ok: {}", file.display())),
        "{stderr}"
    );
}

/// An omitted bare-`optional` argument lowers: the placeholder the caller keeps
/// for the missing position validates as the parameter's declared slot.
#[test]
fn check_lower_accepts_the_optional_parameter_exemplar() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/functio/sponte-vel.fab")
        .canonicalize()
        .expect("locate the sponte/vel exemplar");
    let (ok, stderr) = run_check(&["--lower"], &path);
    assert!(
        ok,
        "omitted optional and defaulted call arguments lower:\n{stderr}"
    );
    assert!(
        stderr.contains(&format!("ok: {}", path.display())),
        "successful lowering reports the input path:\n{stderr}"
    );
}

/// Default expanded diagnostics render the lowering failure with its named issue too.
#[test]
fn check_lower_diagnostics_mode_keeps_the_named_issue() {
    let root = tempfile::tempdir().expect("create fixture root");
    let file = root.path().join("located.fab");
    std::fs::write(&file, CHECKS_BUT_DOES_NOT_LOWER_SOURCE).expect("write source");
    let (ok, stderr) = run_check(&["--lower"], &file);
    assert!(!ok, "{stderr}");
    assert!(
        stderr.contains("package_mir_cli_surface_unsupported"),
        "default diagnostics keeps the named issue:\n{stderr}"
    );
}

#[test]
fn check_lower_accepts_a_single_command_cli_with_a_required_operand() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/operandus/operandus.fab")
        .canonicalize()
        .expect("locate operandus exemplar");
    let (ok, stderr) = run_check(&["--lower"], &path);
    assert!(
        ok,
        "a single-command CLI with a required operand lowers:\n{stderr}"
    );
    assert!(
        stderr.contains(&format!("ok: {}", path.display())),
        "successful lowering reports the input path:\n{stderr}"
    );
}

#[test]
fn check_lower_accepts_a_subcommand_cli_without_argumenta() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/cli/cli.fab")
        .canonicalize()
        .expect("locate CLI exemplar");
    let (ok, stderr) = run_check(&["--lower"], &path);
    assert!(ok, "a subcommand CLI without argumenta lowers:\n{stderr}");
    assert!(
        stderr.contains(&format!("ok: {}", path.display())),
        "successful lowering reports the input path:\n{stderr}"
    );
}

/// A library package has no entry unit: `--lower` lowers each module on its own.
#[test]
fn check_lower_lowers_each_module_of_a_package_without_an_entry() {
    let root = tempfile::tempdir().expect("create fixture root");
    std::fs::create_dir_all(root.path().join("src")).expect("create src");
    std::fs::write(
        root.path().join("faber.toml"),
        "[package]\nname = \"entryless\"\nversion = \"0.1.0\"\nedition = \"2026\"\n\n[paths]\nsource = \"src\"\n\n[build]\nkind = \"lib\"\ntargets = [\"rust\"]\n\n[library]\nprovider = \"entryless\"\n\n[locale]\nlocale = \"en\"\n",
    )
    .expect("write manifest");
    std::fs::write(
        root.path().join("src/one.fab"),
        "fn one() → int { return 1 }\n",
    )
    .expect("write module");
    std::fs::write(
        root.path().join("src/two.fab"),
        "fn two() → int { return 2 }\n",
    )
    .expect("write module");
    let (ok, stderr) = run_check(&["--lower"], root.path());
    assert!(
        ok,
        "an entryless library whose modules lower passes:\n{stderr}"
    );
}

#[test]
fn check_lower_cannot_combine_with_air_or_permissive_or_stdin() {
    use clap::Parser;
    assert!(
        crate::cli::Cli::try_parse_from(["faber", "check", "--lower", "--air", "x.fab"]).is_err()
    );
    assert!(
        crate::cli::Cli::try_parse_from(["faber", "check", "--lower", "--permissive", "x.fab"])
            .is_err()
    );
    let binary = super::format_test::faber_binary()
        .canonicalize()
        .expect("locate faber binary");
    let output = Command::new(binary)
        .args(["check", "--lower", "-"])
        .output()
        .expect("run faber check --lower -");
    assert!(!output.status.success(), "stdin has no package to lower");
}
