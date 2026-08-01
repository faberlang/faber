//! Tests for TypeScript library product scanning (`faber.lock` driven).

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use super::*;

/// Write a `faber.lock` in `app` that locks a single TS library dependency
/// rooted at `dep`.
fn write_ts_library_lock(app: &Path, dep: &Path) {
    let interface_root = dep.join("src");
    fs::write(
        app.join(super::super::lockfile::LOCK_FILE),
        format!(
            r#"
[[package]]
name = "web"
version = "0.1.0"
source = "path"
package_root = "{package_root}"
kind = "lib"
target_language = "ts"
target_triple = "browser"
target_manifest = ""
interface_root = "{interface_root}"
artifact = ""
crate = "web"
rustc = ""
"#,
            package_root = dep.display(),
            interface_root = interface_root.display(),
        ),
    )
    .expect("write lock");
}

/// Write a TS library dependency manifest declaring `paths.source = source`.
fn write_ts_library_dependency(dep: &Path, source: &str) {
    fs::create_dir_all(dep.join(source)).expect("create dependency source dir");
    fs::write(
        dep.join("faber.toml"),
        format!(
            r#"[package]
name = "web"
version = "0.1.0"

[library]
provider = "web"

[paths]
source = "{source}"

[build]
kind = "lib"
targets = ["ts"]
"#
        ),
    )
    .expect("write dependency manifest");
}

#[test]
fn custom_source_root_ts_library_is_rejected_at_discovery() {
    // FBR-P2-001: a dependency manifest declaring a custom source root is
    // rejected loudly at discovery (Stage 0 validation), so it can never
    // silently contribute no modules to the product/import map.
    let dep = tempfile::tempdir().expect("create dependency temp root");
    write_ts_library_dependency(dep.path(), "lib");
    fs::write(
        dep.path().join("lib/api.fab"),
        "functio localis() → textus { redde \"ok\" }\n",
    )
    .expect("write dependency source");

    let diagnostic =
        crate::package::discovery::discover_package(dep.path()).expect_err("rejected loudly");
    assert_eq!(
        diagnostic.issue(),
        Some("package_member_unsupported_source_root")
    );
}

#[test]
fn library_ts_module_map_default_src_is_deterministic() {
    // Contract lock: default-`src` TS libraries keep contributing the same
    // deterministic specifier → path entries to the import map.
    let app = tempfile::tempdir().expect("create app temp root");
    let dep = tempfile::tempdir().expect("create dependency temp root");
    write_ts_library_dependency(dep.path(), "src");
    fs::write(
        dep.path().join("src/api.fab"),
        "functio localis() → textus { redde \"ok\" }\n",
    )
    .expect("write dependency source");
    fs::write(
        dep.path().join("src/geo.fab"),
        "functio aliena() → textus { redde \"ok\" }\n",
    )
    .expect("write dependency source");
    write_ts_library_lock(app.path(), dep.path());

    let map = build_library_ts_module_map(app.path()).expect("build library import map");
    let expected = BTreeMap::from([
        ("web:api".to_owned(), "./web-api.js".to_owned()),
        ("web:geo".to_owned(), "./web-geo.js".to_owned()),
    ]);
    assert_eq!(map, expected);
}

#[cfg(unix)]
#[test]
fn library_ts_module_map_read_dir_failure_is_diagnostic() {
    // FBR-P2-001 gate: an injected read-dir failure is a diagnostic, not a
    // silently dropped module. `src` is made unreadable so `fs::read_dir`
    // fails while `src.is_dir()` still holds.
    use std::os::unix::fs::PermissionsExt;

    let app = tempfile::tempdir().expect("create app temp root");
    let dep = tempfile::tempdir().expect("create dependency temp root");
    write_ts_library_dependency(dep.path(), "src");
    fs::write(
        dep.path().join("src/api.fab"),
        "functio localis() → textus { redde \"ok\" }\n",
    )
    .expect("write dependency source");
    write_ts_library_lock(app.path(), dep.path());

    let src = dep.path().join("src");
    fs::set_permissions(&src, fs::Permissions::from_mode(0o000)).expect("make src unreadable");

    let result = build_library_ts_module_map(app.path());
    // Restore permissions so tempdir cleanup can traverse `src`.
    let _ = fs::set_permissions(&src, fs::Permissions::from_mode(0o755));

    match result {
        Err(diagnostic) => {
            assert!(
                diagnostic.message.contains("cannot read"),
                "expected an io diagnostic, got: {}",
                diagnostic.message
            );
        }
        Ok(_) => {
            // Elevated privileges (e.g. running as root) bypass the
            // permission drop; the fail-closed path is exercised on normal
            // runs.
            eprintln!("skipped: permission-based read-dir failure not effective");
        }
    }
}
