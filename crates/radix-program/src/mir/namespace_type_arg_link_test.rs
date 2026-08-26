//! NS-TYPEARG-LINK: the namespace-call rewrite forwards call-site type
//! arguments onto the rewritten direct call (PAF-NS-002 burn-down).
//!
//! A size-generic function exported by a sibling package file must link and
//! instantiate through the package-MIR namespace-call rewrite both when the
//! caller spells the sizes (`utilModule.id<2>(t)`) and when the checker
//! infers them from the argument (`utilModule.id(t)`). Inferred call sites
//! arrive with empty HIR `type_args` — the semantic lowering writes them
//! only for spelled `<...>` arguments — so one forward-on-rewrite covers
//! both forms.

use super::*;
use radix_module::semantic::{IndexExpr, Type};

const TYPEARG_UTIL: &str = r#"@ public
fn id<size N>(tensor<f32, [N]> x) → tensor<f32, [N]> {
    return x
}
"#;

const TYPEARG_EXPLICIT_MAIN: &str = r#"import from "./util" * as utilModule

fn run() → f32 {
    const tensor<f32, [2]> t ← [1.5, 2.5] ↦ tensor<f32, [2]>
    const tensor<f32, [2]> r ← utilModule.id<2>(t)
    return r[1]
}

main {
    print run()
}
"#;

const TYPEARG_INFERRED_MAIN: &str = r#"import from "./util" * as utilModule

fn run() → f32 {
    const tensor<f32, [3]> t ← [1.5, 2.5, 3.5] ↦ tensor<f32, [3]>
    const tensor<f32, [3]> r ← utilModule.id(t)
    return r[2]
}

main {
    print run()
}
"#;

fn write_typearg_package(dir: &std::path::Path, main_source: &str) -> std::path::PathBuf {
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create package src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "ns-typearg-link"
version = "1.0.0"
edition = "2026"

[paths]
source = "src"
entry = "main.fab"

[build]
kind = "bin"
"#,
    )
    .expect("write faber.toml");
    fs::write(src.join("util.fab"), TYPEARG_UTIL).expect("write util.fab");
    fs::write(src.join("main.fab"), main_source).expect("write main.fab");
    src.join("main.fab")
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

fn first_param_shape(lowered: &LoweredMirUnit<'_>, function_name: &str) -> Option<Vec<i64>> {
    let types = lowered.validated.validation().types;
    lowered
        .program
        .functions
        .iter()
        .filter(|function| {
            function
                .name
                .is_some_and(|name| lowered.interner.resolve(name) == function_name)
        })
        .filter_map(|function| {
            let param = function.params.first()?;
            match types.get(param.ty.semantic_id()) {
                Type::Tensor(_, shape) => literal_dims(types, *shape),
                _ => None,
            }
        })
        .next()
}

#[test]
fn package_mir_links_explicit_type_arg_namespace_call() {
    let dir = crate::test_support::test_temp_dir("ns-typearg-explicit");
    let entry = write_typearg_package(dir.as_ref(), TYPEARG_EXPLICIT_MAIN);
    let config = Config::default();

    with_interpreted_lowered_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        |lowered| {
            assert_eq!(
                first_param_shape(lowered, "id"),
                Some(vec![2]),
                "explicit `utilModule.id<2>` must instantiate the sibling generic at [2]"
            );
        },
    )
    .unwrap_or_else(|errors| {
        panic!(
            "explicit type-arg namespace call must link, got {:?}",
            errors
        )
    });
}

#[test]
fn package_mir_links_inferred_type_arg_namespace_call() {
    // Inferred sites write no HIR type args; the checker unifies N from the
    // argument. The rewrite must carry them through the same empty-args
    // path without the retired explicit-only rejection.
    let dir = crate::test_support::test_temp_dir("ns-typearg-inferred");
    let entry = write_typearg_package(dir.as_ref(), TYPEARG_INFERRED_MAIN);
    let config = Config::default();

    with_interpreted_lowered_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        |lowered| {
            assert_eq!(
                first_param_shape(lowered, "id"),
                Some(vec![3]),
                "inferred `utilModule.id(t)` must instantiate the sibling generic at [3]"
            );
        },
    )
    .unwrap_or_else(|errors| {
        panic!(
            "inferred type-arg namespace call must link, got {:?}",
            errors
        )
    });
}
