//! Tests for the compiler-view manifest projection (radix-program, `RP2a`).
//!
//! The product-only device-backend projection tests stay in faber (product
//! view); this file covers the moved compiler surface: projection parsing
//! (including tolerance of product-only sections) and build-target mapping.

use super::*;
use radix_module::diagnostics::DiagnosticPhase;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("radix-program-manifest-{label}-{nonce}"));
    fs::create_dir_all(&path).expect("create temp dir");
    path
}

#[test]
fn projection_parses_compiler_fields() {
    let manifest: FaberManifest = toml::from_str(
        r#"
[package]
name = "demo"
version = "0.2.0"
edition = "2024"

[paths]
source = "src"
entry = "main.fab"

[paths.templates]
gnu = "../common/gnu"

[library]
provider = "demo"

[build]
target = "rust"
kind = "bin"

[locale]
locale = "en"
pack = "packs/en"

[target.rust]
host = "native"
bindings = "bindings.toml"

[target.rust.dependencies]
serde = "1.0"

[dependencies]
norma = "0.1.0"
"#,
    )
    .expect("compiler projection parses");
    assert_eq!(manifest.package.name, "demo");
    assert_eq!(manifest.package.version, "0.2.0");
    assert_eq!(manifest.package.edition, "2024");
    assert_eq!(manifest.paths.source, "src");
    assert_eq!(manifest.paths.entry.as_deref(), Some("main.fab"));
    assert_eq!(
        manifest.paths.templates.get("gnu").map(String::as_str),
        Some("../common/gnu")
    );
    let library = manifest.library.expect("library table");
    assert_eq!(library.provider, "demo");
    assert_eq!(manifest.build.target.as_deref(), Some("rust"));
    assert_eq!(manifest.build.kind, "bin");
    assert_eq!(manifest.locale.locale.as_deref(), Some("en"));
    assert_eq!(manifest.locale.pack.as_deref(), Some("packs/en"));
    let rust_target = manifest.target.get("rust").expect("target.rust");
    assert_eq!(rust_target.host, Some(ManifestRustHost::Native));
    assert_eq!(rust_target.bindings.as_deref(), Some("bindings.toml"));
    assert_eq!(
        rust_target.dependencies.get("serde").map(String::as_str),
        Some("1.0")
    );
    assert_eq!(
        manifest.dependencies.get("norma").map(String::as_str),
        Some("0.1.0")
    );
}

#[test]
fn projection_tolerates_product_only_sections() {
    let manifest: FaberManifest = toml::from_str(
        r#"
[package]
name = "demo"

[device]
device = "auto"
steps = 250

[product]
kind = "browser-app"
emit = "typescript"
out = "dist"
templates = "pages"
styles = "styles"
public = "public"
assets_manifest = "assets.json"
controllers_json = "controllers.json"

[format]
policy = "pretty-v1"

[dispatch]
providers = ["norma"]
"#,
    )
    .expect("product-only sections are tolerated by the compiler projection");
    assert_eq!(manifest.package.name, "demo");
    assert!(manifest.dependencies.is_empty());
}

#[test]
fn projection_applies_defaults() {
    let manifest: FaberManifest =
        toml::from_str("[package]\nname = \"demo\"\n").expect("defaults apply");
    assert_eq!(manifest.package.version, "0.1.0");
    assert_eq!(manifest.package.edition, "2026");
    assert_eq!(manifest.paths.source, "src");
    assert!(manifest.paths.entry.is_none());
    assert!(manifest.paths.templates.is_empty());
    assert!(manifest.library.is_none());
    assert_eq!(manifest.build.kind, "bin");
    assert_eq!(manifest.build.target, None);
    assert_eq!(manifest.build.targets, [] as [String; 0]);
    assert_eq!(manifest.locale.locale, None);
    assert!(manifest.target.is_empty());
    assert!(manifest.dependencies.is_empty());
}

#[test]
fn library_home_paths_key_parses_and_defaults_to_none() {
    let manifest: FaberManifest = toml::from_str(
        "[package]\nname = \"demo\"\n\n[paths]\nsource = \"src\"\nlibrary_home = \"../libs\"\n",
    )
    .expect("library_home key parses");
    assert_eq!(manifest.paths.library_home.as_deref(), Some("../libs"));

    let manifest: FaberManifest =
        toml::from_str("[package]\nname = \"demo\"\n\n[paths]\nsource = \"src\"\n")
            .expect("paths without library_home parses");
    assert!(manifest.paths.library_home.is_none());
}

#[test]
fn projection_rejects_unknown_compiler_fields() {
    let err = toml::from_str::<FaberManifest>(
        r#"
[package]
name = "demo"
namme = "typo"
"#,
    )
    .expect_err("unknown compiler fields fail closed");
    assert!(err.to_string().contains("namme"), "diagnostic: {err:?}");
}

#[test]
fn read_manifest_parses_projection_from_disk() {
    let root = temp_dir("parse");
    let manifest_path = root.join("faber.toml");
    fs::write(
        &manifest_path,
        "[package]\nname = \"demo\"\n\n[device]\ndevice = \"auto\"\n",
    )
    .expect("write manifest");
    let manifest = read_manifest(&manifest_path).expect("read compiler projection");
    assert_eq!(manifest.package.name, "demo");
}

#[test]
fn read_manifest_rejects_missing_file() {
    let path = Path::new("/nonexistent/faber.toml");
    let err = read_manifest(path).expect_err("missing file fails");
    assert_eq!(err.phase, DiagnosticPhase::Io);
    assert_eq!(err.file, path.display().to_string());
}

#[test]
fn build_target_maps_supported_spellings() {
    assert_eq!(
        manifest_build_target(None, Path::new("faber.toml")).expect("portable default"),
        Target::HirFhir
    );
    assert_eq!(
        manifest_build_target(Some("rust"), Path::new("faber.toml")).expect("rust"),
        Target::HirRust
    );
    assert_eq!(
        manifest_build_target(Some("fhir"), Path::new("faber.toml")).expect("fhir"),
        Target::HirFhir
    );
    assert_eq!(
        manifest_build_target(Some("ts"), Path::new("faber.toml")).expect("ts"),
        Target::HirTypeScript
    );
    assert_eq!(
        manifest_build_target(Some("typescript"), Path::new("faber.toml")).expect("typescript"),
        Target::HirTypeScript
    );
    assert_eq!(
        manifest_build_target(Some("scena"), Path::new("faber.toml")).expect("scena"),
        Target::MirScena
    );
    assert_eq!(
        manifest_build_target(Some("fmir-text"), Path::new("faber.toml")).expect("fmir-text"),
        Target::MirFmir
    );
    assert_eq!(
        manifest_build_target(Some("fmir"), Path::new("faber.toml")).expect("fmir"),
        Target::MirFmirBinary
    );
    assert_eq!(
        manifest_build_target(Some("llvm-host"), Path::new("faber.toml")).expect("llvm-host"),
        Target::MirLlvmHost
    );
}

#[test]
fn build_target_trims_spelling() {
    assert_eq!(
        manifest_build_target(Some("  rust  "), Path::new("faber.toml")).expect("trimmed"),
        Target::HirRust
    );
}

#[test]
fn build_target_rejects_unsupported_spelling() {
    let err = manifest_build_target(Some("ada"), Path::new("faber.toml"))
        .expect_err("unsupported value must fail closed");
    assert_eq!(err.code, Some("PKG001"));
    assert_eq!(err.issue(), Some("package_build_target_unsupported"));
    assert_eq!(
        err.args
            .iter()
            .find(|arg| arg.name == "target")
            .map(|arg| arg.value.as_str()),
        Some("ada")
    );
    // The diagnostic names the accepted vocabulary (S09-7): a rejected
    // spelling must tell the declarer what the manifest does accept.
    let accepted = accepted_arg(&err);
    assert!(accepted.split(", ").any(|spelling| spelling == "go"));
    assert!(accepted.split(", ").any(|spelling| spelling == "fmir"));
}

fn accepted_arg(err: &radix_module::diagnostics::Diagnostic) -> &str {
    err.args
        .iter()
        .find(|arg| arg.name == "accepted")
        .map(|arg| arg.value.as_str())
        .expect("build-target rejection carries the accepted vocabulary")
}

#[test]
fn build_target_accepts_the_cli_vocabulary() {
    // S09-7: every canonical spelling the product CLI accepts for `-t` is
    // declarable — the manifest cannot know less than the build command.
    for (spelling, expected) in [
        ("go", Target::HirGo),
        ("swift", Target::HirSwift),
        ("python", Target::HirPython),
        ("haskell", Target::HirHaskell),
        ("faber", Target::HirFaber),
        ("wasm-text", Target::MirWasm),
        ("llvm-text", Target::MirLlvm),
        ("metal-text", Target::MirMetal),
        ("wgsl-text", Target::MirWgsl),
        ("sexp", Target::MirSexp),
        ("basic", Target::MirBasic),
        ("pascal", Target::HirPascal),
        ("cobol", Target::HirCobol),
        ("fortran", Target::HirFortran),
        ("odin", Target::HirOdin),
        ("c", Target::HirC),
    ] {
        assert_eq!(
            manifest_build_target(Some(spelling), Path::new("faber.toml"))
                .unwrap_or_else(|err| panic!("spelling '{spelling}' rejected: {err:?}")),
            expected
        );
    }
}

#[test]
fn build_target_rejects_internal_targets_with_the_rule_named() {
    // S09-7: the only exclusions are compiler-internal targets with no
    // package build route, and the diagnostic says why.
    for internal in ["stepper", "amd"] {
        let err = manifest_build_target(Some(internal), Path::new("faber.toml"))
            .expect_err("internal spelling must be rejected");
        assert_eq!(err.code, Some("PKG001"));
        assert_eq!(err.issue(), Some("package_build_target_internal"));
        assert_eq!(
            err.args
                .iter()
                .find(|arg| arg.name == "target")
                .map(|arg| arg.value.as_str()),
            Some(internal)
        );
        // The exclusion rule is the issue identity above; the declarer is
        // still told what the manifest accepts.
        assert!(
            accepted_arg(&err)
                .split(", ")
                .any(|spelling| spelling == "go")
        );
    }
}

fn manifest_with_target_tables(tables: &str) -> FaberManifest {
    toml::from_str(&format!("[package]\nname = \"demo\"\n{tables}")).expect("manifest parses")
}

#[test]
fn target_tables_reject_unknown_key_naming_it_and_the_accepted_spellings() {
    let manifest = manifest_with_target_tables("\n[target.wgsl]\nbindings = \"b.toml\"\n");
    let err = validate_target_tables(&manifest.target, Path::new("faber.toml"))
        .expect_err("an unknown [target.<key>] must fail validation");
    assert_eq!(err.code, Some("PKG001"));
    assert_eq!(err.issue(), Some("package_target_table_unknown"));
    assert!(
        err.args
            .iter()
            .any(|arg| arg.name == "target" && arg.value == "wgsl")
    );
    assert_eq!(
        err.args
            .iter()
            .find(|arg| arg.name == "target")
            .map(|arg| arg.value.as_str()),
        Some("wgsl")
    );
    let accepted: Vec<&str> = accepted_arg(&err).split(", ").collect();
    assert!(accepted.contains(&"wgsl-text"));
    assert!(accepted.contains(&"rust"));
    assert!(!accepted.contains(&"wgsl"));
}

#[test]
fn target_tables_accept_target_spellings() {
    let manifest = manifest_with_target_tables(
        "\n[target.wgsl-text]\nbindings = \"w.toml\"\n\n[target.rust]\n\n[target.ts]\n",
    );
    validate_target_tables(&manifest.target, Path::new("faber.toml"))
        .expect("target spellings validate");
    let empty = manifest_with_target_tables("");
    validate_target_tables(&empty.target, Path::new("faber.toml")).expect("no tables validate");
}
