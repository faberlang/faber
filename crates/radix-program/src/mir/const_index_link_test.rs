//! Gap G18: a single-bracket tensor index that is a bare module-qualified
//! constant (`t[c.IDX]`).
//!
//! The namespace member rewrite turns `c.IDX` into `Path(synthetic)`. It used
//! to clear the node's type, and the tensor bracket lowering then dropped the
//! whole function body (no operand, no diagnostic) because the scalar index has
//! to be wrapped into a one-element list typed by the index. The rewrite now
//! keeps the typecheck type, so `t[c.IDX]` lowers exactly like `t[[c.IDX]]`.

use super::*;
use radix_mir_runner::BufferHost;

const INDEX_PROVIDER: &str = "@ public\nconst int IDX = 2\n";

fn write_const_index_package(dir: &std::path::Path, main: &str) -> std::path::PathBuf {
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create const-index package src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "const-index-link"
version = "1.0.0"
edition = "2026"

[paths]
source = "src"
entry = "main.fab"

[build]
target = "fmir"
kind = "bin"

[reader]
locale = "en"
"#,
    )
    .expect("write const-index faber.toml");
    fs::write(src.join("c.fab"), INDEX_PROVIDER).expect("write const provider");
    fs::write(src.join("main.fab"), main).expect("write const-index main");
    src.join("main.fab")
}

fn run_const_index_main(tag: &str, main: &str) -> Vec<String> {
    let dir = crate::test_support::test_temp_dir(tag);
    let entry = write_const_index_package(dir.as_ref(), main);
    let config = Config::default().with_library_home(dir.join("libhome"));
    let mut host = BufferHost::default();
    run_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        &mut host,
    )
    .expect("a module-constant tensor index must link and run");
    host.stdout_lines
}

const SINGLE_MAIN: &str = r#"import from "./c" c

fn read(tensor<f32, [4]> t) → f32 {
    return t[c.IDX]
}

fn read_shifted(tensor<f32, [4]> t) → f32 {
    return t[c.IDX - 1]
}

main {
    var tf32[4] a ← [1.0, 2.0, 3.0, 4.0] ↦ tf32[4]
    a[c.IDX] ← 9.0
    print read(a)
    print read_shifted(a)
    print a[c.IDX]
}
"#;

const DOUBLE_MAIN: &str = r#"import from "./c" c

fn read(tensor<f32, [4]> t) → f32 {
    return t[[c.IDX]]
}

fn read_shifted(tensor<f32, [4]> t) → f32 {
    return t[[c.IDX - 1]]
}

main {
    var tf32[4] a ← [1.0, 2.0, 3.0, 4.0] ↦ tf32[4]
    a[[c.IDX]] ← 9.0
    print read(a)
    print read_shifted(a)
    print a[[c.IDX]]
}
"#;

#[test]
fn package_mir_single_bracket_module_const_index_runs() {
    assert_eq!(
        run_const_index_main("const-index-single", SINGLE_MAIN),
        vec!["9.0".to_owned(), "2.0".to_owned(), "9.0".to_owned()]
    );
}

#[test]
fn package_mir_single_and_double_bracket_module_const_index_agree() {
    assert_eq!(
        run_const_index_main("const-index-single-agree", SINGLE_MAIN),
        run_const_index_main("const-index-double-agree", DOUBLE_MAIN)
    );
}

/// The rank rule is unchanged: one scalar index on a rank-2 tensor stays the
/// named `tensor_scalar_index_rank` error, whether the scalar is a literal or
/// an imported constant.
#[test]
fn package_mir_single_bracket_module_const_index_rank_two_is_named_error() {
    let dir = crate::test_support::test_temp_dir("const-index-rank2");
    let entry = write_const_index_package(
        dir.as_ref(),
        r#"import from "./c" c

fn read(tensor<f32, [2, 2]> t) → f32 {
    return t[c.IDX]
}

main {
    const tf32[2, 2] a ← [[1.0, 2.0], [3.0, 4.0]] ↦ tf32[2, 2]
    print read(a)
}
"#,
    );
    let config = Config::default().with_library_home(dir.join("libhome"));
    let mut host = BufferHost::default();
    let error = run_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        &mut host,
    )
    .expect_err("a scalar index on a rank-2 tensor must be rejected");
    let text = format!("{error:?}");
    assert!(
        text.contains("tensor_scalar_index_rank"),
        "expected the named rank diagnostic, got: {text}"
    );
}
