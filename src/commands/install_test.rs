use super::install::{install_library, install_store_source, InstallError};
use crate::package::compile_package;
use radix::driver::Config;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn test_temp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("faber-install-test-{label}-{nanos}"));
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn write_library_repo(root: &Path, package: &str, provider: &str, source: &str) -> PathBuf {
    fs::create_dir_all(root.join(source).join("math")).expect("create source tree");
    fs::write(
        root.join("faber.toml"),
        format!(
            r#"
[package]
name = "{package}"
version = "0.1.0"

[library]
provider = "{provider}"

[paths]
source = "{source}"

[build]
kind = "lib"
targets = ["rust"]
"#
        ),
    )
    .expect("write manifest");
    fs::write(
        root.join(source).join("math").join("add.fab"),
        r#"
functio addit(numerus left, numerus right) → numerus {
    redde left + right
}
"#,
    )
    .expect("write module");
    git(root, &["init"]);
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "seed library"]);
    root.to_path_buf()
}

fn write_app(root: &Path, provider: &str) {
    fs::create_dir_all(root.join("src")).expect("create app src");
    fs::write(
        root.join("faber.toml"),
        r#"
[package]
name = "consumer"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("write app manifest");
    fs::write(
        root.join("src/main.fab"),
        format!(
            r#"
importa ex "{provider}:math/add" privata add

incipit {{
    nota add.addit(1, 2)
}}
"#
        ),
    )
    .expect("write app entry");
}

fn write_cista_repo(root: &Path, package: &str) -> PathBuf {
    fs::create_dir_all(root.join("src/math")).expect("create cista source tree");
    fs::write(
        root.join("cista.toml"),
        format!(
            r#"[source]
package = "{package}"
version = "0.1.0"
faber_min = "0.38.0"
kind = "source"
interfaces = "src"

[target]
language = "rust"
mode = "compile"
binding_policy = "generated"
crate = "{package}"
"#
        ),
    )
    .expect("write cista manifest");
    fs::write(
        root.join("src/math/add.fab"),
        "functio addit(numerus left, numerus right) → numerus { redde left + right }\n",
    )
    .expect("write cista source");
    git(root, &["init"]);
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "seed cista package"]);
    root.to_path_buf()
}

fn write_project_with_dependency(root: &Path, package: &str) {
    fs::create_dir_all(root.join("src")).expect("create project src");
    fs::write(
        root.join("faber.toml"),
        format!(
            r#"[package]
name = "consumer"
version = "0.1.0"
edition = "2026"

[dependencies]
{package} = "0.1.0"

[paths]
source = "src"
entry = "main.fab"
"#
        ),
    )
    .expect("write project manifest");
}

fn git(cwd: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_AUTHOR_NAME", "Faber Test")
        .env("GIT_AUTHOR_EMAIL", "faber-test@example.invalid")
        .env("GIT_COMMITTER_NAME", "Faber Test")
        .env("GIT_COMMITTER_EMAIL", "faber-test@example.invalid")
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn install_git_url_cista_package_updates_store_and_project_lock() {
    let fixture = test_temp_dir("git-store");
    let source_repo = write_cista_repo(&fixture.join("gitmath-repo"), "gitmath");
    let store = fixture.join("store");
    let project = fixture.join("consumer");
    write_project_with_dependency(&project, "gitmath");
    let url = format!("file://{}", source_repo.display());

    install_store_source(&url, Some(&store), Some(&project), "rust").expect("install from git URL");

    let installed_root = store.join("gitmath/0.1.0");
    assert!(installed_root.join("interfaces/math/add.fab").is_file());
    let lock = cista::faber_lock::read_lock(&project.join("faber.lock")).expect("read lock");
    let gitmath = lock
        .packages
        .iter()
        .find(|package| package.name == "gitmath")
        .expect("gitmath lock entry");
    assert_eq!(gitmath.version, "0.1.0");
    assert_eq!(
        PathBuf::from(&gitmath.package_root),
        installed_root.canonicalize().unwrap()
    );

    fs::remove_dir_all(fixture).expect("cleanup temp root");
}

#[test]
fn install_git_url_without_cista_manifest_fails_closed() {
    let fixture = test_temp_dir("git-faber-only");
    let source_repo = write_library_repo(&fixture.join("source-lib"), "legacy", "legacy", "src");
    let store = fixture.join("store");
    let url = format!("file://{}", source_repo.display());

    let errors = install_store_source(&url, Some(&store), None, "rust")
        .expect_err("faber.toml-only git installs must not use Cista store");

    assert!(errors
        .iter()
        .any(|error| error.contains("has no cista.toml")));
    assert!(
        !store.exists(),
        "failed install must not create a store snapshot"
    );

    fs::remove_dir_all(fixture).expect("cleanup temp root");
}

#[test]
fn install_git_path_library_and_consume_non_default_source_root() {
    let fixture = test_temp_dir("consumer-proof");
    let source_repo = write_library_repo(
        &fixture.join("source-lib"),
        "altmath-package",
        "altmath",
        "interfaces",
    );
    let library_home = fixture.join("library-home");

    let report = install_library(
        source_repo.to_str().expect("source path"),
        library_home.clone(),
    )
    .expect("install library");
    assert_eq!(report.provider, "altmath");
    assert!(report.target.join("interfaces/math/add.fab").is_file());
    assert!(!report.target.join("src/math/add.fab").exists());

    let app = fixture.join("app");
    write_app(&app, "altmath");
    let result = compile_package(&Config::default().with_stdlib(library_home), &app);
    assert!(
        result.success(),
        "expected app to import installed non-default source root, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn install_rejects_existing_provider_with_different_remote_or_identity() {
    let fixture = test_temp_dir("conflict");
    let first = write_library_repo(
        &fixture.join("first-lib"),
        "altmath-package",
        "altmath",
        "interfaces",
    );
    let second = write_library_repo(
        &fixture.join("second-lib"),
        "different-package",
        "altmath",
        "interfaces",
    );
    let library_home = fixture.join("library-home");

    install_library(first.to_str().expect("first path"), library_home.clone())
        .expect("first install");
    let err = install_library(second.to_str().expect("second path"), library_home)
        .expect_err("conflicting install should fail");
    assert!(
        matches!(err, InstallError::ConflictingInstall { .. }),
        "expected conflicting install error, got {err:?}"
    );
}
