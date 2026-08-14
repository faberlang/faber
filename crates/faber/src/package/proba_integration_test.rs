//! End-to-end package-path tests for `.proba` discovery, filters, and import boundary.
//!
//! Drives the real `load_package_with_locale_pack` / `compile_package*` surfaces
//! (not a reimplemented mini-harness).

use super::test_support::{diagnostic_has_issue, test_temp_dir};
use super::{
    compile_package, compile_package_with_test_options, discover_package,
    library_resolver_from_config, load_package_with_locale_pack, PackageSpec, TestSourceFilter,
};
use radix::driver::Config;
use radix::locale::LocalePack;
use radix::Output;
use std::fs;
use std::path::{Path, PathBuf};

const PROBA_IMPORT_MESSAGE: &str =
    ".proba files are test sources and cannot be imported; move shared helpers to a .fab module";

/// The product-default English code pack (en = authority). The loader-level
/// tests below drive `load_package_with_locale_pack` directly, so they must
/// pass the same default pack the `compile_package*`/`analyze_package` routes
/// apply — not the pack-less (registry-direct) lexing `None` would select.
fn english_pack() -> LocalePack {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stdlib/locale/en/pack.toml");
    LocalePack::from_toml_path(path).expect("load English locale pack")
}

fn write_minimal_lib_package(dir: &Path) {
    fs::create_dir_all(dir.join("src")).expect("src");
    fs::write(
        dir.join("faber.toml"),
        r#"[package]
name = "proba_fixture"
version = "0.1.0"
edition = "2026"

[library]
provider = "proba_fixture"

[paths]
source = "src"

[build]
kind = "lib"
targets = ["rust"]
"#,
    )
    .expect("manifest");
    fs::write(
        dir.join("src/helpers.fab"),
        r#"@ public
fn double(int n) → int {
    return n * 2
}

@ public
fn identity(int n) → int {
    return n
}
"#,
    )
    .expect("helpers.fab");
}

#[test]
fn load_with_include_proba_discovers_sibling_proba_and_fab() {
    let dir = test_temp_dir("proba-discover-on");
    write_minimal_lib_package(dir.path());
    fs::write(
        dir.join("src/helpers.proba"),
        r#"import from "./helpers" helpers

fn local_helper(int n) → int {
    return helpers.double(n)
}

describe "helpers" {
    test "double via helper" {
        assert local_helper(3) ≡ 6
    }
}
"#,
    )
    .expect("helpers.proba");
    fs::write(
        dir.join("src/inline.fab"),
        r#"test "inline ok" {
    assert 1 ≡ 1
}
"#,
    )
    .expect("inline.fab");

    let spec = discover_package(dir.path()).expect("discover");
    let resolver = library_resolver_from_config(&Config::default());
    let files = load_package_with_locale_pack(&spec, &resolver, Some(&english_pack()), true, None)
        .expect("load");
    let names: Vec<_> = files
        .iter()
        .map(|f| f.path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert!(
        names.contains(&"helpers.fab".to_owned()),
        "product module missing: {names:?}"
    );
    assert!(
        names.contains(&"helpers.proba".to_owned()),
        "proba missing with include_proba: {names:?}"
    );
    assert!(
        names.contains(&"inline.fab".to_owned()),
        "inline fab missing: {names:?}"
    );
}

#[test]
fn load_without_include_proba_skips_sibling_proba() {
    let dir = test_temp_dir("proba-discover-off");
    write_minimal_lib_package(dir.path());
    fs::write(
        dir.join("src/helpers.proba"),
        r#"describe "x" { test "y" { assert 1 ≡ 1 } }
"#,
    )
    .expect("helpers.proba");

    let spec = discover_package(dir.path()).expect("discover");
    let resolver = library_resolver_from_config(&Config::default());
    // Production path (include_proba = false): the package loader applies the
    // product-default en code pack; pass it explicitly (a `None` pack would
    // select registry-direct lexing instead).
    let files = load_package_with_locale_pack(&spec, &resolver, Some(&english_pack()), false, None)
        .expect("load");
    let names: Vec<_> = files
        .iter()
        .map(|f| f.path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert!(names.contains(&"helpers.fab".to_owned()));
    assert!(
        !names.iter().any(|n| n.ends_with(".proba")),
        "production load must not include .proba: {names:?}"
    );
}

#[test]
fn single_file_proba_entry_fails_closed_outside_test_path() {
    let dir = test_temp_dir("proba-entry-build");
    let proba = dir.join("solo.proba");
    fs::write(
        &proba,
        r#"test "solo" { assert 1 ≡ 1 }
"#,
    )
    .expect("solo.proba");

    let spec = PackageSpec {
        package_root: dir.path().to_path_buf(),
        source_root: dir.path().to_path_buf(),
        entry: proba.clone(),
        templates: Default::default(),
        manifest_backed: false,
    };
    let resolver = library_resolver_from_config(&Config::default());
    let Err(err) = load_package_with_locale_pack(&spec, &resolver, None, false, None) else {
        panic!("non-test load of .proba must fail");
    };
    assert!(
        err.iter()
            .any(|d| diagnostic_has_issue(d, "proba_source_build_forbidden")),
        "expected proba_source_build_forbidden, got {:?}",
        err.iter()
            .map(|d| (&d.message, d.issue()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn product_fab_cannot_import_proba() {
    let dir = test_temp_dir("proba-import-from-fab");
    write_minimal_lib_package(dir.path());
    fs::write(
        dir.join("src/helpers.proba"),
        r#"describe "x" { test "y" { assert 1 ≡ 1 } }
"#,
    )
    .expect("helpers.proba");
    fs::write(
        dir.join("src/bad.fab"),
        r#"import from "./helpers.proba" ht
"#,
    )
    .expect("bad.fab");

    let spec = discover_package(dir.path()).expect("discover");
    let resolver = library_resolver_from_config(&Config::default());
    let Err(err) = load_package_with_locale_pack(&spec, &resolver, Some(&english_pack()), true, None)
    else {
        panic!("import of .proba must fail");
    };
    assert!(
        err.iter()
            .any(|d| diagnostic_has_issue(d, "proba_import_forbidden")),
        "expected proba_import_forbidden: {:?}",
        err.iter()
            .map(|d| (&d.message, d.issue()))
            .collect::<Vec<_>>()
    );
    assert!(
        err.iter().any(|d| d.message.contains(PROBA_IMPORT_MESSAGE)),
        "expected required diagnostic text, got {:?}",
        err.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

#[test]
fn proba_cannot_import_another_proba() {
    let dir = test_temp_dir("proba-import-proba");
    write_minimal_lib_package(dir.path());
    fs::write(
        dir.join("src/a.proba"),
        r#"fn shared() → int { return 1 }
describe "a" { test "ok" { assert 1 ≡ 1 } }
"#,
    )
    .expect("a.proba");
    fs::write(
        dir.join("src/b.proba"),
        r#"import from "./a.proba" a
describe "b" { test "use a" { assert a.shared() ≡ 1 } }
"#,
    )
    .expect("b.proba");

    let spec = discover_package(dir.path()).expect("discover");
    let resolver = library_resolver_from_config(&Config::default());
    let Err(err) =
        load_package_with_locale_pack(&spec, &resolver, Some(&english_pack()), true, None)
    else {
        panic!("proba→proba import must fail");
    };
    assert!(
        err.iter()
            .any(|d| diagnostic_has_issue(d, "proba_import_forbidden")
                && d.message.contains(PROBA_IMPORT_MESSAGE)),
        "expected proba_import_forbidden with required text: {:?}",
        err.iter()
            .map(|d| (&d.message, d.issue()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn proba_may_import_product_fab_helpers() {
    let dir = test_temp_dir("proba-import-fab");
    write_minimal_lib_package(dir.path());
    fs::write(
        dir.join("src/helpers.proba"),
        r#"import from "./helpers" helpers

describe "helpers" {
    test "double works" {
        assert helpers.double(4) ≡ 8
    }
}
"#,
    )
    .expect("helpers.proba");

    let spec = discover_package(dir.path()).expect("discover");
    let resolver = library_resolver_from_config(&Config::default());
    let files = load_package_with_locale_pack(&spec, &resolver, Some(&english_pack()), true, None)
        .expect("load");
    assert!(
        files
            .iter()
            .any(|f| f.path.file_name().and_then(|n| n.to_str()) == Some("helpers.proba")),
        "expected helpers.proba loaded"
    );
    assert!(
        files
            .iter()
            .any(|f| f.path.file_name().and_then(|n| n.to_str()) == Some("helpers.fab")),
        "expected product helpers.fab pulled via import edge"
    );
}

#[test]
fn proba_filter_include_selects_only_matching_proba() {
    let dir = test_temp_dir("proba-filter-include");
    write_minimal_lib_package(dir.path());
    fs::write(
        dir.join("src/math.proba"),
        r#"describe "math" { test "ok" { assert 1 ≡ 1 } }
"#,
    )
    .expect("math.proba");
    fs::write(
        dir.join("src/scene.proba"),
        r#"describe "scene" { test "ok" { assert 1 ≡ 1 } }
"#,
    )
    .expect("scene.proba");

    let filter = TestSourceFilter {
        include: vec!["math.proba".to_owned()],
        exclude: vec![],
    };
    let spec = discover_package(dir.path()).expect("discover");
    let resolver = library_resolver_from_config(&Config::default());
    let files =
        load_package_with_locale_pack(&spec, &resolver, None, true, Some(&filter)).expect("load");
    let proba_names: Vec<_> = files
        .iter()
        .filter_map(|f| {
            let name = f.path.file_name()?.to_str()?;
            name.ends_with(".proba").then(|| name.to_owned())
        })
        .collect();
    assert_eq!(proba_names, vec!["math.proba".to_owned()]);
}

#[test]
fn compile_build_path_ignores_adjacent_proba() {
    let dir = test_temp_dir("proba-build-ignore");
    write_minimal_lib_package(dir.path());
    fs::write(
        dir.join("src/helpers.proba"),
        r#"// this would not even parse as product — intentionally invalid if loaded
this is not valid faber
"#,
    )
    .expect("helpers.proba");

    let result = compile_package(&Config::default(), dir.path());
    assert!(
        result.success(),
        "build must ignore invalid adjacent .proba: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

#[test]
fn compile_test_path_includes_proba_with_helpers() {
    let dir = test_temp_dir("proba-compile-test");
    write_minimal_lib_package(dir.path());
    fs::write(
        dir.join("src/helpers.proba"),
        r#"import from "./helpers" helpers

fn triple(int n) → int {
    return helpers.double(n) + n
}

describe "helpers" {
    test "triple via helpers" {
        assert triple(3) ≡ 9
    }
}
"#,
    )
    .expect("helpers.proba");
    fs::write(
        dir.join("src/inline.fab"),
        r#"test "inline still works" {
    assert 2 ≡ 2
}
"#,
    )
    .expect("inline.fab");

    let result = compile_package_with_test_options(&Config::default(), dir.path(), None, None);
    assert!(
        result.success(),
        "test compile failed: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (&d.message, d.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };
    assert!(
        output.code.contains("helpers_proba") || output.code.contains("#[test]"),
        "expected proba harness emission:\n{}",
        output.code
    );
}

#[test]
fn norma_src_proba_is_discovered_beside_product_sources() {
    let norma_src = radix::paths::faberlang_home()
        .expect("faberlang container root")
        .join("norma/src");
    assert!(
        norma_src.is_dir(),
        "expected sibling norma/src at {}",
        norma_src.display()
    );
    let mathesis_proba = norma_src.join("mathesis.proba");
    assert!(
        mathesis_proba.is_file(),
        "stdlib proof fixture missing: {}",
        mathesis_proba.display()
    );
    // Must live under norma product source layout, not exempla corpus.
    let path_str = mathesis_proba.to_string_lossy();
    assert!(
        !path_str.contains("exempla"),
        "stdlib .proba must not live under exempla: {path_str}"
    );

    let files = super::source_files::package_source_files(&norma_src, true).expect("discover");
    assert!(
        files.iter().any(|p| p.ends_with("mathesis.proba")),
        "include_proba=true must find mathesis.proba under norma/src"
    );
    let prod = super::source_files::package_source_files(&norma_src, false).expect("prod");
    assert!(
        !prod.iter().any(|p| p.ends_with(".proba")),
        "production discovery must skip *.proba under norma/src"
    );
}

#[test]
fn norma_style_proba_loads_with_local_product_import() {
    // Mirrors norma/src layout: product .fab + colocated .proba importing it.
    // Norma itself has no faber.toml; prove the package test path for that shape.
    let dir = test_temp_dir("norma-style-proba");
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("src");
    fs::write(
        dir.join("faber.toml"),
        r#"[package]
name = "norma_style"
version = "0.1.0"
edition = "2026"

[library]
provider = "norma_style"

[paths]
source = "src"

[build]
kind = "lib"
targets = ["rust"]
"#,
    )
    .expect("manifest");
    // Slim pure surface matching mathesis-style catalog helpers.
    fs::write(
        src.join("mathesis.fab"),
        r#"@ public
fn PI() → float {
    return 3.141592653589793
}

@ public
fn addita(float a, float b) → float {
    return a + b
}
"#,
    )
    .expect("mathesis.fab");
    // En-surface fixture mirroring the norma mathesis catalog style (import +
    // describe/test blocks). The real `norma/src/mathesis.proba` is Latin
    // surface with its own `privata` import; pinning the probe here keeps the
    // package test independent of the sibling repo's surface.
    let proba_body = r#"import from "./mathesis" mathesis

fn sum_pi(float x) → float {
    return mathesis.addita(mathesis.PI(), x)
}

describe "mathesis catalog" {
    test "PI is positive" {
        assert mathesis.PI() > 0.0
    }

    test "addita composes with PI helper" {
        assert sum_pi(0.0) ≡ mathesis.PI()
    }
}
"#
    .to_owned();
    fs::write(src.join("mathesis.proba"), proba_body).expect("mathesis.proba");

    let result = compile_package_with_test_options(&Config::default(), dir.path(), None, None);
    assert!(
        result.success(),
        "norma-style proba compile failed: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (&d.message, d.issue()))
            .collect::<Vec<_>>()
    );
}
