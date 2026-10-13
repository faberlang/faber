//! MIXED-SHAPE-MIR-LINK: package-MIR instantiation must distinguish shapes.
//!
//! A library generic `_sgd_family<size S>` instantiated at [2, 2] inside the
//! library and at [8, 8] / [8] from the consumer must not collapse to one
//! signature. That is the BERT `train_step_bert_*` → `optimize._sgd_family`
//! pattern.

use super::*;
use radix_module::semantic::{IndexExpr, Type};

const OPTIMIZE_LIB: &str = r#"+++
locale = "en"
+++
@ public { }
fn family<size Figura>(list<tensor<f32, Figura>> params, list<tensor<f32, Figura>> grads, f32 lr) → list<tensor<f32, Figura>> {
    var list<tensor<f32, Figura>> out ← empty
    for range 0‥params.length() const i {
        out.append(params[i] - grads[i] * lr)
    }
    return out
}

@ public { }
fn step_2x2(tensor<f32, [2, 2]> param, tensor<f32, [2, 2]> grad, f32 lr) → tensor<f32, [2, 2]> {
    const list<tensor<f32, [2, 2]>> out ← family([param], [grad], lr)
    return out[0]
}
"#;

const TRAIN_MAIN: &str = r#"+++
locale = "en"
+++
import from "optlib:optimize" optimize

main {
    const tensor<f32, [2, 2]> a ← empty
    const tensor<f32, [8, 8]> b ← empty
    const tensor<f32, [8]> c ← empty
    const f32 lr ← 0.01 ∷ f32
    const _ r2 ← optimize.step_2x2(a, a, lr)
    const list<tensor<f32, [8, 8]>> r88 ← optimize.family([b], [b], lr)
    const list<tensor<f32, [8]>> r8 ← optimize.family([c], [c], lr)
    print r88.length()
    print r8.length()
}
"#;

fn write_mixed_shape_package(dir: &std::path::Path) -> std::path::PathBuf {
    let src = dir.join("src");
    let libhome = dir.join("libhome");
    let library = libhome.join("optlib");
    fs::create_dir_all(&src).expect("create package src");
    fs::create_dir_all(library.join("src")).expect("create library src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "mixed-shape-link"
version = "1.0.0"
edition = "2026"

[paths]
source = "src"
entry = "main.fab"

[build]
kind = "bin"

[dependencies]
optlib = "1.0.0"
"#,
    )
    .expect("write faber.toml");
    let library_display = library.display();
    let lock = format!(
        "[[package]]\nname = \"optlib\"\nversion = \"1.0.0\"\nsource = \"path:{library_display}\"\npackage_root = \"{library_display}\"\nkind = \"source\"\ntarget_language = \"\"\ntarget_triple = \"\"\ntarget_manifest = \"\"\ninterface_root = \"{library_display}/src\"\nartifact = \"\"\ncrate = \"\"\nrustc = \"\"\n"
    );
    fs::write(dir.join("faber.lock"), lock).expect("write faber.lock");
    fs::write(library.join("src/optimize.fab"), OPTIMIZE_LIB).expect("write library module");
    fs::write(src.join("main.fab"), TRAIN_MAIN).expect("write main.fab");
    fs::write(src.join("train.proba"), MIXED_SHAPE_PROBA).expect("write train.proba");
    src.join("main.fab")
}

const MIXED_SHAPE_PROBA: &str = r#"+++
locale = "en"
+++
import from "optlib:optimize" optimize

fn prove_mixed_shape_family() {
    const tensor<f32, [8, 8]> b ← empty
    const tensor<f32, [8]> c ← empty
    const f32 lr ← 0.01 ∷ f32
    const list<tensor<f32, [8, 8]>> r88 ← optimize.family([b], [b], lr)
    const list<tensor<f32, [8]>> r8 ← optimize.family([c], [c], lr)
    print r88.length()
    print r8.length()
}
"#;

fn tensor_shape_of_first_param(
    types: &radix_types::TypeTable,
    ty: radix_mir::MirType,
) -> Option<Vec<i64>> {
    match types.get(ty.semantic_id()) {
        Type::Array(elem) | Type::Set(elem) => {
            tensor_shape_of_first_param(types, radix_mir::MirType::semantic(*elem))
        }
        Type::Tensor(_, shape) => literal_dims(types, *shape),
        _ => None,
    }
}

fn literal_dims(
    types: &radix_types::TypeTable,
    shape: radix_module::semantic::IndexId,
) -> Option<Vec<i64>> {
    match types.get_index(shape) {
        IndexExpr::Literal(value) => Some(vec![i64::try_from(*value).ok()?]),
        IndexExpr::Tuple(dims) => dims
            .iter()
            .map(|dim| match types.get_index(*dim) {
                IndexExpr::Literal(value) => i64::try_from(*value).ok(),
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

#[test]
fn package_mir_mixed_shape_generic_instances_are_distinct() {
    let dir = crate::test_support::test_temp_dir("mixed-shape-link");
    let entry = write_mixed_shape_package(dir.as_ref());
    let config = Config::default().with_library_home(dir.join("libhome"));

    with_interpreted_lowered_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        |lowered| {
            let types = lowered.validated.validation().types.as_table();
            let mut family_shapes = lowered.validated.program()
                .functions
                .iter()
                .filter(|function| {
                    function
                        .name
                        .is_some_and(|name| lowered.interner.resolve(name) == "family")
                })
                .filter_map(|function| {
                    function
                        .params
                        .first()
                        .and_then(|param| tensor_shape_of_first_param(types, param.ty))
                })
                .collect::<Vec<_>>();
            family_shapes.sort();
            family_shapes.dedup();
            assert!(
                family_shapes.iter().any(|shape| shape == &vec![2, 2]),
                "library [2, 2] instance must survive, got {family_shapes:?}"
            );
            assert!(
                family_shapes.iter().any(|shape| shape == &vec![8, 8]),
                "consumer [8, 8] instance must not collapse onto [2, 2], got {family_shapes:?}"
            );
            assert!(
            family_shapes.iter().any(|shape| shape == &vec![8]),
            "consumer [8] instance must not collapse onto [2, 2] or [8, 8], got {family_shapes:?}"
        );
        },
    )
    .unwrap_or_else(|errors| panic!("mixed-shape package MIR must link, got {:?}", errors));
}

#[test]
fn package_mir_mixed_shape_generic_links_through_test_package() {
    // The BERT `faber test` route uses `with_linked_test_package_mir`, not
    // the interpreted `main` entry. Same mixed-shape family must link there.
    let dir = crate::test_support::test_temp_dir("mixed-shape-test-link");
    let entry = write_mixed_shape_package(dir.as_ref());
    let package_root = entry.parent().unwrap().parent().unwrap();
    let config = Config::default().with_library_home(dir.join("libhome"));
    let package = crate::analyze::analyze_package_for_tests(
        &config,
        package_root,
        None,
        &crate::test_support::test_analysis_seams(),
    )
    .expect("analyze mixed-shape package tests");
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
                && unit.analysis.interner.resolve(function.name) == "prove_mixed_shape_family"
            {
                functions.push((unit.path.clone(), item.def_id));
            }
        }
    }
    assert_eq!(
        functions.len(),
        1,
        "expected one prove_mixed_shape_family case"
    );
    let scope = TestPackageScope::for_program(&package, functions);
    with_linked_test_package_mir(
        package,
        &scope,
        &config,
        &crate::test_support::test_mir_seams(),
        |_validated, _function_ids| (),
    )
    .unwrap_or_else(|errors| panic!("mixed-shape test-package MIR must link, got {:?}", errors));
}
