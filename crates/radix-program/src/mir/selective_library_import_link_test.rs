//! A selective import of a library function links like a namespace member
//! call (core-norma N2 gap R2).
//!
//! `import from "lib:mod" const f` binds `f` as a caller-owned value local
//! with no namespace receiver, so the bare call `f(x)` carries no
//! `(namespace, member)` key. Left unlinked it stayed a package provider and
//! the runner refused it (`unsupported: provider package:lib:mod::f`), while
//! the namespace form `mod.f(x)` ran. The bare call must resolve to the same
//! exported function body the namespace member call reaches.

use super::*;
use radix_mir_runner::BufferHost;

const SELECTIVE_LIB: &str = r#"+++
locale = "en"
+++
@ public
fn triple(int x) → int {
    return x + x + x
}

@ public
fn bump(int x) → int {
    return x + 1
}
"#;

const SELECTIVE_MAIN: &str = r#"+++
locale = "en"
+++
import from "optlib:mathlib" const triple, bump

main {
    print triple(14)
    print bump(triple(2))
}
"#;

const SELECTIVE_ALIAS_MAIN: &str = r#"+++
locale = "en"
+++
import from "optlib:mathlib" const triple as thrice

main {
    print thrice(5)
}
"#;

const NAMESPACE_MAIN: &str = r#"+++
locale = "en"
+++
import from "optlib:mathlib" mathlib

main {
    print mathlib.triple(14)
}
"#;

fn write_selective_package(dir: &std::path::Path, main: &str) -> std::path::PathBuf {
    let src = dir.join("src");
    let libhome = dir.join("libhome");
    let library = libhome.join("optlib");
    fs::create_dir_all(&src).expect("create package src");
    fs::create_dir_all(library.join("src")).expect("create library src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "selective-library-link"
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
    fs::write(library.join("src/mathlib.fab"), SELECTIVE_LIB).expect("write library module");
    fs::write(src.join("main.fab"), main).expect("write main.fab");
    src.join("main.fab")
}

fn run_selective_main(tag: &str, main: &str) -> Result<Vec<String>, Vec<Diagnostic>> {
    let dir = crate::test_support::test_temp_dir(tag);
    let entry = write_selective_package(dir.as_ref(), main);
    let config = Config::default().with_library_home(dir.join("libhome"));
    let mut host = BufferHost::default();
    run_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        &mut host,
    )?;
    Ok(host.stdout_lines)
}

#[test]
fn package_mir_namespace_library_call_runs() {
    // Control: the namespace form already linked before the repair.
    assert_eq!(
        run_selective_main("selective-link-namespace", NAMESPACE_MAIN)
            .unwrap_or_else(|errors| panic!("namespace call must link, got {errors:?}")),
        vec!["42".to_owned()]
    );
}

#[test]
fn package_mir_links_selective_library_function_calls() {
    assert_eq!(
        run_selective_main("selective-link-bare", SELECTIVE_MAIN)
            .unwrap_or_else(|errors| panic!("selective import must link, got {errors:?}")),
        vec!["42".to_owned(), "7".to_owned()]
    );
}

#[test]
fn package_mir_links_aliased_selective_library_function_call() {
    assert_eq!(
        run_selective_main("selective-link-alias", SELECTIVE_ALIAS_MAIN)
            .unwrap_or_else(|errors| panic!("aliased selective import must link, got {errors:?}")),
        vec!["15".to_owned()]
    );
}

const SIBLING_LOCAL: &str = r#"+++
locale = "en"
+++
fn double(int x) → int {
    return x + x
}

@ public
fn quad(int x) → int {
    return double(double(x))
}
"#;

const SIBLING_SELECTIVE_MAIN: &str = r#"+++
locale = "en"
+++
import from "./local" const quad

main {
    print quad(3)
}
"#;

const SIBLING_ALIAS_MAIN: &str = r#"+++
locale = "en"
+++
import from "./local" const quad as fourfold

main {
    print fourfold(5)
}
"#;

fn run_sibling_main(tag: &str, main: &str) -> Result<Vec<String>, Vec<Diagnostic>> {
    let dir = crate::test_support::test_temp_dir(tag);
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create package src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "selective-sibling-link"
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
    fs::write(src.join("local.fab"), SIBLING_LOCAL).expect("write local.fab");
    fs::write(src.join("main.fab"), main).expect("write main.fab");
    let mut host = BufferHost::default();
    run_package_mir(
        &Config::default(),
        &src.join("main.fab"),
        &crate::test_support::test_mir_seams(),
        &mut host,
    )?;
    Ok(host.stdout_lines)
}

#[test]
fn package_mir_links_selective_sibling_function_calls() {
    assert_eq!(
        run_sibling_main("selective-link-sibling", SIBLING_SELECTIVE_MAIN)
            .unwrap_or_else(|errors| panic!("sibling selective import must link, got {errors:?}")),
        vec!["12".to_owned()]
    );
}

#[test]
fn package_mir_links_aliased_selective_sibling_function_call() {
    assert_eq!(
        run_sibling_main("selective-link-sibling-alias", SIBLING_ALIAS_MAIN).unwrap_or_else(
            |errors| panic!("aliased sibling selective import must link, got {errors:?}")
        ),
        vec!["20".to_owned()]
    );
}
