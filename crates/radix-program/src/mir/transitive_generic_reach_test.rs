//! Reachability through a transitive generic import: a generic library
//! function reachable only through another library's generic function (and a
//! `@ kernel` reachable the same way) must stay in the merged program. A
//! generic template has no row in the lowered library program, so its
//! sibling-library calls were invisible to the reachability closure and the
//! callee was pruned (runner `missing MIR function` at run time).

use super::*;
use radix_mir_runner::BufferHost;

const KERNELS: &str = r#"+++
locale = "en"
+++
@ public
fn width<size D>(tensor<f32, [D]> wx) → int {
    return 7
}

@ kernel
@ public
fn gather<size V, size D>(tensor<f32, [D]> gdom, tensor<f32, [V, D]> gtable, tensor<i32, [1]> gids, mut tensor<f32, [D]> gout) → void {
    for from gdom at [j] thread lane let e {
        gout[[j]] ← gtable[[gids[0] ↦ int, j]]
    }
}
"#;

const HOST: &str = r#"+++
locale = "en"
+++
import from "reachgen:kernels" ks

@ public
fn probe<size D>(tensor<f32, [D]> hx) → int {
    return ks.width(hx)
}

@ public
fn embed<size V, size D>(tensor<f32, [V, D]> table, tensor<i32, [1]> ids, mut tensor<f32, [D]> hidden) → void {
    mut tensor<f32, [D]> dom ← tf32[D] {}
    ks.gather(dom, table, ids, mut hidden)
}
"#;

const MAIN: &str = r#"import from "reachgen:host" host

main {
    mut tensor<f32, [2]> x ← tf32[2] {}
    print host.probe(x)
    mut tensor<f32, [3, 2]> table ← tf32[3, 2] {}
    table[[1, 0]] ← 1.5
    table[[1, 1]] ← 2.5
    mut tensor<i32, [1]> ids ← tensor<i32, [1]> {}
    ids[0] ← 1
    mut tensor<f32, [2]> hidden ← tf32[2] {}
    host.embed(table, ids, mut hidden)
    print hidden[[0]]
    print hidden[[1]]
}
"#;

fn write_package(dir: &std::path::Path) -> std::path::PathBuf {
    let src = dir.join("src");
    let libhome = dir.join("libhome");
    let library = libhome.join("reachgen");
    fs::create_dir_all(&src).expect("create package src");
    fs::create_dir_all(library.join("src")).expect("create library src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "transitive-generic-reach"
version = "1.0.0"
edition = "2026"

[paths]
source = "src"
entry = "main.fab"

[build]
kind = "bin"

[dependencies]
reachgen = "1.0.0"
"#,
    )
    .expect("write faber.toml");
    let library_display = library.display();
    let lock = format!(
        "[[package]]\nname = \"reachgen\"\nversion = \"1.0.0\"\nsource = \"path:{library_display}\"\npackage_root = \"{library_display}\"\nkind = \"source\"\ntarget_language = \"\"\ntarget_triple = \"\"\ntarget_manifest = \"\"\ninterface_root = \"{library_display}/src\"\nartifact = \"\"\ncrate = \"\"\nrustc = \"\"\n"
    );
    fs::write(dir.join("faber.lock"), lock).expect("write faber.lock");
    fs::write(library.join("src/kernels.fab"), KERNELS).expect("write kernels module");
    fs::write(library.join("src/host.fab"), HOST).expect("write host module");
    fs::write(src.join("main.fab"), MAIN).expect("write main.fab");
    src.join("main.fab")
}

#[test]
fn transitive_generic_function_and_kernel_stay_reachable() {
    let dir = crate::test_support::test_temp_dir("transitive-generic-reach");
    let entry = write_package(dir.as_ref());
    let config = Config::default().with_library_home(dir.join("libhome"));

    let mut host = BufferHost::default();
    run_package_mir_with_selection(
        &config,
        &entry,
        DeviceSelection::Cpu,
        &crate::test_support::test_mir_seams(),
        &mut host,
    )
    .unwrap_or_else(|errors| panic!("transitive generic imports must run, got {errors:?}"));
    assert_eq!(
        host.stdout_lines,
        vec!["7".to_owned(), "1.5".to_owned(), "2.5".to_owned()]
    );
}
