use super::{
    lock_index, read_lock, validate_dependencies_against_lock, FaberLock, LockedPackage, LOCK_FILE,
};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("faber-lockfile-{label}-{nonce}"));
    fs::create_dir_all(&path).expect("create temp dir");
    path
}

fn package(name: &str, version: &str) -> LockedPackage {
    LockedPackage {
        name: name.to_owned(),
        version: version.to_owned(),
        source: "path".to_owned(),
        package_root: "lib".to_owned(),
        kind: "lib".to_owned(),
        target_language: "rust".to_owned(),
        target_triple: "host".to_owned(),
        target_manifest: String::new(),
        interface_root: "lib/src".to_owned(),
        artifact: String::new(),
        crate_name: name.replace('-', "_"),
        rustc: String::new(),
    }
}

fn duplicate_lock_source() -> String {
    r#"
[[package]]
name = "liba"
version = "1.0.0"
source = "path"
package_root = "lib-a"
kind = "lib"
target_language = "rust"
target_triple = "host"
target_manifest = ""
interface_root = "lib-a/src"
artifact = ""
crate = "liba"
rustc = ""

[[package]]
name = "liba"
version = "2.0.0"
source = "path"
package_root = "lib-b"
kind = "lib"
target_language = "rust"
target_triple = "host"
target_manifest = ""
interface_root = "lib-b/src"
artifact = ""
crate = "liba"
rustc = ""
"#
    .to_owned()
}

fn has_issue(diagnostics: &[radix::Diagnostic], issue: &str) -> bool {
    diagnostics
        .iter()
        .any(|diagnostic| diagnostic.issue() == Some(issue))
}

#[test]
fn read_lock_rejects_duplicate_package_names() {
    let root = temp_dir("read-duplicate");
    fs::write(root.join(LOCK_FILE), duplicate_lock_source()).expect("write lock");

    let diagnostic = read_lock(&root).expect_err("duplicate package name should fail");

    assert_eq!(diagnostic.issue(), Some("duplicate_locked_package"));
    assert!(
        diagnostic.message.contains("liba"),
        "diagnostic should name duplicate package: {diagnostic:?}"
    );
}

#[test]
fn lock_index_rejects_duplicate_package_names() {
    let root = temp_dir("index-duplicate");
    let lock = FaberLock {
        packages: vec![package("liba", "1.0.0"), package("liba", "2.0.0")],
    };

    let diagnostics = lock_index(&root.join(LOCK_FILE), &lock).expect_err("duplicate index");

    assert!(has_issue(&diagnostics, "duplicate_locked_package"));
}

#[test]
fn dependency_validation_rejects_duplicate_package_names_before_version_match() {
    let root = temp_dir("validate-duplicate");
    let lock = FaberLock {
        packages: vec![package("liba", "1.0.0"), package("liba", "2.0.0")],
    };
    let dependencies = BTreeMap::from([("liba".to_owned(), "2.0.0".to_owned())]);

    let diagnostics = validate_dependencies_against_lock(&root, &dependencies, Some(&lock));

    assert!(has_issue(&diagnostics, "duplicate_locked_package"));
    assert!(!has_issue(&diagnostics, "dependency_version_mismatch"));
}
