//! A size-generic `@ kernel` that carries `@ radix backward` in the caller's
//! own package: a concrete caller instantiates the primal and its companion,
//! and both calls reach concrete device functions.
//!
//! The AIR backward bundle produces the companion, so this needs the device
//! lane; the public build has no companion to reach.
#![cfg(feature = "device")]

use super::fs;
use super::{Config, with_interpreted_lowered_package_mir};
use radix_mir::{MirCallee, MirFunctionId, MirStatementKind};

const MAIN: &str = r#"+++
locale = "en"
+++
@ kernel
@ radix { lane = "air" }
@ radix backward "loss_backward"
@ public
fn loss<size M, size K, size N>(tensor<f32, [M, K]> x, tensor<f32, [K, N]> w, tensor<f32, [M, N]> b) → f32 {
    let _ h ← x · w
    let _ hb ← h + b
    return hb.mean()
}

fn nil() → void {
    pass
}

main {
    let list<f32> d ← [1.0, 2.0, 3.0, 4.0, 1.0, 2.0, 3.0, 4.0, 1.0, 2.0, 3.0, 4.0, 1.0, 2.0, 3.0, 4.0]
    let tensor<f32, [4, 4]> x ← d ↦ tensor<f32, [4, 4]>
    print loss(x, x, x)
    let _ grads ← loss_backward(x, x, x, nil(), 1.0)
    print "ok"
}
"#;

#[test]
fn generic_kernel_primal_and_companion_calls_reach_concrete_functions() {
    let dir = crate::test_support::test_temp_dir("generic-kernel-backward");
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create package src");
    fs::write(
        dir.join("faber.toml"),
        "[package]\nname = \"gkb\"\nversion = \"1.0.0\"\nedition = \"2026\"\n\n[paths]\nsource = \"src\"\nentry = \"main.fab\"\n\n[build]\nkind = \"bin\"\n",
    )
    .expect("write faber.toml");
    fs::write(src.join("main.fab"), MAIN).expect("write main.fab");
    let config = Config::default().with_library_home(dir.join("libhome"));

    with_interpreted_lowered_package_mir(
        &config,
        &src.join("main.fab"),
        &crate::test_support::test_mir_seams(),
        |lowered| {
            let program = lowered.validated.program();
            let main = program
                .functions
                .iter()
                .find(|function| function.name.is_none() && function.source.is_none())
                .expect("the entry function");
            let callees: Vec<MirFunctionId> = main
                .blocks
                .iter()
                .flat_map(|block| &block.statements)
                .filter_map(|statement| match &statement.kind {
                    MirStatementKind::Call {
                        callee: MirCallee::Function(id),
                        ..
                    } => Some(*id),
                    MirStatementKind::Call {
                        callee: MirCallee::Definition { source, .. },
                        ..
                    } => {
                        panic!("call left on definition {source:?}")
                    }
                    _ => None,
                })
                .collect();
            // `nil`, the primal instance and the companion instance.
            let device_callees: Vec<MirFunctionId> = callees
                .into_iter()
                .filter(|id| lowered.device_function_metadata.contains_key(id))
                .collect();
            assert_eq!(
                device_callees.len(),
                1,
                "the primal instance is a device function: {device_callees:?}"
            );
            let primal = device_callees[0];
            assert_eq!(
                lowered.device_function_metadata[&primal].visibility,
                radix_mir::MirFunctionVisibility::Public,
                "the instance of a `@ public` kernel is a public entry"
            );
            assert!(
                program
                    .functions
                    .iter()
                    .any(|function| function.shader_stage.is_some()
                        && function.id != primal
                        && function
                            .source
                            .is_some_and(|source| source.0 >= 0x8000_0000)),
                "the companion instance is a device function"
            );
        },
    )
    .unwrap_or_else(|errors| panic!("package must lower, got {errors:?}"));
}
