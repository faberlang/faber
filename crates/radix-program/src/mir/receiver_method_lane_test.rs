//! RM-1: same-unit receiver methods on the package test-link lane.
//!
//! A linked sibling unit that calls its own class methods (`v.longitudo()`,
//! `self.quadratum()`) must lower and execute under `with_linked_test_package_mir`.
//! Cross-unit method calls, proba-root methods, and the run/build lanes are
//! already green; this lock is the sibling-unit hole.

use super::*;
use radix_mir_runner::BufferHost;

#[test]
fn package_test_same_unit_receiver_methods_execute() {
    let dir = crate::test_support::test_temp_dir("same-unit-receiver-method");
    let package_root = dir.join("libhome").join("recvlib");
    let src = package_root.join("src");
    fs::create_dir_all(&src).expect("create same-unit receiver package src");
    fs::write(
        package_root.join("faber.toml"),
        r#"
[package]
name = "recvlib"
version = "1.0.0"
edition = "2026"

[library]
provider = "recvlib"

[paths]
source = "src"

[build]
kind = "lib"
targets = ["fmir"]

[reader]
locale = "en"
"#,
    )
    .expect("write recvlib faber.toml");
    // Sibling unit: class methods called from the same unit (value receiver,
    // `self.other()`, and a `ref` receiver). 3²+4²+0² = 25 and 3·3+4·4+0 = 25
    // are the executed-value oracles.
    fs::write(
        src.join("math.fab"),
        r"
@ public
class Vector3 {
    mut int x
    mut int y
    mut int z

    fn quadratum() → int {
        return self.x * self.x + self.y * self.y + self.z * self.z
    }

    fn longitudo() → int {
        return self.quadratum()
    }

    fn productum(ref Vector3 alter) → int {
        return self.x * alter.x + self.y * alter.y + self.z * alter.z
    }
}

@ public
fn probe(Vector3 v) → int {
    return v.longitudo()
}

@ public
fn dot(ref Vector3 axis) → int {
    return axis.productum(axis)
}

@ public
fn sample() → Vector3 {
    return Vector3 { x = 3, y = 4, z = 0 }
}
",
    )
    .expect("write math.fab");
    fs::write(
        src.join("main.proba"),
        r#"+++
locale = "en"
+++
import from "recvlib:math" math

fn prove_same_unit_method() {
    let math.Vector3 v ← math.sample()
    print math.probe(v)
    print math.dot(v)
}
"#,
    )
    .expect("write main.proba");

    let config = Config::default().with_library_home(dir.join("libhome"));
    let mut package = crate::analyze::analyze_package_for_tests(
        &config,
        &package_root,
        None,
        &crate::test_support::test_analysis_seams(),
    )
    .expect("analyze same-unit receiver package tests");
    // Mirror `faber test`: isolate the proba root's local import closure and
    // name it as the package entry so remap_anchor is the test unit, not a
    // library sibling (the hole is non-root siblings on that lane).
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
        let is_proba = unit
            .path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext == "proba");
        if !is_proba {
            continue;
        }
        for item in &unit.analysis.hir.items {
            if let HirItemKind::Function(function) = &item.kind
                && unit.analysis.interner.resolve(function.name) == "prove_same_unit_method"
            {
                functions.push((unit.path.clone(), item.def_id));
            }
        }
    }
    assert_eq!(
        functions.len(),
        1,
        "expected one prove_same_unit_method case"
    );
    let scope = TestPackageScope::for_program(&package, functions);
    let output = with_linked_test_package_mir(
        package,
        &scope,
        &config,
        &crate::test_support::test_mir_seams(),
        |validated, function_ids| {
            let mut host = BufferHost::default();
            for function_id in function_ids.values() {
                radix_mir_runner::run_function(validated, *function_id, &mut host)
                    .expect("same-unit receiver methods must execute on the test-link lane");
            }
            host.stdout_lines
        },
    )
    .unwrap_or_else(|diagnostics| {
        panic!(
            "same-unit receiver methods must lower on the test-link lane: {:?}",
            diagnostics
                .iter()
                .map(|diag| (diag.code, diag.issue()))
                .collect::<Vec<_>>()
        )
    });
    assert_eq!(
        output,
        vec!["25".to_owned(), "25".to_owned()],
        "probe/dot must run same-unit value and ref-receiver methods (3²+4²+0² / 3·3+4·4)"
    );
}
