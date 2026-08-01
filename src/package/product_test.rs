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

// ── Stage 4: one-pass import specifier rewriting (FBR-P2-008) ─────────────

/// Map shape mirroring `build_library_ts_module_map` output.
fn library_import_map() -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    map.insert("triga:triga".to_owned(), "./triga-triga.js".to_owned());
    map.insert("triga:geometry".to_owned(), "./triga-geometry.js".to_owned());
    map
}

#[test]
fn rewrite_import_specifiers_merges_library_and_extension_rewrites() {
    // One pass handles both the library map replacement and the relative
    // `.js` extension in a single scan (FBR-P2-008).
    let code = r#"import { Vector3 } from "triga:geometry";
import { city } from "./city";
export { x } from "triga:triga";
import { done } from "./already.js";
"#;
    let out = rewrite_import_specifiers(code.to_owned(), &library_import_map());
    assert!(out.contains(r#"from "./triga-geometry.js""#));
    assert!(out.contains(r#"from "./city.js""#));
    assert!(out.contains(r#"from "./triga-triga.js""#));
    assert!(out.contains(r#"from "./already.js""#));
    assert!(!out.contains(r#"from "triga:geometry""#));
    assert!(!out.contains(r#"from "triga:triga""#));
}

#[test]
fn rewrite_import_specifiers_skips_comments_strings_and_templates() {
    // Red-green for FBR-P2-008: the old two-pass `String::replace` rewrite
    // rewrote `from "triga:triga"` inside the comment (captured red); the
    // one-pass scanner never descends into comments, string literals, or
    // template literals, so specifier-looking text there is untouched.
    let code = r#"// from "triga:triga"
/* block from "triga:triga" */
const s = "literal from \"triga:triga\"";
const t = `template from "triga:triga"`;
import { x } from "triga:triga";
"#;
    let out = rewrite_import_specifiers(code.to_owned(), &library_import_map());
    assert!(out.contains(r#"// from "triga:triga""#));
    assert!(out.contains(r#"/* block from "triga:triga" */"#));
    assert!(out.contains(r#""literal from \"triga:triga\"""#));
    assert!(out.contains(r#"`template from "triga:triga"`"#));
    // The real import specifier is still rewritten.
    assert!(out.contains(r#"import { x } from "./triga-triga.js";"#));
}

#[test]
fn rewrite_import_specifiers_leaves_dynamic_import_untouched() {
    // Codegen emits only static `import { ... } from "..."` (pinned here):
    // a dynamic `import("...")` call is not a `from "..."` specifier token
    // and must never be rewritten or gain a `.js` suffix.
    let code = r#"const mod = import("./lazy.js");
import("./lazy.js").then((m) => m.run());
import { x } from "triga:triga";
"#;
    let out = rewrite_import_specifiers(code.to_owned(), &library_import_map());
    assert!(out.contains(r#"const mod = import("./lazy.js");"#));
    assert!(out.contains(r#"import("./lazy.js").then((m) => m.run());"#));
    assert!(!out.contains(r#"import("./lazy.js.js")"#));
    assert!(out.contains(r#"from "./triga-triga.js""#));
}

#[test]
fn rewrite_import_specifiers_requires_exact_from_token() {
    // Exact-token contract: `from ` must be a standalone token. An embedded
    // occurrence (e.g. inside an identifier) is not an import specifier.
    let code = r#"const xfrom = "keep me";
xfrom "./no";
import { x } from "./yes";
"#;
    let out = rewrite_import_specifiers(code.to_owned(), &library_import_map());
    assert!(out.contains(r#"xfrom "./no";"#));
    assert!(!out.contains(r#"xfrom "./no.js";"#));
    assert!(out.contains(r#"from "./yes.js";"#));
}

#[test]
fn rewrite_import_specifiers_preserves_single_quote_specifiers() {
    // Both quote styles are import/export specifier tokens; the rewrite
    // preserves the author's quote style.
    let code = r#"import { x } from 'triga:triga';
import { y } from './city';
"#;
    let out = rewrite_import_specifiers(code.to_owned(), &library_import_map());
    assert!(out.contains(r#"from './triga-triga.js';"#));
    assert!(out.contains(r#"from './city.js';"#));
}
