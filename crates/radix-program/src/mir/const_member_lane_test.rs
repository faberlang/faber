//! Package-MIR module-const lane: module-level collection
//! consts must be readable from function bodies under `with_linked_test_package_mir`.
//!
//! Two faces of one hole, plus the already-green same-unit control that the
//! need's done-when names:
//!   * face 1 — a SIBLING unit's function body reads another unit's exported
//!     module const (`k ∈ keys.KEYS`). The namespace member rewrite
//!     turned the read into `Path(synthetic)`, but the synthetic const item
//!     only existed in the entry analysis, so the sibling lowered
//!     "path that does not resolve to a local value".
//!   * face 2 — a linked const initializer carries the conversion shape
//!     (`[…] ↦ set<string>`); `transplant_expr` failed closed on it.
//!
//! The standalone `∈` operator is not under test here (radix-module owns
//! that proof); this lane pins the package transplant/link seam only.

use super::*;
use radix_mir_runner::BufferHost;

fn write_const_member_package(
    dir: &Path,
    provider: &str,
    keys_body: &str,
    reader_body: Option<&str>,
) -> PathBuf {
    let package_root = dir.join("libhome").join(provider);
    let src = package_root.join("src");
    fs::create_dir_all(&src).expect("create const-member package src");
    fs::write(
        package_root.join("faber.toml"),
        format!(
            r#"
[package]
name = "{provider}"
version = "1.0.0"
edition = "2026"

[library]
provider = "{provider}"

[paths]
source = "src"

[build]
kind = "lib"
targets = ["fmir"]

[locale]
locale = "en"
"#
        ),
    )
    .expect("write faber.toml");
    fs::write(src.join("keys.fab"), keys_body).expect("write keys.fab");
    if let Some(reader_body) = reader_body {
        fs::write(src.join("reader.fab"), reader_body).expect("write reader.fab");
    }
    src
}

/// Mirror `faber test`: isolate the proba root's local import closure, name it
/// the package entry, and run every proba test item through the linked runner.
fn run_const_member_package_tests(src: &Path) -> Vec<String> {
    let package_root = src.parent().unwrap().to_path_buf();
    let stdlib_home = package_root.parent().unwrap().to_path_buf();
    let config = Config::default().with_library_home(stdlib_home);
    let mut package = crate::analyze::analyze_package_for_tests(
        &config,
        &package_root,
        None,
        &crate::test_support::test_analysis_seams(),
    )
    .unwrap_or_else(|diagnostics| panic!("analyze const-member package: {:?}", diagnostics));
    let proba_path = src.join("main.proba");
    let (root_id, _) = package
        .node_by_path(&proba_path)
        .expect("main.proba is an analyzed node");
    let root_id = root_id.clone();
    let included = package.imports.local_closure(std::iter::once(&root_id));
    package.nodes.retain(|id, _| included.contains(id));
    package.entry = Some(root_id.clone());
    package.roots = vec![root_id];
    let mut functions = Vec::new();
    for unit in package.nodes.values() {
        if unit.path.extension().and_then(|ext| ext.to_str()) != Some("proba") {
            continue;
        }
        for item in &unit.analysis.hir.items {
            if let HirItemKind::Function(function) = &item.kind
                && function.test.is_some()
            {
                functions.push((unit.path.clone(), item.def_id));
            }
        }
    }
    assert!(
        !functions.is_empty(),
        "expected at least one proba test case"
    );
    let scope = TestPackageScope::for_program(&package, functions);
    with_linked_test_package_mir(
        package,
        &scope,
        &config,
        &crate::test_support::test_mir_seams(),
        |validated, function_ids| {
            let mut host = BufferHost::default();
            for function_id in function_ids.values() {
                radix_mir_runner::run_function(validated, *function_id, &mut host)
                    .expect("const-member proba test must execute on the test-link lane");
            }
            host.stdout_lines
        },
    )
    .unwrap_or_else(|diagnostics| {
        panic!(
            "const-member package must lower on the test-link lane: {:?}",
            diagnostics
        )
    })
}

const LIST_KEYS: &str = r#"+++
locale = "en"
+++

@ public
const list<string> KEYS = ["a", "b"]

@ public
fn admits(string k) → bool {
    return k ∈ KEYS
}
"#;

/// Need done-when shape (control): module const + same-unit `∈`
/// fn + proba true/false asserts already run on this lane; pinned here so the
/// two red faces below cannot regress the seam they ride.
#[test]
fn package_test_module_list_const_membership_runs() {
    let dir = crate::test_support::test_temp_dir("module-list-const-control");
    let src = write_const_member_package(dir.as_ref(), "llib", LIST_KEYS, None);
    fs::write(
        src.join("main.proba"),
        r#"+++
locale = "en"
+++
import from "llib:keys" keys

test "module const list membership" {
    assert keys.admits("a") ≡ true
    assert keys.admits("z") ≡ false
    print keys.admits("a")
    print keys.admits("z")
}
"#,
    )
    .expect("write main.proba");
    let output = run_const_member_package_tests(&src);
    assert_eq!(
        output,
        vec!["true".to_owned(), "false".to_owned()],
        "same-unit module list const must answer ∈ from the proba lane"
    );
}

/// Face 1: the READER is a sibling unit, not the entry. Its function body
/// reads another unit's exported module const through the namespace member;
/// the rewritten `Path(synthetic)` must materialize through the reader's own
/// top-level-const seam.
#[test]
fn package_test_sibling_fn_reads_module_list_const() {
    let dir = crate::test_support::test_temp_dir("sibling-reads-module-const");
    let src = write_const_member_package(
        dir.as_ref(),
        "keyslane",
        LIST_KEYS,
        Some(
            r#"+++
locale = "en"
+++
import from "keyslane:keys" keys

@ public
fn admits_via_module(string k) → bool {
    return k ∈ keys.KEYS
}
"#,
        ),
    );
    fs::write(
        src.join("main.proba"),
        r#"+++
locale = "en"
+++
import from "keyslane:reader" reader
import from "keyslane:keys" keys

test "sibling fn reads module const" {
    assert reader.admits_via_module("a") ≡ true
    assert reader.admits_via_module("z") ≡ false
    assert keys.admits("a") ≡ true
    print reader.admits_via_module("a")
    print reader.admits_via_module("z")
}
"#,
    )
    .expect("write main.proba");
    let output = run_const_member_package_tests(&src);
    assert_eq!(
        output,
        vec!["true".to_owned(), "false".to_owned()],
        "sibling function bodies must read linked module consts through the transplant seam"
    );
}

/// Face 2 used to prove: the linked const initializer carries the conversion
/// shape (`[…] ↦ set<string>`); the transplant must carry the `↦` expression
/// instead of failing closed on it.
///
/// D5.7 retired top-level `const`/`fixum`/`varia` outright (SEM062). The
/// top-level equivalent, `static`, only accepts a compile-time-evaluable `=`
/// initializer (D5.9); `↦` is a runtime conversion and is not part of that
/// evaluable surface, and `static`'s own grammar accepts only `=`. There is
/// no way to write this fixture as legal Faber any more — assert the SEM062
/// rejection instead of the old transplant behavior.
#[test]
fn package_test_module_const_set_conversion_rejects_as_top_level_binding() {
    let dir = crate::test_support::test_temp_dir("module-const-set-conversion");
    let src = write_const_member_package(
        dir.as_ref(),
        "setlane",
        r#"+++
locale = "en"
+++

@ public
let set<string> KEYS ← ["a", "b"] ↦ set<string>

@ public
fn admits(string k) → bool {
    return k ∈ KEYS
}
"#,
        None,
    );
    fs::write(
        src.join("main.proba"),
        r#"+++
locale = "en"
+++
import from "setlane:keys" keys

test "module const set conversion membership" {
    assert keys.admits("a") ≡ true
    assert keys.admits("z") ≡ false
    print keys.admits("a")
    print keys.admits("z")
}
"#,
    )
    .expect("write main.proba");
    let package_root = src.parent().unwrap().to_path_buf();
    let stdlib_home = package_root.parent().unwrap().to_path_buf();
    let config = Config::default().with_library_home(stdlib_home);
    let Err(diagnostics) = crate::analyze::analyze_package_for_tests(
        &config,
        &package_root,
        None,
        &crate::test_support::test_analysis_seams(),
    ) else {
        panic!("top-level const with a `↦` initializer must fail closed at analysis (SEM062)")
    };
    assert!(
        diagnostics
            .iter()
            .any(|diag| diag.issue() == Some("top_level_binding")),
        "expected top_level_binding rejection: {:?}",
        diagnostics
            .iter()
            .map(|d| (d.code, d.issue()))
            .collect::<Vec<_>>()
    );
}

/// A library reader is outside the application's local-node closure. Its
/// namespace const reads need bindings in the cached library analysis, not
/// just in the application entry or sibling analyses.
#[test]
fn package_test_library_fn_reads_module_consts() {
    let dir = crate::test_support::test_temp_dir("library-reads-module-const");
    write_const_member_package(
        dir.as_ref(),
        "constlib",
        r#"+++
locale = "en"
+++
@ public
const string schema_version = "2.0.0"
@ private
const int BASE = 20
@ public
const int ANSWER = BASE * 2 + 2
@ public
const list<string> KEYS = ["a", "b"]
"#,
        Some(
            r#"+++
locale = "en"
+++
import from "constlib:keys" keys

@ public
fn version() → string {
    return keys.schema_version
}
@ public
fn answer() → int {
    return keys.ANSWER
}
@ public
fn admits(string k) → bool {
    return k ∈ keys.KEYS
}
"#,
        ),
    );
    let src = write_const_member_package(dir.as_ref(), "consumer", "", None);
    fs::write(
        src.join("main.proba"),
        r#"+++
locale = "en"
+++
import from "constlib:reader" reader

test "library fn reads module consts" {
    assert reader.version() ≡ "2.0.0"
    assert reader.answer() ≡ 42
    assert reader.admits("a") ≡ true
    assert reader.admits("z") ≡ false
    print reader.version()
    print reader.answer()
    print reader.admits("a")
    print reader.admits("z")
}
"#,
    )
    .expect("write main.proba");
    assert_eq!(
        run_const_member_package_tests(&src),
        vec!["2.0.0", "42", "true", "false"],
        "library function bodies must receive linked const value bindings"
    );
}
