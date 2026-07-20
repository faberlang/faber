use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use super::verify_library_bindings;

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

fn test_package(label: &str, source: &str, bindings: &str, shim: &str) -> PathBuf {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let root =
        std::env::temp_dir().join(format!("faber-binding-{label}-{}-{id}", std::process::id()));
    fs::create_dir_all(root.join("src")).expect("create source directory");
    fs::create_dir_all(root.join("bindings")).expect("create binding directory");
    fs::create_dir_all(root.join("rust")).expect("create shim directory");
    fs::write(
        root.join("faber.toml"),
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
    fs::write(root.join("src/api.fab"), source).expect("write Faber source");
    fs::write(root.join("bindings/rust.toml"), bindings).expect("write binding manifest");
    fs::write(root.join("rust/shim.rs"), shim).expect("write Rust shim");
    root
}

fn has_issue(diagnostics: &[radix::diagnostics::Diagnostic], issue: &str) -> bool {
    let expected = radix::diagnostics::DiagnosticArg::new("issue", issue);
    diagnostics
        .iter()
        .any(|diagnostic| diagnostic.args.contains(&expected))
}

#[test]
fn compiler_analysis_handles_annotations_multiline_signatures_and_next_line_bodies() {
    let root = test_package(
        "analyzed-layout",
        r"@ futura
functio delegata(
    textus value
) → textus

functio localis(
    textus value
) → textus
{
    redde value
}
",
        r#"[functions."fixture:api.delegata"]
symbol = "crate::shim::delegata"

[shim]
path = "rust/shim.rs"
"#,
        "pub async fn delegata(value: String) -> String { value }\n",
    );

    let result = verify_library_bindings(&root, "rust").expect("analyzed bindings verify");
    assert_eq!(result.declarations, 2);
    assert_eq!(result.bindings, 1);
}

#[test]
fn nested_method_cannot_satisfy_a_top_level_binding() {
    let root = test_package(
        "nested-method",
        r"genus Capsa {
    functio abscondita(textus value) → textus {
        redde value
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

    let diagnostics = verify_library_bindings(&root, "rust").expect_err("nested method rejected");
    assert!(has_issue(&diagnostics, "binding_unknown_declaration"));
}

#[test]
fn rust_probe_rejects_a_missing_symbol() {
    let root = test_package(
        "missing-symbol",
        "functio delegata(textus value) → textus\n",
        r#"[functions."fixture:api.delegata"]
symbol = "crate::shim::delegata"

[shim]
path = "rust/shim.rs"
"#,
        "pub fn aliud(value: String) -> String { value }\n",
    );

    let diagnostics = verify_library_bindings(&root, "rust").expect_err("missing symbol rejected");
    assert!(has_issue(&diagnostics, "binding_rust_probe_failed"));
}

#[test]
fn rust_probe_rejects_a_signature_mismatch() {
    let root = test_package(
        "wrong-signature",
        "functio delegata(textus value) → textus\n",
        r#"[functions."fixture:api.delegata"]
symbol = "crate::shim::delegata"

[shim]
path = "rust/shim.rs"
"#,
        "pub fn delegata(value: i64) -> String { value.to_string() }\n",
    );

    let diagnostics = verify_library_bindings(&root, "rust").expect_err("wrong signature rejected");
    assert!(has_issue(&diagnostics, "binding_rust_probe_failed"));
}

#[test]
fn rust_probe_rejects_a_sync_symbol_for_an_async_contract() {
    let root = test_package(
        "wrong-async-signature",
        "@ futura\nfunctio delegata(textus value) → textus\n",
        r#"[functions."fixture:api.delegata"]
symbol = "crate::shim::delegata"

[shim]
path = "rust/shim.rs"
"#,
        "pub fn delegata(value: String) -> String { value }\n",
    );

    let diagnostics = verify_library_bindings(&root, "rust").expect_err("sync symbol rejected");
    assert!(has_issue(&diagnostics, "binding_rust_probe_failed"));
}

#[test]
fn rust_probe_rejects_a_missing_error_channel() {
    let root = test_package(
        "wrong-error-signature",
        "functio delegata(textus value) → textus ⇥ textus\n",
        r#"[functions."fixture:api.delegata"]
symbol = "crate::shim::delegata"

[shim]
path = "rust/shim.rs"
"#,
        "pub fn delegata(value: String) -> String { value }\n",
    );

    let diagnostics =
        verify_library_bindings(&root, "rust").expect_err("missing error channel rejected");
    assert!(has_issue(&diagnostics, "binding_rust_probe_failed"));
}

#[test]
fn duplicate_binding_rows_are_rejected_by_manifest_parsing() {
    let root = test_package(
        "duplicate-row",
        "functio delegata(textus value) → textus\n",
        r#"[functions."fixture:api.delegata"]
symbol = "crate::shim::delegata"

[functions."fixture:api.delegata"]
symbol = "crate::shim::altera"
"#,
        "pub fn delegata(value: String) -> String { value }\n",
    );

    let diagnostics = verify_library_bindings(&root, "rust").expect_err("duplicate row rejected");
    assert!(has_issue(&diagnostics, "invalid_binding_manifest"));
}

#[test]
fn parent_escaping_source_path_is_rejected() {
    let root = test_package(
        "source-parent",
        "functio localis() → textus { redde \"ok\" }\n",
        "",
        "",
    );
    let manifest = fs::read_to_string(root.join("faber.toml")).expect("read manifest");
    fs::write(
        root.join("faber.toml"),
        manifest.replace("source = \"src\"", "source = \"../outside\""),
    )
    .expect("rewrite manifest");

    let diagnostics = verify_library_bindings(&root, "rust").expect_err("escaping source rejected");
    assert!(has_issue(&diagnostics, "package_member_parent_escape"));
}

#[test]
fn absolute_binding_manifest_path_is_rejected() {
    let root = test_package(
        "binding-absolute",
        "functio localis() → textus { redde \"ok\" }\n",
        "",
        "",
    );
    let absolute = root.join("bindings/rust.toml");
    let manifest = fs::read_to_string(root.join("faber.toml")).expect("read manifest");
    fs::write(
        root.join("faber.toml"),
        manifest.replace(
            "bindings = \"bindings/rust.toml\"",
            &format!("bindings = {:?}", absolute.display().to_string()),
        ),
    )
    .expect("rewrite manifest");

    let diagnostics =
        verify_library_bindings(&root, "rust").expect_err("absolute binding rejected");
    assert!(has_issue(&diagnostics, "package_member_absolute"));
}

#[cfg(unix)]
#[test]
fn symlinked_shim_escape_is_rejected() {
    use std::os::unix::fs::symlink;

    let root = test_package(
        "shim-symlink",
        "functio delegata(textus value) → textus\n",
        r#"[functions."fixture:api.delegata"]
symbol = "crate::shim::delegata"

[shim]
path = "rust/shim.rs"
"#,
        "",
    );
    let outside = root.with_extension("outside.rs");
    fs::write(
        &outside,
        "pub fn delegata(value: String) -> String { value }\n",
    )
    .expect("write outside shim");
    fs::remove_file(root.join("rust/shim.rs")).expect("remove placeholder shim");
    symlink(&outside, root.join("rust/shim.rs")).expect("symlink escaping shim");

    let diagnostics = verify_library_bindings(&root, "rust").expect_err("symlink escape rejected");
    assert!(has_issue(&diagnostics, "package_member_symlink_escape"));
}

#[cfg(unix)]
#[test]
fn missing_source_below_symlinked_parent_is_rejected() {
    use std::os::unix::fs::symlink;

    let root = test_package(
        "source-missing-symlink",
        "functio localis() → textus { redde \"ok\" }\n",
        "",
        "",
    );
    let outside = root.with_extension("outside-dir");
    fs::create_dir_all(&outside).expect("create outside directory");
    symlink(&outside, root.join("linked")).expect("symlink escaping source parent");
    let manifest = fs::read_to_string(root.join("faber.toml")).expect("read manifest");
    fs::write(
        root.join("faber.toml"),
        manifest.replace("source = \"src\"", "source = \"linked/missing\""),
    )
    .expect("rewrite manifest");

    let diagnostics =
        verify_library_bindings(&root, "rust").expect_err("missing symlink child rejected");
    assert!(has_issue(&diagnostics, "package_member_symlink_escape"));
}

#[cfg(unix)]
#[test]
fn symlinked_faber_source_outside_source_root_is_rejected() {
    use std::os::unix::fs::symlink;

    let root = test_package(
        "source-file-symlink",
        "functio localis() → textus { redde \"ok\" }\n",
        "",
        "",
    );
    let outside = root.with_extension("outside.fab");
    fs::write(
        &outside,
        "functio abscondita() → textus { redde \"outside\" }\n",
    )
    .expect("write outside source");
    symlink(&outside, root.join("src/escape.fab")).expect("symlink escaping source file");

    let diagnostics =
        verify_library_bindings(&root, "rust").expect_err("source symlink escape rejected");
    assert!(has_issue(&diagnostics, "package_source_symlink_escape"));
}
