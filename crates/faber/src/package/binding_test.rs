use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(feature = "hir-rust")]
use super::verify_library_binding_shapes;
use super::verify_library_bindings;

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

fn test_package(label: &str, source: &str, bindings: &str, shim: &str) -> tempfile::TempDir {
    let _ = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let _ = label;
    let root = tempfile::tempdir().expect("create temp root");
    fs::create_dir_all(root.path().join("src")).expect("create source directory");
    fs::create_dir_all(root.path().join("bindings")).expect("create binding directory");
    fs::create_dir_all(root.path().join("rust")).expect("create shim directory");
    fs::write(
        root.path().join("faber.toml"),
        r#"[package]
name = "fixture"

[library]
provider = "fixture"

[paths]
source = "src"

[build]
kind = "lib"
targets = ["rust"]

[target.rust]
bindings = "bindings/rust.toml"
"#,
    )
    .expect("write package manifest");
    fs::write(root.path().join("src/api.fab"), source).expect("write Faber source");
    fs::write(root.path().join("bindings/rust.toml"), bindings).expect("write binding manifest");
    fs::write(root.path().join("rust/shim.rs"), shim).expect("write Rust shim");
    root
}

fn has_issue(diagnostics: &[radix::module::diagnostics::Diagnostic], issue: &str) -> bool {
    let expected = radix::module::diagnostics::DiagnosticArg::new("issue", issue);
    diagnostics
        .iter()
        .any(|diagnostic| diagnostic.args.contains(&expected))
}

#[cfg(feature = "hir-rust")]
#[test]
fn compiler_analysis_handles_annotations_multiline_signatures_and_next_line_bodies() {
    let root = test_package(
        "analyzed-layout",
        r"@ public
@ future
fn delegata(
    string value
) → string

@ public
fn localis(
    string value
) → string
{
    return value
}
",
        r#"[functions."fixture:api.delegata"]
symbol = "crate::shim::delegata"

[shim]
path = "rust/shim.rs"
"#,
        "pub async fn delegata(value: String) -> String { value }\n",
    );

    let result =
        verify_library_binding_shapes(root.path(), "rust").expect("analyzed bindings verify");
    assert_eq!(result.declarations, 2);
    assert_eq!(result.bindings, 1);
}

#[test]
fn nested_method_cannot_satisfy_a_top_level_binding() {
    let root = test_package(
        "nested-method",
        r"class Capsa {
    fn abscondita(string value) → string {
        return value
    }
}
",
        r#"[functions."fixture:api.abscondita"]
symbol = "crate::shim::abscondita"

[shim]
path = "rust/shim.rs"
"#,
        "pub fn abscondita(value: String) -> String { value }\n",
    );

    let diagnostics =
        verify_library_bindings(root.path(), "rust").expect_err("nested method rejected");
    assert!(has_issue(&diagnostics, "binding_unknown_declaration"));
}

#[test]
#[ignore = "full Rust symbol compatibility probe; run with ignored integration checks"]
fn rust_probe_rejects_a_missing_symbol() {
    let root = test_package(
        "missing-symbol",
        "@ public\nfn delegata(string value) → string\n",
        r#"[functions."fixture:api.delegata"]
symbol = "crate::shim::delegata"

[shim]
path = "rust/shim.rs"
"#,
        "pub fn aliud(value: String) -> String { value }\n",
    );

    let diagnostics =
        verify_library_bindings(root.path(), "rust").expect_err("missing symbol rejected");
    assert!(has_issue(&diagnostics, "binding_rust_probe_failed"));
}

#[test]
#[ignore = "full Rust symbol compatibility probe; run with ignored integration checks"]
fn rust_probe_rejects_a_signature_mismatch() {
    let root = test_package(
        "wrong-signature",
        "@ public\nfn delegata(string value) → string\n",
        r#"[functions."fixture:api.delegata"]
symbol = "crate::shim::delegata"

[shim]
path = "rust/shim.rs"
"#,
        "pub fn delegata(value: i64) -> String { value.to_string() }\n",
    );

    let diagnostics =
        verify_library_bindings(root.path(), "rust").expect_err("wrong signature rejected");
    assert!(has_issue(&diagnostics, "binding_rust_probe_failed"));
}

#[test]
#[ignore = "full Rust symbol compatibility probe; run with ignored integration checks"]
fn rust_probe_rejects_a_sync_symbol_for_an_async_contract() {
    let root = test_package(
        "wrong-async-signature",
        "@ public\n@ future\nfn delegata(string value) → string\n",
        r#"[functions."fixture:api.delegata"]
symbol = "crate::shim::delegata"

[shim]
path = "rust/shim.rs"
"#,
        "pub fn delegata(value: String) -> String { value }\n",
    );

    let diagnostics =
        verify_library_bindings(root.path(), "rust").expect_err("sync symbol rejected");
    assert!(has_issue(&diagnostics, "binding_rust_probe_failed"));
}

#[test]
#[ignore = "full Rust symbol compatibility probe; run with ignored integration checks"]
fn rust_probe_rejects_a_missing_error_channel() {
    let root = test_package(
        "wrong-error-signature",
        "@ public\nfn delegata(string value) → string ⇥ string\n",
        r#"[functions."fixture:api.delegata"]
symbol = "crate::shim::delegata"

[shim]
path = "rust/shim.rs"
"#,
        "pub fn delegata(value: String) -> String { value }\n",
    );

    let diagnostics =
        verify_library_bindings(root.path(), "rust").expect_err("missing error channel rejected");
    assert!(has_issue(&diagnostics, "binding_rust_probe_failed"));
}

#[test]
fn duplicate_binding_rows_are_rejected_by_manifest_parsing() {
    let root = test_package(
        "duplicate-row",
        "@ public\nfn delegata(string value) → string\n",
        r#"[functions."fixture:api.delegata"]
symbol = "crate::shim::delegata"

[functions."fixture:api.delegata"]
symbol = "crate::shim::altera"
"#,
        "pub fn delegata(value: String) -> String { value }\n",
    );

    let diagnostics =
        verify_library_bindings(root.path(), "rust").expect_err("duplicate row rejected");
    assert!(has_issue(&diagnostics, "invalid_binding_manifest"));
}

#[test]
fn parent_escaping_source_path_is_rejected() {
    let root = test_package(
        "source-parent",
        "fn localis() → string { return \"ok\" }\n",
        "",
        "",
    );
    let manifest = fs::read_to_string(root.path().join("faber.toml")).expect("read manifest");
    fs::write(
        root.path().join("faber.toml"),
        manifest.replace("source = \"src\"", "source = \"../outside\""),
    )
    .expect("rewrite manifest");

    // Stage 0 restricts `paths.source` to the supported set (`src`, `.`), so
    // an escaping source value is rejected at manifest validation before the
    // member-path containment guards can run.
    let diagnostics =
        verify_library_bindings(root.path(), "rust").expect_err("escaping source rejected");
    assert!(has_issue(
        &diagnostics,
        "package_member_unsupported_source_root"
    ));
}

#[cfg(feature = "hir-rust")]
#[test]
fn dot_source_library_with_bindings_verifies() {
    // FBR-P1-001 scope update: `paths.source = "."` is a supported contract,
    // so bindings must resolve the package root itself instead of failing
    // with package_member_empty on the bare `.` member path.
    let root = test_package(
        "dot-source",
        "@ public\nfn localis() → string { return \"ok\" }\n",
        r#"[functions."fixture:api.localis"]
symbol = "crate::shim::localis"

[shim]
path = "rust/shim.rs"
"#,
        "pub fn localis(value: String) -> String { value }\n",
    );
    let manifest = fs::read_to_string(root.path().join("faber.toml")).expect("read manifest");
    fs::write(
        root.path().join("faber.toml"),
        manifest.replace("source = \"src\"", "source = \".\""),
    )
    .expect("rewrite manifest");
    // `.`-source keeps sources at the package root, not under `src/`.
    fs::write(
        root.path().join("api.fab"),
        "@ public\nfn localis() → string { return \"ok\" }\n",
    )
    .expect("write package-root source");
    fs::remove_dir_all(root.path().join("src")).expect("remove default src dir");

    let result =
        verify_library_binding_shapes(root.path(), "rust").expect("dot-source bindings verify");
    assert_eq!(result.declarations, 1);
    assert_eq!(result.bindings, 1);
}

#[cfg(feature = "hir-rust")]
#[test]
fn src_source_library_with_bindings_still_verifies() {
    // Regression lock: the default `src` layout keeps verifying after
    // source-root resolution was aligned with discovery's supported set.
    let root = test_package(
        "src-source",
        "@ public\nfn localis() → string { return \"ok\" }\n",
        r#"[functions."fixture:api.localis"]
symbol = "crate::shim::localis"

[shim]
path = "rust/shim.rs"
"#,
        "pub fn localis(value: String) -> String { value }\n",
    );

    let result =
        verify_library_binding_shapes(root.path(), "rust").expect("src-source bindings verify");
    assert_eq!(result.declarations, 1);
    assert_eq!(result.bindings, 1);
}

#[test]
fn absolute_binding_manifest_path_is_rejected() {
    let root = test_package(
        "binding-absolute",
        "fn localis() → string { return \"ok\" }\n",
        "",
        "",
    );
    let absolute = root.path().join("bindings/rust.toml");
    let manifest = fs::read_to_string(root.path().join("faber.toml")).expect("read manifest");
    fs::write(
        root.path().join("faber.toml"),
        manifest.replace(
            "bindings = \"bindings/rust.toml\"",
            &format!("bindings = {:?}", absolute.display().to_string()),
        ),
    )
    .expect("rewrite manifest");

    let diagnostics =
        verify_library_bindings(root.path(), "rust").expect_err("absolute binding rejected");
    assert!(has_issue(&diagnostics, "package_member_absolute"));
}

#[cfg(unix)]
#[test]
fn symlinked_shim_escape_is_rejected() {
    use std::os::unix::fs::symlink;

    let root = test_package(
        "shim-symlink",
        "@ public\nfn delegata(string value) → string\n",
        r#"[functions."fixture:api.delegata"]
symbol = "crate::shim::delegata"

[shim]
path = "rust/shim.rs"
"#,
        "",
    );
    let outside = root.path().with_extension("outside.rs");
    fs::write(
        &outside,
        "pub fn delegata(value: String) -> String { value }\n",
    )
    .expect("write outside shim");
    fs::remove_file(root.path().join("rust/shim.rs")).expect("remove placeholder shim");
    symlink(&outside, root.path().join("rust/shim.rs")).expect("symlink escaping shim");

    let diagnostics =
        verify_library_bindings(root.path(), "rust").expect_err("symlink escape rejected");
    assert!(has_issue(&diagnostics, "package_member_symlink_escape"));
}

#[cfg(unix)]
#[test]
fn missing_source_below_symlinked_parent_is_rejected() {
    use std::os::unix::fs::symlink;

    let root = test_package(
        "source-missing-symlink",
        "fn localis() → string { return \"ok\" }\n",
        "",
        "",
    );
    let outside = root.path().with_extension("outside-dir");
    fs::create_dir_all(&outside).expect("create outside directory");
    symlink(&outside, root.path().join("linked")).expect("symlink escaping source parent");
    let manifest = fs::read_to_string(root.path().join("faber.toml")).expect("read manifest");
    fs::write(
        root.path().join("faber.toml"),
        manifest.replace("source = \"src\"", "source = \"linked/missing\""),
    )
    .expect("rewrite manifest");

    // Any non-`src`/`.` source value is rejected at manifest validation
    // (Stage 0), so this symlinked-parent layout is caught before the
    // member-path containment guards run.
    let diagnostics =
        verify_library_bindings(root.path(), "rust").expect_err("missing symlink child rejected");
    assert!(has_issue(
        &diagnostics,
        "package_member_unsupported_source_root"
    ));
}

#[cfg(unix)]
#[test]
fn symlinked_faber_source_outside_source_root_is_rejected() {
    use std::os::unix::fs::symlink;

    let root = test_package(
        "source-file-symlink",
        "fn localis() → string { return \"ok\" }\n",
        "",
        "",
    );
    let outside = root.path().with_extension("outside.fab");
    fs::write(
        &outside,
        "fn abscondita() → string { return \"outside\" }\n",
    )
    .expect("write outside source");
    symlink(&outside, root.path().join("src/escape.fab")).expect("symlink escaping source file");

    let diagnostics =
        verify_library_bindings(root.path(), "rust").expect_err("source symlink escape rejected");
    assert!(has_issue(&diagnostics, "package_source_symlink_escape"));
}

#[test]
fn empty_source_has_no_declarations() {
    let root = test_package("empty-source", "", "", "");
    let result = verify_library_bindings(root.path(), "rust").expect("empty source must not error");
    assert_eq!(result.declarations, 0);
    assert_eq!(result.bindings, 0);
}

#[test]
fn all_local_functions_without_bindings() {
    let root = test_package(
        "local-only",
        "@ public\nfn localis() → string { return \"ok\" }\n",
        "",
        "",
    );
    let result =
        verify_library_bindings(root.path(), "rust").expect("local functions without bindings");
    assert_eq!(result.declarations, 1);
    assert_eq!(result.bindings, 0);
}

// DECISIONS D5.1 for bodyless binding declarations: they follow the one
// export-visibility rule. A marked declaration is a binding contract; an
// unmarked one is module-private, so a manifest row naming it matches nothing.

#[cfg(feature = "hir-rust")]
const LIGATA_BINDINGS: &str = r#"[functions."fixture:api.ligata"]
symbol = "crate::shim::ligata"

[shim]
path = "rust/shim.rs"
"#;
#[cfg(feature = "hir-rust")]
const LIGATA_SHIM: &str = "pub fn ligata(via: String) -> Result<String, String> { Ok(via) }\n";

#[cfg(feature = "hir-rust")]
#[test]
fn marked_bodyless_binding_declaration_is_a_binding_contract() {
    let root = test_package(
        "marked-bodyless",
        "@ public\nfn ligata(string via) → string ⇥ string\n",
        LIGATA_BINDINGS,
        LIGATA_SHIM,
    );
    let report = verify_library_binding_shapes(root.path(), "rust")
        .expect("a marked bodyless declaration binds");
    assert_eq!(report.bindings, 1);
}

#[cfg(feature = "hir-rust")]
#[test]
fn unmarked_bodyless_binding_declaration_is_module_private() {
    let root = test_package(
        "unmarked-bodyless",
        "fn ligata(string via) → string ⇥ string\n",
        LIGATA_BINDINGS,
        LIGATA_SHIM,
    );
    let diagnostics = verify_library_binding_shapes(root.path(), "rust")
        .expect_err("an unmarked bodyless declaration is not a binding contract");
    assert!(has_issue(&diagnostics, "binding_unknown_declaration"));
}

const WGSL_TARGET: &str = "wgsl-text";

/// A library whose only target is WGSL: no Rust shim, no Rust probe.
fn wgsl_package(source: &str, bindings: &str) -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("create temp root");
    fs::create_dir_all(root.path().join("src")).expect("create source directory");
    fs::create_dir_all(root.path().join("bindings")).expect("create binding directory");
    fs::write(
        root.path().join("faber.toml"),
        r#"[package]
name = "fixture"

[library]
provider = "fixture"

[paths]
source = "src"

[build]
kind = "lib"
targets = ["wgsl-text"]

[target.wgsl-text]
bindings = "bindings/wgsl.toml"
"#,
    )
    .expect("write package manifest");
    fs::write(root.path().join("src/api.fab"), source).expect("write Faber source");
    fs::write(root.path().join("bindings/wgsl.toml"), bindings).expect("write binding manifest");
    root
}

const WGSL_SOURCE: &str = "@ public
class Lumen {
    mut f32 x = 0.0
}

@ public
fn lucis(f32 value) → f32
";

#[test]
fn wgsl_manifest_with_function_and_type_binding_verifies() {
    let root = wgsl_package(
        WGSL_SOURCE,
        r#"[functions."fixture:api.lucis"]
symbol = "zq_lucis"
opener = "f32"
result = "f32"

[types."fixture:api.Lumen"]
symbol = "zq_lumen"
"#,
    );

    let result = verify_library_bindings(root.path(), WGSL_TARGET).expect("wgsl bindings verify");
    assert_eq!(result.declarations, 1);
    assert_eq!(result.bindings, 1);
    assert_eq!(result.types, 1);
    assert!(result.shim.is_none());
}

#[test]
fn wgsl_type_binding_naming_no_declared_type_is_rejected() {
    let root = wgsl_package(
        WGSL_SOURCE,
        r#"[functions."fixture:api.lucis"]
symbol = "zq_lucis"

[types."fixture:api.Absens"]
symbol = "zq_absens"
"#,
    );

    let diagnostics =
        verify_library_bindings(root.path(), WGSL_TARGET).expect_err("unknown type rejected");
    assert!(has_issue(&diagnostics, "binding_unknown_type"));
}

#[test]
fn type_binding_with_an_empty_spelling_is_rejected() {
    let root = wgsl_package(
        WGSL_SOURCE,
        r#"[functions."fixture:api.lucis"]
symbol = "zq_lucis"

[types."fixture:api.Lumen"]
symbol = "  "
"#,
    );

    let diagnostics =
        verify_library_bindings(root.path(), WGSL_TARGET).expect_err("empty spelling rejected");
    assert!(has_issue(&diagnostics, "binding_symbol_empty"));
}

#[test]
fn unknown_manifest_fields_are_still_rejected() {
    let root = wgsl_package(
        WGSL_SOURCE,
        r#"[functions."fixture:api.lucis"]
symbol = "zq_lucis"
spelling = "zq_lucis"
"#,
    );

    let diagnostics =
        verify_library_bindings(root.path(), WGSL_TARGET).expect_err("unknown field rejected");
    assert!(has_issue(&diagnostics, "invalid_binding_manifest"));
}

fn write_target_table_package(table: &str) -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("create temp root");
    fs::create_dir_all(root.path().join("src")).expect("create source directory");
    fs::write(
        root.path().join("faber.toml"),
        format!(
            "[package]\nname = \"fixture\"\n\n[library]\nprovider = \"fixture\"\n\n[build]\nkind = \"lib\"\ntargets = [\"rust\"]\n\n[target.{table}]\nbindings = \"bindings/b.toml\"\n"
        ),
    )
    .expect("write package manifest");
    root
}

#[test]
fn load_target_bindings_rejects_an_unknown_target_table() {
    let root = write_target_table_package("wgsl");
    let err = super::load_target_bindings(root.path(), "wgsl-text")
        .expect_err("a [target.wgsl] table must be an error, not empty maps");
    assert!(has_issue(&err, "package_target_table_unknown"));
    assert!(err.iter().any(|d| {
        d.args
            .iter()
            .any(|arg| arg.name == "target" && arg.value == "wgsl")
    }));
}

#[test]
fn load_target_bindings_accepts_a_target_spelling_table() {
    let root = write_target_table_package("wgsl-text");
    fs::create_dir_all(root.path().join("bindings")).expect("create binding directory");
    fs::write(root.path().join("bindings/b.toml"), "").expect("write binding manifest");
    let bindings =
        super::load_target_bindings(root.path(), "wgsl-text").expect("a known target table loads");
    assert!(bindings.functions.is_empty() && bindings.types.is_empty());
}

const WGSL_STAGE_SOURCE: &str = "@ public
@ annotation { target = fn }
class Fragment {
}

@ public
@ annotation { target = field }
class Position {
}
";

#[test]
fn wgsl_annotation_bindings_verify_and_load() {
    let root = wgsl_package(
        WGSL_STAGE_SOURCE,
        r#"[annotations."fixture:api.Fragment"]
symbol = "fragment"

[annotations."fixture:api.Position"]
symbol = "builtin(position)"
"#,
    );

    let result = verify_library_bindings(root.path(), WGSL_TARGET).expect("wgsl annotation bindings verify");
    assert_eq!(result.bindings, 0);
    let loaded = super::load_target_bindings(root.path(), WGSL_TARGET).expect("load bindings");
    assert_eq!(loaded.annotations.get("fixture:api.Fragment").map(String::as_str), Some("fragment"));
    assert_eq!(
        loaded.annotations.get("fixture:api.Position").map(String::as_str),
        Some("builtin(position)")
    );
}

#[test]
fn wgsl_annotation_binding_naming_no_declared_contract_is_rejected() {
    let root = wgsl_package(
        WGSL_STAGE_SOURCE,
        r#"[annotations."fixture:api.Absens"]
symbol = "fragment"
"#,
    );

    let diagnostics =
        verify_library_bindings(root.path(), WGSL_TARGET).expect_err("unknown annotation rejected");
    assert!(has_issue(&diagnostics, "binding_unknown_annotation"));
}
