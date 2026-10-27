//! GSF-1: symbolic size forwarding through package MIR.
//!
//! A generic caller (`forward<size T, size D>`) must pass its own symbolic
//! size witnesses on to a generic callee (`sink<size T, size D>(x)`), and the linked
//! package-MIR program must instantiate the callee at the caller's concrete
//! dimensions with a concrete tensor signature and the declared return shape.
//! The proof deliberately uses non-REF-01 dimensions (3, 5).

use super::*;
use radix_module::semantic::{IndexExpr, Type};

const GSF1_MAIN: &str = r"fn sink<size M, size N>(tensor<f32, [M, N]> x) → tensor<f32, [M, N]> {
    return x
}

fn forward<size T, size D>(tensor<f32, [T, D]> x) → tensor<f32, [T, D]> {
    return sink<size T, size D>(x)
}

main {
    let tensor<f32, [3, 5]> t ← empty
    let tensor<f32, [3, 5]> r ← forward<size 3, size 5>(t)
    let list<int, _> s ← r.shape()
    print s.length()
}
";

fn write_gsf1_package(dir: &std::path::Path) -> std::path::PathBuf {
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create package src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "gsf1-size-forwarding"
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
    fs::write(src.join("main.fab"), GSF1_MAIN).expect("write main.fab");
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

/// Parameter and return shapes of one function, when statically known.
type FunctionShapes = (Option<Vec<i64>>, Option<Vec<i64>>);

fn named_function_shapes(lowered: &LoweredMirUnit<'_>, function_name: &str) -> Vec<FunctionShapes> {
    let types = lowered.validated.validation().types.as_table();
    lowered
        .validated
        .program()
        .functions
        .iter()
        .filter(|function| {
            function
                .name
                .is_some_and(|name| lowered.interner.resolve(name) == function_name)
        })
        .map(|function| {
            let param_shape =
                function
                    .params
                    .first()
                    .and_then(|param| match types.get(param.ty.semantic_id()) {
                        Type::Tensor(_, shape) => literal_dims(types, *shape),
                        _ => None,
                    });
            let return_shape = match types.get(function.return_ty.semantic_id()) {
                Type::Tensor(_, shape) => literal_dims(types, *shape),
                _ => None,
            };
            (param_shape, return_shape)
        })
        .collect()
}

#[test]
fn package_mir_instantiates_forwarded_symbolic_size_callee() {
    let dir = crate::test_support::test_temp_dir("gsf1-size-forwarding");
    let entry = write_gsf1_package(dir.as_ref());
    let config = Config::default();

    with_interpreted_lowered_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        |lowered| {
            let sink_shapes = named_function_shapes(lowered, "sink");
            assert!(
                sink_shapes
                    .iter()
                    .any(|(param, ret)| param == &Some(vec![3, 5]) && ret == &Some(vec![3, 5])),
                "`sink<T, D>` forwarded from `forward<3, 5>` must instantiate at the concrete \
                 [3, 5] signature with the declared return shape, got {sink_shapes:?}"
            );
            let forward_shapes = named_function_shapes(lowered, "forward");
            assert!(
                forward_shapes
                    .iter()
                    .any(|(param, ret)| param == &Some(vec![3, 5]) && ret == &Some(vec![3, 5])),
                "`forward<3, 5>` must instantiate with the caller-side concrete [3, 5] \
                 signature, got {forward_shapes:?}"
            );
        },
    )
    .unwrap_or_else(|errors| {
        panic!(
            "symbolic size forwarding package must link and run, got {:?}",
            errors
        )
    });
}
