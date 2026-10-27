//! GSF-2: symbolic rank-2 tensor-target pin (`↦`) through package MIR.
//!
//! A generic `gather<size V, size D, size T>` pinning its result through
//! `flat ↦ tensor<f32, [T, D]>` must carry the declared rank and both
//! dimensions into the linked package-MIR instance: the `TensorFromFlat`
//! shape operand is materialized from shape-size terms at instantiation —
//! never from the flat payload length — and the cross-module runner proof
//! observes the declared `[T, D]` shape. Non-REF-01 dimensions (3, 4).

use super::*;
use radix_mir::{
    MirAggregateFields, MirAggregateItem, MirCollectionOp, MirConstant, MirIntrinsic, MirOperand,
    MirPlaceBase, MirStatementKind,
};
use radix_module::semantic::{IndexExpr, Type};

const PIN_UTIL: &str = r"@ public
fn gather<size V, size D, size T>(tensor<f32, [V, D]> table, list<int> ids) → tensor<f32, [T, D]> ⇥ string {
    let list<f32> flat ← table ↦ list<f32>
    return flat ↦ tensor<f32, [T, D]>
}
";

const PIN_MAIN: &str = r#"import from "./util" * as utilModule

main {
    let tensor<f32, [3, 4]> t ← [[1.0, 2.0, 3.0, 4.0], [5.0, 6.0, 7.0, 8.0], [9.0, 10.0, 11.0, 12.0]] ↦ tensor<f32, [3, 4]>
    let list<int> ids ← [0, 1, 2]
    do {
        let tensor<f32, [3, 4]> r ← utilModule.gather<size 3, size 4, size 3>(t, ids)
        let list<int, _> s ← r.shape()
        print s.length()
        print s[0]
        print s[1]
    }
    catch err {
        print "gather failed"
    }
}
"#;

const PIN_MAIN_WRONG_COUNT: &str = r#"import from "./util" * as utilModule

main {
    let tensor<f32, [3, 4]> t ← [[1.0, 2.0, 3.0, 4.0], [5.0, 6.0, 7.0, 8.0], [9.0, 10.0, 11.0, 12.0]] ↦ tensor<f32, [3, 4]>
    let list<int> ids ← [0, 1]
    do {
        let tensor<f32, [2, 4]> r ← utilModule.gather<size 3, size 4, size 2>(t, ids)
        let list<int, _> s ← r.shape()
        print s.length()
    }
    catch err {
        print "gather failed"
    }
}
"#;

fn write_pin_package(dir: &std::path::Path, main_source: &str) -> std::path::PathBuf {
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create package src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "gsf2-generic-pin"
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
    fs::write(src.join("util.fab"), PIN_UTIL).expect("write util.fab");
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

/// The instantiated `gather` imported at the concrete `[3, 4]` signature.
fn gather_instance<'a>(lowered: &'a LoweredMirUnit<'_>) -> &'a radix_mir::MirFunction {
    let types = lowered.validated.validation().types.as_table();
    lowered
        .validated
        .program()
        .functions
        .iter()
        .filter(|function| {
            function
                .name
                .is_some_and(|name| lowered.interner.resolve(name) == "gather")
        })
        .find(|function| {
            function
                .params
                .first()
                .is_some_and(|param| match types.get(param.ty.semantic_id()) {
                    Type::Tensor(_, shape) => literal_dims(types, *shape) == Some(vec![3, 4]),
                    _ => false,
                })
        })
        .expect("the imported generic gather must instantiate at the concrete [3, 4] signature")
}

/// One materialized shape-operand dimension: a constant, or a temp whose
/// defining runtime conversion carries the constant (the shape-size term
/// after package finalization).
fn operand_dim(function: &radix_mir::MirFunction, operand: &MirOperand) -> Option<i64> {
    match operand {
        MirOperand::Constant(MirConstant::Int(value)) => Some(*value),
        MirOperand::Constant(MirConstant::Float(value)) if value.fract() == 0.0 => {
            Some(*value as i64)
        }
        MirOperand::Temp(temp) => function
            .blocks
            .iter()
            .flat_map(|block| block.statements.iter())
            .find_map(|statement| {
                let MirStatementKind::RuntimeCall { destination, call } = &statement.kind else {
                    return None;
                };
                let destination = destination.as_ref()?;
                if !matches!(
                    destination.base,
                    MirPlaceBase::Temp(base) if base == *temp
                ) {
                    return None;
                }
                match call.intrinsic {
                    MirIntrinsic::Convert(_) => operand_dim(function, call.args.first()?),
                    _ => None,
                }
            }),
        _ => None,
    }
}

/// The dims of the `TensorFromFlat` shape operand of the flat pin path, as
/// materialized in the linked instance.
fn tensor_from_flat_shape_dims(function: &radix_mir::MirFunction) -> Option<Vec<i64>> {
    let shape_arg = function
        .blocks
        .iter()
        .flat_map(|block| block.statements.iter())
        .find_map(|statement| {
            let MirStatementKind::RuntimeCall { call, .. } = &statement.kind else {
                return None;
            };
            matches!(
                call.intrinsic,
                MirIntrinsic::Collection(MirCollectionOp::TensorFromFlat)
            )
            .then(|| call.args.get(2).cloned())?
        })?;
    let MirOperand::Temp(shape_temp) = shape_arg else {
        return None;
    };
    let items = function
        .blocks
        .iter()
        .flat_map(|block| block.statements.iter())
        .find_map(|statement| {
            let MirStatementKind::Construct {
                destination,
                aggregate,
            } = &statement.kind
            else {
                return None;
            };
            if !matches!(
                destination.base,
                MirPlaceBase::Temp(base) if base == shape_temp
            ) {
                return None;
            }
            match &aggregate.fields {
                MirAggregateFields::Ordered(items) => Some(items.clone()),
                _ => None,
            }
        })?;
    items
        .iter()
        .map(|item| match item {
            MirAggregateItem::Operand(operand) => operand_dim(function, operand),
            _ => None,
        })
        .collect()
}

#[test]
fn package_mir_bakes_symbolic_rank_two_pin_shape_operand() {
    let dir = crate::test_support::test_temp_dir("gsf2-generic-pin");
    let entry = write_pin_package(dir.as_ref(), PIN_MAIN);
    let config = Config::default();

    with_interpreted_lowered_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        |lowered| {
            let instance = gather_instance(lowered);
            assert_eq!(
                tensor_from_flat_shape_dims(instance),
                Some(vec![3, 4]),
                "the symbolic [T, D] pin must materialize both concrete dimensions in the \
                 TensorFromFlat shape operand, not a flat-length shape"
            );
        },
    )
    .unwrap_or_else(|errors| {
        panic!(
            "symbolic generic pin package must link and finalize, got {:?}",
            errors
        )
    });
}

#[test]
fn package_mir_runs_symbolic_rank_two_pin_at_declared_shape() {
    let dir = crate::test_support::test_temp_dir("gsf2-generic-pin-run");
    let entry = write_pin_package(dir.as_ref(), PIN_MAIN);
    let config = Config::default();

    let mut host = radix_mir_runner::BufferHost::default();
    run_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        &mut host,
    )
    .expect("the imported symbolic generic pin must link and run");
    assert_eq!(
        host.stdout_lines,
        vec!["2".to_owned(), "3".to_owned(), "4".to_owned()],
        "the executed pin must observe the declared rank-2 [3, 4] shape, not the flat \
         payload length"
    );
}

#[test]
fn package_mir_symbolic_pin_wrong_count_fails_closed() {
    let dir = crate::test_support::test_temp_dir("gsf2-generic-pin-wrong-count");
    let entry = write_pin_package(dir.as_ref(), PIN_MAIN_WRONG_COUNT);
    let config = Config::default();

    let mut host = radix_mir_runner::BufferHost::default();
    let errors = run_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        &mut host,
    )
    .expect_err("a flat list longer than the declared [T, D] element count must fail closed");
    assert!(
        errors
            .iter()
            .any(|diagnostic| diagnostic.code == Some("PKG001")),
        "the wrong-count failure must carry the structured PKG001 identity, got {:?}",
        errors
    );
}
