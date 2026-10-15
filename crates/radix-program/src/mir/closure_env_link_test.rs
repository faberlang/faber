//! Closure-environment package-link lane (need, task):
//! a SIBLING unit's capture-closure must merge into one validated program.
//!
//! Two faces of one seam, both diagnosed under a8dfb5fc:
//!   * face 1 — `append_shifted_parts` re-keys the merged closure-environment
//!     registry past the merged prefix, but `shift_aggregate_ids` walked only
//!     `aggregate.fields`, never `MirAggregateKind::ClosureEnvironment(id)`.
//!     A sibling unit's `construct closure_env envN {..}` kept the pre-merge
//!     id while the registry key became `env(N + offset)` → "closure
//!     environment aggregate is missing metadata" (radix-mir validate).
//!   * face 2 — `rewrite_program_sources` rewrote the closure BODY's
//!     `MirProjection::ClosureCapture { source }` DefId but not the unit's
//!     `closure_environments[].captures[].source` metadata → body and
//!     environment row disagreed on the capture DefId → "closure capture
//!     projection is missing metadata".
//!
//! The entry unit merges first with offset 0, which is why entry-owned
//! closures always linked; the controls below pin that seam.

use super::*;
use radix_mir_runner::BufferHost;

fn write_self_provider_package(
    dir: &Path,
    provider: &str,
    modules: &[(&str, &str)],
    proba: &str,
) -> PathBuf {
    let package_root = dir.join("libhome").join(provider);
    let src = package_root.join("src");
    fs::create_dir_all(&src).expect("create closure-env package src");
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
    for (name, body) in modules {
        fs::write(src.join(format!("{name}.fab")), body).expect("write sibling module");
    }
    fs::write(src.join("main.proba"), proba).expect("write main.proba");
    src
}

/// External-library twin of the sibling layout (mixed-shape link shape): the
/// capture-closure lives in a lock-pinned path library, so its parts merge
/// through the `append_shifted_parts` library route, not the pending-unit
/// route.
fn write_library_dep_package(dir: &Path, dep: &str, dep_modules: &[(&str, &str)], proba: &str) {
    let library = dir.join("libhome").join(dep);
    fs::create_dir_all(library.join("src")).expect("create library src");
    fs::write(
        library.join("faber.toml"),
        format!(
            r#"
[package]
name = "{dep}"
version = "1.0.0"
edition = "2026"

[library]
provider = "{dep}"

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
    .expect("write library faber.toml");
    for (name, body) in dep_modules {
        fs::write(library.join("src").join(format!("{name}.fab")), body)
            .expect("write library module");
    }
    let package_root = dir.join("pkg");
    fs::create_dir_all(package_root.join("src")).expect("create package src");
    fs::write(
        package_root.join("faber.toml"),
        format!(
            r#"
[package]
name = "closure-env-lib-link"
version = "1.0.0"
edition = "2026"

[paths]
source = "src"
entry = "main.fab"

[dependencies]
{dep} = "1.0.0"

[build]
kind = "bin"

[locale]
locale = "en"
"#
        ),
    )
    .expect("write package faber.toml");
    let library_display = library.display();
    fs::write(
        package_root.join("faber.lock"),
        format!(
            "[[package]]\nname = \"{dep}\"\nversion = \"1.0.0\"\nsource = \"path:{library_display}\"\npackage_root = \"{library_display}\"\nkind = \"source\"\ntarget_language = \"\"\ntarget_triple = \"\"\ntarget_manifest = \"\"\ninterface_root = \"{library_display}/src\"\nartifact = \"\"\ncrate = \"\"\nrustc = \"\"\n"
        ),
    )
    .expect("write faber.lock");
    fs::write(
        package_root.join("src").join("main.fab"),
        r#"+++
locale = "en"
+++

main {
    print 0
}
"#,
    )
    .expect("write main.fab");
    fs::write(package_root.join("src").join("main.proba"), proba).expect("write main.proba");
}

/// Mirror `faber test`: isolate the proba root's local import closure, name it
/// the package entry, and run every proba test item through the linked runner.
fn run_closure_env_package_tests(package_root: &Path, stdlib_home: &Path) -> Vec<String> {
    let config = Config::default().with_library_home(stdlib_home.to_path_buf());
    let mut package = crate::analyze::analyze_package_for_tests(
        &config,
        package_root,
        None,
        &crate::test_support::test_analysis_seams(),
    )
    .unwrap_or_else(|diagnostics| panic!("analyze closure-env package: {:?}", diagnostics));
    let proba_path = package_root.join("src").join("main.proba");
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
                    .expect("closure-env proba test must execute on the test-link lane");
            }
            host.stdout_lines
        },
    )
    .unwrap_or_else(|diagnostics| {
        panic!(
            "closure-env package must lower on the test-link lane: {:?}",
            diagnostics.iter().map(|diag| diag.code).collect::<Vec<_>>()
        )
    })
}

fn assert_true(output: &[String], context: &str) {
    assert_eq!(
        output,
        &["true".to_owned()],
        "{context} must run the linked sibling closure"
    );
}

const G_PARAM_CAPTURE: &str = r#"+++
locale = "en"
+++

fn _cut(list<u8> b, int start, int length) → int {
    return start + length
}

@ public
fn inspect_wrap(list<u8> b) → int {
    return _fold((int start, int length) → int ∴ _cut(b, start, length), b.length())
}

fn _fold((int, int) → int f, int n) → int {
    return f(1, n)
}
"#;

/// Face 1: the sibling unit owns a closure capturing a PARAM. The sibling
/// merges with id offset > 0, so the `construct closure_env` aggregate kind
/// must shift with the re-keyed environment registry.
#[test]
fn package_test_sibling_param_capture_closure_links() {
    let dir = crate::test_support::test_temp_dir("closure-env-sibling-param");
    let src = write_self_provider_package(
        dir.as_ref(),
        "rlib",
        &[("g", G_PARAM_CAPTURE)],
        r#"+++
locale = "en"
+++

import from "rlib:g" g

test "sibling closure captures param" {
    let list<u8> b ← [1, 2, 3]
    let int out ← g.inspect_wrap(b)
    print out ≡ 4
    assert out ≡ 4
}
"#,
    );
    let package_root = src.parent().unwrap().to_path_buf();
    let output = run_closure_env_package_tests(&package_root, package_root.parent().unwrap());
    assert_true(&output, "sibling param capture");
}

/// Face 1 through a sibling LOCAL capture (same shift seam, distinct capture
/// shape from the param row above).
#[test]
fn package_test_sibling_local_capture_closure_links() {
    let dir = crate::test_support::test_temp_dir("closure-env-sibling-local");
    let src = write_self_provider_package(
        dir.as_ref(),
        "rlib",
        &[(
            "g",
            r#"+++
locale = "en"
+++

fn _add3(int a, int b, int base) → int {
    return a + b + base
}

@ public
fn inspect_local(int n) → int {
    let int base ← 2
    return _fold((int a, int b) → int ∴ _add3(a, b, base), n)
}

fn _fold((int, int) → int f, int n) → int {
    return f(1, n)
}
"#,
        )],
        r#"+++
locale = "en"
+++

import from "rlib:g" g

test "sibling closure captures local" {
    let int out ← g.inspect_local(1)
    print out ≡ 4
    assert out ≡ 4
}
"#,
    );
    let package_root = src.parent().unwrap().to_path_buf();
    let output = run_closure_env_package_tests(&package_root, package_root.parent().unwrap());
    assert_true(&output, "sibling local capture");
}

const G_CONST_CAPTURE: &str = r#"+++
locale = "en"
+++

@ public
const int LIMIT = 8

fn _add(int start, int base) → int {
    return start + base
}

@ public
fn inspect_limit(int start) → int {
    return _fold((int a, int b) → int ∴ _add(a, LIMIT), start)
}

fn _fold((int, int) → int f, int n) → int {
    return f(1, n)
}
"#;

/// Face 2 (with face 1 riding): the sibling closure captures its OWN exported
/// module const. The const's DefId is a `source_rewrites` key (linked const
/// data member), so the closure body's capture projection is rewritten to the
/// synthetic id while the environment's capture row kept the original — body
/// and metadata must stay in sync.
#[test]
fn package_test_sibling_module_const_capture_closure_links() {
    let dir = crate::test_support::test_temp_dir("closure-env-sibling-const");
    let src = write_self_provider_package(
        dir.as_ref(),
        "rlib",
        &[("g", G_CONST_CAPTURE)],
        r#"+++
locale = "en"
+++

import from "rlib:g" g

test "sibling closure captures module const" {
    let int out ← g.inspect_limit(1)
    print out ≡ 9
    assert out ≡ 9
}
"#,
    );
    let package_root = src.parent().unwrap().to_path_buf();
    let output = run_closure_env_package_tests(&package_root, package_root.parent().unwrap());
    assert_true(&output, "sibling module-const capture");
}

/// Face 1 only: the sibling closure reads a THIRD unit's exported const
/// (`keys.LIMIT`). The HIR-level member rewrite keeps the capture DefId
/// consistent between closure body and environment metadata (synthetic on both
/// sides), so only the aggregate-kind shift can break this link.
#[test]
fn package_test_sibling_cross_unit_const_capture_closure_links() {
    let dir = crate::test_support::test_temp_dir("closure-env-sibling-cross");
    let src = write_self_provider_package(
        dir.as_ref(),
        "rlib",
        &[
            (
                "keys",
                r#"+++
locale = "en"
+++

@ public
const int LIMIT = 8
"#,
            ),
            (
                "g",
                r#"+++
locale = "en"
+++

import from "rlib:keys" keys

fn _add(int start, int base) → int {
    return start + base
}

@ public
fn inspect_cross(int start) → int {
    return _fold((int a, int b) → int ∴ _add(a, keys.LIMIT), start)
}

fn _fold((int, int) → int f, int n) → int {
    return f(1, n)
}
"#,
            ),
        ],
        r#"+++
locale = "en"
+++

import from "rlib:g" g

test "sibling closure captures cross-unit const" {
    let int out ← g.inspect_cross(1)
    print out ≡ 9
    assert out ≡ 9
}
"#,
    );
    let package_root = src.parent().unwrap().to_path_buf();
    let output = run_closure_env_package_tests(&package_root, package_root.parent().unwrap());
    assert_true(&output, "sibling cross-unit const capture");
}

/// Control (green before the fix): the same capture-closure lowered inside the
/// ENTRY unit merges with offset 0, so the environment registry and the
/// aggregate kind always agreed. Pins the seam the red faces ride.
#[test]
fn package_test_entry_unit_capture_closure_control() {
    let dir = crate::test_support::test_temp_dir("closure-env-entry-control");
    let src = write_self_provider_package(
        dir.as_ref(),
        "rlib",
        &[],
        r#"+++
locale = "en"
+++
fn _cut(list<u8> b, int start, int length) → int {
    return start + length
}

fn _fold((int, int) → int f, int n) → int {
    return f(1, n)
}

test "entry closure captures param" {
    let list<u8> b ← [1, 2, 3]
    let int out ← _fold((int start, int length) → int ∴ _cut(b, start, length), b.length())
    print out ≡ 4
    assert out ≡ 4
}
"#,
    );
    let package_root = src.parent().unwrap().to_path_buf();
    let output = run_closure_env_package_tests(&package_root, package_root.parent().unwrap());
    assert_true(&output, "entry-unit capture control");
}

/// Control (green before the fix): a sibling closure with NO captures builds
/// no environment aggregate, so it must keep linking regardless of the shift
/// seam.
#[test]
fn package_test_sibling_no_capture_closure_control() {
    let dir = crate::test_support::test_temp_dir("closure-env-no-capture-control");
    let src = write_self_provider_package(
        dir.as_ref(),
        "rlib",
        &[(
            "g",
            r#"+++
locale = "en"
+++

@ public
fn inspect_pure(int n) → int {
    return _fold((int start, int length) → int ∴ start + length, n)
}

fn _fold((int, int) → int f, int n) → int {
    return f(1, n)
}
"#,
        )],
        r#"+++
locale = "en"
+++

import from "rlib:g" g

test "sibling closure captures nothing" {
    let int out ← g.inspect_pure(3)
    print out ≡ 4
    assert out ≡ 4
}
"#,
    );
    let package_root = src.parent().unwrap().to_path_buf();
    let output = run_closure_env_package_tests(&package_root, package_root.parent().unwrap());
    assert_true(&output, "sibling no-capture control");
}

/// Library route (both faces): the capture-closure lives in a lock-pinned path
/// library, so its parts merge through the library `append_shifted_parts`
/// route (unshifted aggregate kinds and unrewritten environment metadata were
/// the same two defects there).
#[test]
fn package_test_library_const_capture_closure_links() {
    let dir = crate::test_support::test_temp_dir("closure-env-library-const");
    write_library_dep_package(
        dir.as_ref(),
        "clib",
        &[("ops", G_CONST_CAPTURE)],
        r#"+++
locale = "en"
+++

import from "clib:ops" ops

test "library closure captures const" {
    let int out ← ops.inspect_limit(1)
    print out ≡ 9
    assert out ≡ 9
}
"#,
    );
    let output = run_closure_env_package_tests(&dir.join("pkg"), &dir.join("libhome"));
    assert_true(&output, "library module-const capture");
}
