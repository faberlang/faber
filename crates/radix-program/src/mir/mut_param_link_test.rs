//! A `mut` parameter of an imported generic function validates at the call
//! site: the semantic signature carried in the validation context spells the
//! access intent as a `Ref` wrapper, MIR carries it in `MirParamMode`, so the
//! argument is the payload place. A `trap` around a checked tensor read whose
//! literal index the checker proved in range has no error member; the tagged
//! result must therefore select `returned` with the body's `void` payload.

use super::*;

const MUT_BASE: &str = r"@ public
fn zero() → int {
    return 0
}
";

const MUT_UTIL: &str = r#"import from "./base" * as baseModule

@ public
fn copy_into<size T, size D>(tensor<f32, [T, D]> input, mut tensor<f32, [T, D]> output) → void {
    output ⇇ input
}
"#;

const MUT_MAIN: &str = r#"import from "./util" * as utilModule

main {
    let tensor<f32, [2, 2]> input ← [[1.0, 2.0], [3.0, 4.0]] ↦ tensor<f32, [2, 2]>
    mut tensor<f32, [2, 2]> output ← [[0.0, 0.0], [0.0, 0.0]] ↦ tensor<f32, [2, 2]>
    utilModule.copy_into<size 2, size 2>(input, mut output)
    print output.get([1, 0]) coalesce (0.0 ∷ f32)
}
"#;

const TRAP_MAIN: &str = r"main {
    let _ trapped ← trap {
        let tensor<f32, [2, 2]> y ← [[1.0, 2.0], [3.0, 4.0]] ↦ tensor<f32, [2, 2]>
        print y.get([0, 1]) coalesce (0.0 ∷ f32)
    }
    match trapped {
        case returned let value { print value is void }
        case thrown let error { print false }
    }
}
";

fn write_package(
    dir: &std::path::Path,
    util: Option<&str>,
    main_source: &str,
) -> std::path::PathBuf {
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create package src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "mut-param-link"
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
    if let Some(util) = util {
        fs::write(src.join("util.fab"), util).expect("write util.fab");
        fs::write(src.join("base.fab"), MUT_BASE).expect("write base.fab");
    }
    fs::write(src.join("main.fab"), main_source).expect("write main.fab");
    src.join("main.fab")
}

fn run_stdout(name: &str, util: Option<&str>, main_source: &str) -> Vec<String> {
    let dir = crate::test_support::test_temp_dir(name);
    let entry = write_package(dir.as_ref(), util, main_source);
    let config = Config::default();
    let mut host = radix_mir_runner::BufferHost::default();
    run_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        &mut host,
    )
    .unwrap_or_else(|errors| panic!("package must link and run, got {errors:?}"));
    host.stdout_lines
}

#[test]
fn imported_generic_mut_parameter_links_and_writes_through() {
    let lines = run_stdout("mut-param-link", Some(MUT_UTIL), MUT_MAIN);
    assert_eq!(lines, vec!["3.0".to_owned()]);
}

#[test]
fn trap_around_in_range_literal_tensor_read_has_no_error_member() {
    let lines = run_stdout("mut-param-trap-read", None, TRAP_MAIN);
    assert_eq!(lines, vec!["2.0".to_owned(), "true".to_owned()]);
}
