//! D5.1 sibling link: an exported function reaching a module-private helper
//! of its own module must link. Before D5.1 every top-level function was
//! exported, so the sibling link paths only allocated synthetic sources for
//! exports; the private helper then kept its unit-local def id and the merged
//! program resolved it wrongly (PKG001). The fixture lines the helper's
//! unit-local def id up with the entry's zero-argument `run` so the stale id
//! collides visibly.

use super::*;

const HELPER: &str = r#"import from "./unit" unit

fn double(int n) → int {
    return n * 2
}

@ public
fn quad(int n) → int {
    return double(double(n)) + unit.one()
}
"#;

const UNIT: &str = r"@ public
fn one() → int {
    return 1
}
";

const MAIN: &str = r#"import from "./helper" helper

fn run() → int {
    return helper.quad(3)
}

main {
    print run()
}
"#;

fn write_package(dir: &Path) -> PathBuf {
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create package src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "private-helper-link"
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
    fs::write(src.join("helper.fab"), HELPER).expect("write helper.fab");
    fs::write(src.join("unit.fab"), UNIT).expect("write unit.fab");
    fs::write(src.join("main.fab"), MAIN).expect("write main.fab");
    src.join("main.fab")
}

#[test]
fn sibling_exported_function_links_module_private_helper() {
    let dir = crate::test_support::test_temp_dir("private-helper-link");
    let entry = write_package(dir.as_ref());
    with_interpreted_lowered_package_mir(
        &Config::default(),
        &entry,
        &crate::test_support::test_mir_seams(),
        |_| {},
    )
    .unwrap_or_else(|errors| panic!("private sibling helper must link, got {errors:?}"));
}

const CONSTS: &str = r"const int N ← 3

@ public
const int M ← N * 2

@ public
fn m() → int {
    return N * 2
}
";

const CONSTS_MAIN: &str = r#"import from "./consts" consts

main {
    print consts.M
    print consts.m()
}
"#;

/// D5.1 const converse: an exported const initializer and an exported
/// function body both reference a module-private const. The private const
/// must still be linked (transplanted) even though it is not a member target.
#[test]
fn sibling_exported_const_and_function_link_module_private_const() {
    let dir = crate::test_support::test_temp_dir("private-const-link");
    let src = dir.as_ref().join("src");
    let entry = write_package(dir.as_ref());
    fs::write(src.join("consts.fab"), CONSTS).expect("write consts.fab");
    fs::write(&entry, CONSTS_MAIN).expect("write main.fab");
    with_interpreted_lowered_package_mir(
        &Config::default(),
        &entry,
        &crate::test_support::test_mir_seams(),
        |_| {},
    )
    .unwrap_or_else(|errors| panic!("private sibling const must link, got {errors:?}"));
}
