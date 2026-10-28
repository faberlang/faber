//! Generic `limes<P>()` dispatch survives a package boundary.

use super::*;
use radix_mir::{MirEdgePolicy, MirIntrinsic, MirStatementKind};

const EDGE_POLICY_UTIL: &str = r#"+++
locale = "en"
+++

@ public
fn resolve_shift<P>(tf32[3] source) → list<f32> {
    return source.shift([1]).edge<P>().flatten()
}
"#;

const EDGE_POLICY_MAIN: &str = r#"+++
locale = "en"
+++

import from "./util" * as utilModule

class ZeroGhost {
    fn remap(vector<int, size 1> coord) → vector<int, size 1> {
        return [0] ↦ vector<int, size 1>
    }
    fn transform(vector<int, size 1> coord, f32 value) → f32 {
        return -value
    }
}

main {
    let tf32[3] source ← [1.0, 2.0, 3.0] ↦ tf32[3]
    let list<f32> clamped ← utilModule.resolve_shift<clamp>(source)
    let list<f32> custom ← utilModule.resolve_shift<ZeroGhost>(source)
    print clamped
    print custom
}
"#;

fn write_edge_policy_package(dir: &std::path::Path) -> std::path::PathBuf {
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create package src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "tensor-edge-policy-link"
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
    fs::write(src.join("util.fab"), EDGE_POLICY_UTIL).expect("write util.fab");
    fs::write(src.join("main.fab"), EDGE_POLICY_MAIN).expect("write main.fab");
    src.join("main.fab")
}

fn limes_policy(function: &radix_mir::MirFunction) -> Option<MirEdgePolicy> {
    function
        .blocks
        .iter()
        .flat_map(|block| &block.statements)
        .find_map(|statement| {
            let MirStatementKind::RuntimeCall { call, .. } = &statement.kind else {
                return None;
            };
            match &call.intrinsic {
                MirIntrinsic::TensorLimes(policy) => Some(policy.clone()),
                _ => None,
            }
        })
}

#[test]
fn imported_generic_limes_resolves_builtin_and_custom_policy_before_mir_validation() {
    let dir = crate::test_support::test_temp_dir("tensor-edge-policy-link");
    let entry = write_edge_policy_package(dir.as_ref());
    let config = Config::default();

    with_interpreted_lowered_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        |lowered| {
            let instances = lowered
                .validated
                .program()
                .functions
                .iter()
                .filter(|function| {
                    function
                        .name
                        .is_some_and(|name| lowered.interner.resolve(name) == "resolve_shift")
                })
                .filter_map(|function| limes_policy(function).map(|policy| (function, policy)))
                .collect::<Vec<_>>();

            assert!(
                instances
                    .iter()
                    .any(|(_, policy)| matches!(policy, MirEdgePolicy::Builtin(_))),
                "the imported generic builtin call must lower to a concrete builtin policy"
            );
            let custom = instances
                .iter()
                .find_map(|(function, policy)| match policy {
                    MirEdgePolicy::Custom { remap, transform: Some(transform), .. } => {
                        Some((function, *remap, *transform))
                    }
                    _ => None,
                })
                .expect("the imported generic custom call must lower to its checked callbacks");
            let functions = &lowered.validated.program().functions;
            assert!(functions.iter().any(|function| function.id == custom.1));
            assert!(functions.iter().any(|function| function.id == custom.2));
            assert!(
                custom.0.blocks.iter().flat_map(|block| &block.statements).any(|statement| {
                    matches!(
                        &statement.kind,
                        MirStatementKind::RuntimeCall { call, .. }
                            if matches!(&call.intrinsic, MirIntrinsic::TensorLimes(MirEdgePolicy::Custom { .. }))
                    )
                }),
                "the concrete clone must not retain the generic Clamp placeholder"
            );
        },
    )
    .unwrap_or_else(|errors| panic!("imported generic edge-policy package must link: {errors:?}"));
}

#[test]
fn imported_generic_limes_dispatch_runs_for_builtin_and_custom_policy() {
    let dir = crate::test_support::test_temp_dir("tensor-edge-policy-link-run");
    let entry = write_edge_policy_package(dir.as_ref());
    let mut host = radix_mir_runner::BufferHost::default();

    run_package_mir(
        &Config::default(),
        &entry,
        &crate::test_support::test_mir_seams(),
        &mut host,
    )
    .expect("imported generic builtin and custom limes must run");
    assert_eq!(
        host.stdout_lines,
        vec!["[2.0, 3.0, 3.0]".to_owned(), "[2.0, 3.0, -1.0]".to_owned()]
    );
}
