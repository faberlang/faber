use super::{
    FaberLock, LOCK_FILE, LockedPackage, lock_index, read_lock, validate_dependencies_against_lock,
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
        content_sha256: None,
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

fn has_issue(diagnostics: &[radix_module::Diagnostic], issue: &str) -> bool {
    diagnostics
        .iter()
        .any(|diagnostic| diagnostic.issue() == Some(issue))
}

#[test]
fn read_lock_rejects_duplicate_package_names() {
    let root = temp_dir("read-duplicate");
    fs::write(root.join(LOCK_FILE), duplicate_lock_source()).expect("write lock");

    let diagnostic = read_lock(&root).expect_err("duplicate package name should fail");

    assert_eq!(diagnostic.code, Some("PKG001"));
    assert_eq!(diagnostic.issue(), Some("duplicate_locked_package"));
    assert_eq!(
        diagnostic
            .args
            .iter()
            .find(|arg| arg.name == "package")
            .map(|arg| arg.value.as_str()),
        Some("liba")
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

/// SHA-256 of the empty string; used as a well-formed stand-in digest.
const VALID_DIGEST: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

fn single_package_lock_source(content_sha256: Option<&str>) -> String {
    let digest_row = content_sha256
        .map(|digest| format!("content_sha256 = \"{digest}\"\n"))
        .unwrap_or_default();
    format!(
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
{digest_row}"#
    )
}

fn package_arg(diagnostic: &radix_module::Diagnostic) -> Option<&str> {
    diagnostic
        .args
        .iter()
        .find(|arg| arg.name == "package")
        .map(|arg| arg.value.as_str())
}

#[test]
fn read_lock_accepts_content_sha256_round_trip() {
    let root = temp_dir("digest-round-trip");
    fs::write(
        root.join(LOCK_FILE),
        single_package_lock_source(Some(VALID_DIGEST)),
    )
    .expect("write lock");

    let lock = read_lock(&root).expect("lock with content_sha256 must parse");

    let locked = &lock.expect("lock present").packages[0];
    assert_eq!(locked.content_sha256.as_deref(), Some(VALID_DIGEST));
}

#[test]
fn read_lock_tolerates_absent_content_sha256() {
    let root = temp_dir("digest-absent");
    fs::write(root.join(LOCK_FILE), single_package_lock_source(None)).expect("write lock");

    let lock = read_lock(&root).expect("pre-digest lock must keep reading");

    let locked = &lock.expect("lock present").packages[0];
    assert!(
        locked.content_sha256.is_none(),
        "absent digest defaults to none"
    );
}

#[test]
fn read_lock_rejects_malformed_content_sha256_length() {
    let root = temp_dir("digest-short");
    fs::write(
        root.join(LOCK_FILE),
        single_package_lock_source(Some("abcd")),
    )
    .expect("write lock");

    let diagnostic = read_lock(&root).expect_err("short digest must fail closed");

    assert_eq!(diagnostic.issue(), Some("locked_content_sha256_malformed"));
    assert_eq!(package_arg(&diagnostic), Some("liba"));
}

#[test]
fn read_lock_rejects_malformed_content_sha256_characters() {
    let root = temp_dir("digest-nonhex");
    fs::write(
        root.join(LOCK_FILE),
        single_package_lock_source(Some(&"g".repeat(64))),
    )
    .expect("write lock");

    let diagnostic = read_lock(&root).expect_err("non-hex digest must fail closed");

    assert_eq!(diagnostic.issue(), Some("locked_content_sha256_malformed"));
    assert_eq!(package_arg(&diagnostic), Some("liba"));
}
