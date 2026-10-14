use std::path::Path;
use std::process::Command;

use super::check_library::{
    CheckLibraryError, check_dev_library, render_checkout, resolve_dev_library_from,
};
use super::format_test::faber_binary;
use faber_cli::library::{
    LibraryHomeSource, LibraryRootRule, ResolvedLibraryHome, SpecifierIdentity, specifier_identity,
};

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

fn env_home(path: &Path) -> ResolvedLibraryHome {
    ResolvedLibraryHome {
        path: path.to_path_buf(),
        source: LibraryHomeSource::Env,
    }
}

const LIB_FAB: &str = "+++\nlocale = \"en\"\n+++\n@ public\nfn one() → int {\n    return 1\n}\n";

#[test]
fn check_library_src_fallback_ignores_garbage_cista_toml() {
    let root = tempfile::tempdir().expect("temp home");
    write_file(
        &root.path().join("norma/cista.toml"),
        "this is not toml {{{",
    );
    write_file(
        &root.path().join("norma/not-src/hidden.fab"),
        "fn hidden() → int { return 0 }\n",
    );
    write_file(&root.path().join("norma/src/solum.fab"), LIB_FAB);

    let checkout = resolve_dev_library_from("norma", env_home(root.path())).expect("resolve norma");
    assert_eq!(checkout.root_rule, LibraryRootRule::SrcFallback);
    assert_eq!(checkout.source_root, root.path().join("norma/src"));
    assert_eq!(
        specifier_identity(&checkout.name),
        SpecifierIdentity::Builtin
    );
    assert_eq!(kv(&render_checkout(&checkout), "identity"), "builtin");
    assert_eq!(kv(&render_checkout(&checkout), "root_rule"), "src-fallback");

    let diagnostics = check_dev_library(&checkout);
    assert!(
        !diagnostics
            .iter()
            .any(radix::module::diagnostics::Diagnostic::is_error),
        "src-fallback norma fixture must check clean: {diagnostics:?}"
    );
}

#[test]
fn check_library_uses_faber_toml_source_dot_not_src_fallback() {
    let root = tempfile::tempdir().expect("temp home");
    write_file(
        &root.path().join("triga/faber.toml"),
        "[package]\nname = \"triga\"\nversion = \"0.1.0\"\nedition = \"2026\"\n\n[library]\nprovider = \"triga\"\n\n[paths]\nsource = \".\"\n\n[build]\nkind = \"lib\"\ntargets = [\"rust\"]\n",
    );
    write_file(&root.path().join("triga/math.fab"), LIB_FAB);

    let checkout = resolve_dev_library_from("triga", env_home(root.path())).expect("resolve triga");
    assert_eq!(checkout.root_rule, LibraryRootRule::FaberToml);
    assert_eq!(checkout.source_root, root.path().join("triga"));
    assert_eq!(
        specifier_identity(&checkout.name),
        SpecifierIdentity::Package
    );
    assert_eq!(kv(&render_checkout(&checkout), "identity"), "package");

    let diagnostics = check_dev_library(&checkout);
    assert!(
        !diagnostics
            .iter()
            .any(radix::module::diagnostics::Diagnostic::is_error),
        "faber.toml library fixture must check clean: {diagnostics:?}"
    );
}

#[test]
fn check_library_malformed_faber_toml_does_not_fall_back_to_src() {
    let root = tempfile::tempdir().expect("temp home");
    write_file(
        &root.path().join("triga/faber.toml"),
        "this is not toml {{{",
    );
    write_file(&root.path().join("triga/src/math.fab"), LIB_FAB);

    let err = resolve_dev_library_from("triga", env_home(root.path()))
        .expect_err("malformed faber.toml must not src-fallback");
    assert!(matches!(err, CheckLibraryError::Diagnostic(_)));
}

#[test]
fn check_library_refuses_bin_package() {
    let root = tempfile::tempdir().expect("temp home");
    write_file(
        &root.path().join("app/faber.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n[paths]\nsource = \"src\"\nentry = \"main.fab\"\n\n[build]\nkind = \"bin\"\n",
    );
    write_file(&root.path().join("app/src/main.fab"), "main {}\n");

    let err = resolve_dev_library_from("app", env_home(root.path())).expect_err("bin refused");
    assert!(matches!(err, CheckLibraryError::NotALibrary { .. }));
}

#[test]
fn check_library_missing_provider_is_unknown() {
    let root = tempfile::tempdir().expect("temp home");
    let err = resolve_dev_library_from("nope", env_home(root.path())).expect_err("missing");
    assert!(matches!(err, CheckLibraryError::UnknownProvider { .. }));
}

#[test]
fn check_library_cli_reports_selected_root() {
    let root = tempfile::tempdir().expect("temp home");
    write_file(&root.path().join("norma/cista.toml"), "not-toml");
    write_file(&root.path().join("norma/src/solum.fab"), LIB_FAB);

    let binary = faber_binary();
    let output = Command::new(&binary)
        .args(["check-library", "norma", "--library-home"])
        .arg(root.path())
        .output()
        .expect("run check-library");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "check-library norma fixture must succeed:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert_eq!(kv(&stdout, "name"), "norma");
    assert_eq!(kv(&stdout, "identity"), "builtin");
    assert_eq!(kv(&stdout, "root_rule"), "src-fallback");
    assert_eq!(
        kv(&stdout, "root"),
        root.path().join("norma/src").display().to_string()
    );
    assert!(
        stderr.contains("ok: norma"),
        "successful check-library must report ok:\n{stderr}"
    );
}

#[test]
fn check_library_cli_unknown_name_exits_nonzero() {
    let root = tempfile::tempdir().expect("temp home");
    let binary = faber_binary();
    let output = Command::new(&binary)
        .args(["check-library", "nope", "--library-home"])
        .arg(root.path())
        .output()
        .expect("run check-library");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "missing library must fail:\n{stderr}"
    );
    assert!(
        stderr.contains("nope"),
        "missing library must name the provider:\n{stderr}"
    );
}
