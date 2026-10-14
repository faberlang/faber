//! Tests for import path resolution and provider segment validation.
//! Proves: canonical and non-canonical import paths resolve/diagnose
//! correctly; file-interface paths are accepted; missing imports fail
//! with expected diagnostics.

use super::*;
use std::path::Path;

// ── library-home thread-local override (test support) ───────────────────────

pub(crate) mod library_home_test_support {
    use std::cell::RefCell;
    use std::path::PathBuf;

    thread_local! {
        /// Thread-local override so `compile_test` env-var mutations don't race
        /// parallel vertex-source tests that also call `library_home()`.
        static OVERRIDE: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
    }

    /// Read the current thread-local library-home override.
    pub(crate) fn get_thread_library_home() -> Option<PathBuf> {
        OVERRIDE.with(|cell| cell.borrow().clone())
    }

    /// Set the thread-local library-home override. Returns the previous value.
    pub(crate) fn set_thread_library_home(path: Option<PathBuf>) -> Option<PathBuf> {
        OVERRIDE.with(|cell| cell.replace(path))
    }
}

// ── provider segment validation ─────────────────────────────────────────────

#[test]
fn valid_provider_segments_accepted() {
    assert!(is_valid_provider_segment("norma"));
    assert!(is_valid_provider_segment("my_provider"));
    assert!(is_valid_provider_segment("my-provider"));
    assert!(is_valid_provider_segment("a"));
    assert!(is_valid_provider_segment("provider123"));
}

#[test]
fn invalid_provider_segments_rejected() {
    assert!(!is_valid_provider_segment(""));
    assert!(!is_valid_provider_segment("provider:name"));
    assert!(!is_valid_provider_segment("provider/name"));
    assert!(!is_valid_provider_segment("provider.name"));
    assert!(!is_valid_provider_segment("spaced name"));
}

// ── paths_source_from_manifest ──────────────────────────────────────────────

#[test]
fn paths_source_from_manifest_empty_toml() {
    let config = paths_source_from_manifest("");
    assert!(config.is_none());
}

#[test]
fn paths_source_from_manifest_missing_paths_section() {
    let toml = r#"
[package]
name = "test"
"#;
    let config = paths_source_from_manifest(toml);
    assert!(config.is_none());
}

#[test]
fn paths_source_from_manifest_only_source() {
    let toml = r#"
[paths]
source = "src"
"#;
    let config = paths_source_from_manifest(toml);
    assert_eq!(config, Some("src".to_owned()));
}

#[test]
fn paths_source_from_manifest_accepts_toml_whitespace_and_inline_comment() {
    let config = paths_source_from_manifest("[ paths ]\nsource = \"src\" # source root\n");
    assert_eq!(config.as_deref(), Some("src"));
}

#[test]
fn provider_name_from_manifest_uses_toml_table() {
    assert_eq!(
        provider_name_from_manifest("[library]\nprovider = \"web\" # segment\n").as_deref(),
        Some("web")
    );
}

#[test]
fn paths_source_from_manifest_missing_source_key() {
    let toml = r#"
[paths]
output = "dist"
"#;
    let config = paths_source_from_manifest(toml);
    assert!(config.is_none());
}

#[test]
fn paths_source_from_manifest_source_with_templates_section() {
    let toml = r#"
[paths]
source = "lib"

[paths.templates]
assets = "assets"
"#;
    let config = paths_source_from_manifest(toml);
    assert_eq!(config, Some("lib".to_owned()));
}

#[test]
fn paths_source_from_manifest_empty_source_value_skipped() {
    let toml = r#"
[paths]
source = ""
"#;
    let config = paths_source_from_manifest(toml);
    assert!(config.is_none());
}

#[test]
fn paths_source_from_manifest_source_with_nested_path() {
    let toml = r#"
[paths]
source = "src/faber"
"#;
    let config = paths_source_from_manifest(toml);
    assert_eq!(config, Some("src/faber".to_owned()));
}

#[test]
fn paths_source_from_manifest_other_sections_ignored() {
    let toml = r#"
[build]
source = "elsewhere"

[paths]
source = "src"

[paths.templates]
extra = "extra_dir"
"#;
    let config = paths_source_from_manifest(toml);
    assert_eq!(config, Some("src".to_owned()));
}

// ── resolve_fab_file ────────────────────────────────────────────────────────

#[test]
fn resolve_fab_file_returns_none_for_nonexistent_path() {
    let result = resolve_fab_file(Path::new("/nonexistent/path.fab"));
    assert!(result.is_none());
}

#[test]
fn resolve_fab_file_returns_none_for_nonexistent_directory_module() {
    let result = resolve_fab_file(Path::new("/nonexistent/mod"));
    assert!(result.is_none());
}

// ── resolve_namespace_import (filesystem-dependent) ──────────────────────────

#[test]
fn resolve_namespace_import_returns_err_for_empty_import() {
    let result = resolve_namespace_import(&fresh_session(), Path::new("test.fab"), "", None);
    assert!(result.is_err());
}

#[test]
fn resolve_namespace_import_returns_err_for_malformed_provider() {
    let result = resolve_namespace_import(&fresh_session(), Path::new("test.fab"), ":module", None);
    assert!(result.is_err());
}

#[test]
fn resolve_namespace_import_returns_err_for_empty_provider() {
    let result = resolve_namespace_import(&fresh_session(), Path::new("test.fab"), ":", None);
    assert!(result.is_err());
}

#[test]
fn resolve_namespace_import_returns_err_for_norma_stdlib_nonexistent() {
    let result = resolve_namespace_import(&fresh_session(), Path::new("test.fab"), "norma:nonexistent_module", None);
    assert!(result.is_err());
}

#[test]
fn resolve_namespace_import_returns_err_for_unknown_provider() {
    let result = resolve_namespace_import(&fresh_session(), Path::new("test.fab"), "xeno:module", None);
    assert!(result.is_err());
}

#[test]
fn resolve_namespace_import_returns_err_for_provider_with_colon() {
    let result = resolve_namespace_import(&fresh_session(), Path::new("test.fab"), "provider:module:extra", None);
    assert!(result.is_err());
}

#[test]
fn resolve_namespace_import_returns_err_for_non_import_identifier() {
    let result = resolve_namespace_import(&fresh_session(), Path::new("test.fab"), "just_a_name", None);
    assert!(result.is_err());
}

#[test]
fn resolve_namespace_import_returns_err_for_nonexistent_relative_path() {
    let result = resolve_namespace_import(&fresh_session(), Path::new("/tmp/test.fab"), "./nonexistent", None);
    assert!(result.is_err());
}

#[test]
fn resolve_namespace_import_returns_err_for_parent_relative_nonexistent() {
    let result = resolve_namespace_import(&fresh_session(), Path::new("/tmp/sub/test.fab"), "../nonexistent", None);
    assert!(result.is_err());
}

#[test]
fn resolve_namespace_import_returns_err_for_unadorned_relative_looking_path() {
    let result = resolve_namespace_import(&fresh_session(), Path::new("/tmp/test.fab"), "relative/path", None);
    assert!(result.is_err());
}

// ── G1-U1: canonicalize-and-contain on the relative arm ─────────────────────

#[test]
fn resolve_namespace_import_relative_parent_inside_package_resolves() {
    // Containment is decided on the resolved path, not the spelling: a legal
    // in-package `../` import must keep resolving.
    let dir = crate::test_support::test_temp_dir("rel-parent-inside");
    let root = dir.path();
    std::fs::write(root.join("faber.toml"), "[package]\nname = \"demo\"\n").unwrap();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("b.fab"), "functio f() → vacuum { }\n").unwrap();
    let from = root.join("src").join("a.fab");
    std::fs::write(&from, "functio g() → vacuum { }\n").unwrap();

    let result = resolve_namespace_import(&fresh_session(), &from, "../b", None);
    let target = result.expect("in-package parent import must resolve");
    assert!(target.ends_with("b.fab"), "target: {target:?}");
}

#[test]
fn resolve_namespace_import_relative_parent_escape_rejected() {
    // The escape target exists on disk, so only containment can reject it.
    let dir = crate::test_support::test_temp_dir("rel-parent-escape");
    let root = dir.path();
    let pkg = root.join("pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(pkg.join("faber.toml"), "[package]\nname = \"demo\"\n").unwrap();
    std::fs::write(root.join("outside.fab"), "functio f() → vacuum { }\n").unwrap();
    let from = pkg.join("app.fab");
    std::fs::write(&from, "").unwrap();

    let error = resolve_namespace_import(&fresh_session(), &from, "../outside", None)
        .expect_err("parent escape must be rejected");
    assert_eq!(error.issue, "import_resolve_escape", "{error:?}");
}

#[cfg(unix)]
#[test]
fn resolve_namespace_import_relative_symlink_escape_rejected() {
    // An in-tree symlink whose target lives outside the package root resolves
    // through `is_file` but must be rejected before the target is read.
    let dir = crate::test_support::test_temp_dir("rel-symlink-escape");
    let root = dir.path();
    let pkg = root.join("pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(pkg.join("faber.toml"), "[package]\nname = \"demo\"\n").unwrap();
    std::fs::write(root.join("secret.fab"), "functio f() → vacuum { }\n").unwrap();
    std::os::unix::fs::symlink(root.join("secret.fab"), pkg.join("link.fab")).unwrap();
    let from = pkg.join("app.fab");
    std::fs::write(&from, "").unwrap();

    let error =
        resolve_namespace_import(&fresh_session(), &from, "./link", None).expect_err("symlink escape must be rejected");
    assert_eq!(error.issue, "import_resolve_escape", "{error:?}");
}

// ── resolve_fab_file with temp directory ────────────────────────────────────

#[test]
fn resolve_fab_file_finds_file_with_explicit_extension() {
    let dir = crate::test_support::test_temp_dir("import-ext");
    let file_path = dir.path().join("test.fab");
    std::fs::write(&file_path, "functio f() → vacuum { }").ok();

    let result = resolve_fab_file(&file_path);
    assert_eq!(result, Some(file_path));
}

#[test]
fn resolve_fab_file_adds_fab_extension() {
    let dir = crate::test_support::test_temp_dir("import-fab-ext");
    let file_path = dir.path().join("salve.fab");
    std::fs::write(&file_path, "functio f() → vacuum { }").ok();

    let result = resolve_fab_file(&dir.path().join("salve"));
    assert_eq!(result, Some(file_path));
}

#[test]
fn resolve_fab_file_finds_mod_fab_directory_module() {
    let dir = crate::test_support::test_temp_dir("import-mod");
    let mod_file = dir.path().join("mod.fab");
    std::fs::write(&mod_file, "functio f() → vacuum { }").ok();

    let result = resolve_fab_file(dir.path());
    assert_eq!(result, Some(mod_file));
}

#[test]
fn resolve_fab_file_prefers_dot_fab_over_directory_module() {
    let dir = crate::test_support::test_temp_dir("import-pref");

    let file = dir.path().join("salve.fab");
    std::fs::write(&file, "functio f() → vacuum { }").ok();

    let sub = dir.path().join("salve");
    let _ = std::fs::create_dir_all(&sub);
    let mod_file = sub.join("mod.fab");
    std::fs::write(&mod_file, "functio g() → vacuum { }").ok();

    let result = resolve_fab_file(&dir.path().join("salve"));
    assert_eq!(result, Some(file));
}

// ── provider segment edge cases ─────────────────────────────────────────────

#[test]
fn rejects_newline_in_provider_segment() {
    assert!(!is_valid_provider_segment("provider\nname"));
}

#[test]
fn rejects_tab_in_provider_segment() {
    assert!(!is_valid_provider_segment("provider\tname"));
}

#[test]
fn rejects_backslash_in_provider_segment() {
    assert!(!is_valid_provider_segment("provider\\name"));
}

#[test]
fn paths_source_from_manifest_empty_paths_section() {
    let toml = r"
[paths]
";
    let config = paths_source_from_manifest(toml);
    assert!(config.is_none());
}

// ── PRP-066 BO-D-1 package root memo ────────────────────────────────────────

/// One session per test = one driver run (ARCH-U3): the package-root memo
/// lives on the session, so memoization asserts share one session.
fn fresh_session() -> Session {
    crate::driver::Session::new(crate::driver::Config::default())
}

#[test]
fn package_root_is_memoized_for_same_package() {
    let session = fresh_session();
    let dir = crate::test_support::test_temp_dir("pkg-root-memo");
    let root = dir.path();
    std::fs::write(root.join("faber.toml"), "[package]\nname = \"demo\"\n").unwrap();
    let deep = root.join("a").join("b").join("c");
    std::fs::create_dir_all(&deep).unwrap();
    let file_a = deep.join("one.fab");
    let file_b = deep.join("two.fab");
    std::fs::write(&file_a, "").unwrap();
    std::fs::write(&file_b, "").unwrap();

    let first = find_package_root_with_session(&session, &file_a);
    let second = find_package_root_with_session(&session, &file_b);
    assert_eq!(first.as_deref(), Some(root));
    assert_eq!(first, second);

    // Sibling deeper path still resolves to same root via cache fill.
    let deeper = deep.join("d");
    std::fs::create_dir_all(&deeper).unwrap();
    let file_c = deeper.join("three.fab");
    std::fs::write(&file_c, "").unwrap();
    assert_eq!(find_package_root_with_session(&session, &file_c), first);
}

#[test]
fn package_root_distinct_packages_are_not_confused() {
    let session = fresh_session();
    let outer = crate::test_support::test_temp_dir("pkg-root-multi");
    let pkg_a = outer.path().join("a");
    let pkg_b = outer.path().join("b");
    std::fs::create_dir_all(pkg_a.join("src")).unwrap();
    std::fs::create_dir_all(pkg_b.join("src")).unwrap();
    std::fs::write(pkg_a.join("faber.toml"), "[package]\nname = \"a\"\n").unwrap();
    std::fs::write(pkg_b.join("faber.toml"), "[package]\nname = \"b\"\n").unwrap();
    let file_a = pkg_a.join("src").join("main.fab");
    let file_b = pkg_b.join("src").join("main.fab");
    std::fs::write(&file_a, "").unwrap();
    std::fs::write(&file_b, "").unwrap();

    assert_eq!(
        find_package_root_with_session(&session, &file_a).as_deref(),
        Some(pkg_a.as_path())
    );
    assert_eq!(
        find_package_root_with_session(&session, &file_b).as_deref(),
        Some(pkg_b.as_path())
    );
}

// ── BO-D-1 library_home memo ────────────────────────────────────────────────

#[test]
fn library_home_memo_preserves_thread_override_precedence() {
    // The OnceLock memo cannot be reset, so this test proves the source spec §5
    // risk directly: the thread-local test override must still win after the
    // production cache has been initialized.

    // (a) Initialize the production cache via the env-var/ancestor-walk path.
    let production = library_home();

    // (b) Override with a fresh temp dir — must win after cache init.
    let dir = crate::test_support::test_temp_dir("libhome-memo-override");
    let override_path = dir.path().to_path_buf();
    let previous = library_home_test_support::set_thread_library_home(Some(override_path.clone()));

    // (c) The thread-local override keeps precedence over the cached value.
    assert_eq!(library_home(), Some(override_path));

    // (d) Restore the previous override (defensive; `None` on a fresh thread).
    library_home_test_support::set_thread_library_home(previous);

    // (e) The cache serves the deterministic production value again.
    assert_eq!(library_home(), production);
}

/// Serializes the tests below that mutate the process-wide configured home,
/// so their transient values cannot interleave.
static CONFIGURED_LIBRARY_HOME_TEST_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn configured_library_home_setting_round_trips() {
    let _guard = CONFIGURED_LIBRARY_HOME_TEST_GUARD
        .lock()
        .expect("configured-home test guard");
    let dir = crate::test_support::test_temp_dir("libhome-setting");
    let home = dir.path().to_path_buf();

    set_configured_library_home(Some(home.clone()));
    assert_eq!(configured_library_home_setting(), Some(home));

    set_configured_library_home(None);
    assert_eq!(configured_library_home_setting(), None);
}

#[test]
fn configured_library_home_ignores_environment_variable() {
    let _guard = CONFIGURED_LIBRARY_HOME_TEST_GUARD
        .lock()
        .expect("configured-home test guard");
    let dir = crate::test_support::test_temp_dir("libhome-ignores-env");
    let home = dir.path().to_path_buf();

    set_configured_library_home(Some(home.clone()));
    assert_eq!(configured_library_home_setting(), Some(home));

    set_configured_library_home(None);
    assert!(configured_library_home_setting().is_none());
}

// ── U6b: `norma:*` resolves from the in-tree `lib/` ──────────────────────────

#[test]
fn norma_resolves_from_in_tree_lib_when_no_home_configured() {
    // No thread-local override on a fresh test thread; the process env var is
    // not set by the suite. `norma:math` must come from `<repo>/lib/norma/src`.
    let in_tree = in_tree_library_home().expect("<repo>/lib exists in the tree");
    let resolved = resolve_namespace_import(&fresh_session(), Path::new("test.fab"), "norma:math", None)
        .expect("norma:math resolves with no home configured");
    let expected_root = in_tree.join("norma/src");
    assert!(
        resolved.starts_with(&expected_root),
        "{} not under {}",
        resolved.display(),
        expected_root.display()
    );
    assert!(resolved.is_file());
}

#[test]
fn norma_resolves_from_tree_when_configured_home_lacks_it() {
    // A configured home holding only a fake provider: `norma` is not there, so
    // it falls back to the in-tree home; the fake provider still comes from the
    // configured home.
    let dir = crate::test_support::test_temp_dir("libhome-norma-fallback");
    let fake_src = dir.path().join("fakeprov/src");
    std::fs::create_dir_all(&fake_src).unwrap();
    std::fs::write(fake_src.join("thing.fab"), "").unwrap();
    let previous = library_home_test_support::set_thread_library_home(Some(dir.path().to_path_buf()));

    let norma = resolve_namespace_import(&fresh_session(), Path::new("test.fab"), "norma:math", None);
    let fake = resolve_namespace_import(&fresh_session(), Path::new("test.fab"), "fakeprov:thing", None);

    library_home_test_support::set_thread_library_home(previous);

    let in_tree = in_tree_library_home().expect("<repo>/lib exists in the tree");
    let norma = norma.expect("norma:math resolves from the in-tree lib");
    assert!(norma.starts_with(in_tree.join("norma/src")), "{}", norma.display());
    let fake = fake.expect("fakeprov:thing resolves from the configured home");
    assert!(fake.starts_with(dir.path()), "{}", fake.display());
}

#[test]
fn norma_fault_union_matches_by_variant() {
    // The IoFault union crosses as an ordinary value: `io_fault_from` turns a
    // parsed record into the variant named by `kind`, and a flat `match`
    // handles it arm by arm. The app must check with no error diagnostics.
    let dir = crate::test_support::test_temp_dir("norma-fault-union");
    let app = dir.path().join("app.fab");
    let source = r#"+++
locale = "en"
+++

import from "norma:fault" fault
import from "norma:json" json

main {
    const json document ← json.parse(«{"kind":"NotFound","path":"/x"}»)
    const value raw ← document ↦ value
    const fault.IoFault got ← fault.io_fault_from(raw)
    match got {
        case NotFound const path {
            print path
        }
        case Io const path {
            print path
        }
        default {
            print "other"
        }
    }
}
"#;
    std::fs::write(&app, source).expect("write norma:fault app");
    let result = crate::services::check_diagnostics_for_source(&app.display().to_string(), source, false);
    assert!(
        result.success,
        "norma:fault union match must check without error diagnostics: {:?}",
        result
            .diagnostics
            .iter()
            .filter(|diag| diag.is_error())
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
}

// ── F-046: path traversal via template rest ─────────────────────────────────

#[test]
fn contains_path_traversal_rejects_bare_dotdot() {
    assert!(contains_path_traversal(".."));
}

#[test]
fn contains_path_traversal_rejects_dotdot_prefix() {
    assert!(contains_path_traversal("../escape"));
}

#[test]
fn contains_path_traversal_rejects_embedded_dotdot() {
    assert!(contains_path_traversal("good/../bad"));
}

#[test]
fn contains_path_traversal_rejects_trailing_dotdot() {
    assert!(contains_path_traversal("good/.."));
}

#[test]
fn contains_path_traversal_allows_clean_path() {
    assert!(!contains_path_traversal("sub/module"));
}

#[test]
fn contains_path_traversal_allows_single_segment() {
    assert!(!contains_path_traversal("module"));
}

#[test]
fn contains_path_traversal_allows_deep_clean_path() {
    assert!(!contains_path_traversal("a/b/c/d"));
}

#[test]
fn contains_path_traversal_allows_dotted_filename() {
    // Filenames with dots are fine — only literal `..` segments are traversal.
    assert!(!contains_path_traversal("some.file.fab"));
    assert!(!contains_path_traversal("dir/some.file.fab"));
}

// ── F-046 residual: absolute-path rest segments ─────────────────────────────

#[test]
fn contains_path_traversal_rejects_absolute_prefix() {
    // rest starting with `/` is absolute → `Path::join` replaces the base.
    assert!(contains_path_traversal("/etc/passwd"));
    assert!(contains_path_traversal("/"));
    assert!(contains_path_traversal("/usr/bin"));
}

#[test]
fn contains_path_traversal_allows_relative_but_starts_with_slash_subpath() {
    // Traversal check must not reject valid relative paths that happen to
    // contain a subpath starting with `/` after a separator (impossible in
    // practice since `/` is the separator, but the logic should be clean).
    assert!(!contains_path_traversal("sub/module"));
}

#[test]
fn resolve_template_import_rejects_absolute_rest() {
    // §src//etc/passwd should be rejected because rest starts with `/`.
    let dir = crate::test_support::test_temp_dir("rest-absolute");
    std::fs::write(dir.path().join("faber.toml"), "[package]\n").unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    let from = dir.path().join("main.fab");
    std::fs::write(&from, "").unwrap();

    let mut templates = BTreeMap::new();
    templates.insert("src".to_owned(), "src".to_owned());

    let result = resolve_namespace_import(&fresh_session(), &from, "§src//etc/passwd", Some(&templates));
    assert!(result.is_err(), "absolute rest must be rejected");
}

#[test]
fn resolve_template_import_rejects_rest_bare_dotdot() {
    // §src/.. should be rejected because rest == ".."
    let dir = crate::test_support::test_temp_dir("rest-dotdot");
    std::fs::write(dir.path().join("faber.toml"), "[package]\n").unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    let from = dir.path().join("main.fab");
    std::fs::write(&from, "").unwrap();

    let mut templates = BTreeMap::new();
    templates.insert("src".to_owned(), "src".to_owned());

    let result = resolve_namespace_import(&fresh_session(), &from, "§src/..", Some(&templates));
    assert!(result.is_err(), "bare .. rest must be rejected");
}

#[test]
fn resolve_template_import_rejects_rest_dotdot_prefix() {
    // §src/../escape should be rejected
    let dir = crate::test_support::test_temp_dir("rest-dotdot-prefix");
    std::fs::write(dir.path().join("faber.toml"), "[package]\n").unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    let from = dir.path().join("main.fab");
    std::fs::write(&from, "").unwrap();

    let mut templates = BTreeMap::new();
    templates.insert("src".to_owned(), "src".to_owned());

    let result = resolve_namespace_import(&fresh_session(), &from, "§src/../escape", Some(&templates));
    assert!(result.is_err(), ".. prefix in rest must be rejected");
}

#[test]
fn resolve_template_import_rejects_rest_embedded_dotdot() {
    // §src/a/../b should be rejected
    let dir = crate::test_support::test_temp_dir("rest-embedded-dotdot");
    std::fs::write(dir.path().join("faber.toml"), "[package]\n").unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    let from = dir.path().join("main.fab");
    std::fs::write(&from, "").unwrap();

    let mut templates = BTreeMap::new();
    templates.insert("src".to_owned(), "src".to_owned());

    let result = resolve_namespace_import(&fresh_session(), &from, "§src/a/../b", Some(&templates));
    assert!(result.is_err(), "embedded .. in rest must be rejected");
}

#[test]
fn resolve_template_import_rejects_rest_trailing_dotdot() {
    // §src/a/.. should be rejected
    let dir = crate::test_support::test_temp_dir("rest-trailing-dotdot");
    std::fs::write(dir.path().join("faber.toml"), "[package]\n").unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    let from = dir.path().join("main.fab");
    std::fs::write(&from, "").unwrap();

    let mut templates = BTreeMap::new();
    templates.insert("src".to_owned(), "src".to_owned());

    let result = resolve_namespace_import(&fresh_session(), &from, "§src/a/..", Some(&templates));
    assert!(result.is_err(), "trailing .. in rest must be rejected");
}

#[test]
fn resolve_template_import_allows_clean_rest() {
    // §src/sub/module should resolve normally
    let dir = crate::test_support::test_temp_dir("rest-clean");
    std::fs::write(dir.path().join("faber.toml"), "[package]\n").unwrap();
    std::fs::create_dir_all(dir.path().join("src").join("sub")).unwrap();
    let target = dir.path().join("src").join("sub").join("module.fab");
    std::fs::write(&target, "").unwrap();
    let from = dir.path().join("main.fab");
    std::fs::write(&from, "").unwrap();

    let mut templates = BTreeMap::new();
    templates.insert("src".to_owned(), "src".to_owned());

    let result = resolve_namespace_import(&fresh_session(), &from, "§src/sub/module", Some(&templates));
    assert_eq!(result, Ok(target));
}

// ── F-114: installed provider source path must not silently fall back ───────

#[test]
fn installed_provider_source_root_returns_configured_source() {
    // When faber.toml has [paths].source = "lib" and lib/ exists, return it.
    let dir = crate::test_support::test_temp_dir("provider-source-configured");
    let provider = dir.path().join("my_provider");
    std::fs::create_dir_all(provider.join("lib")).unwrap();
    std::fs::write(provider.join("faber.toml"), "[paths]\nsource = \"lib\"\n").unwrap();

    let root = installed_provider_source_root(&provider);
    assert_eq!(root, Some(provider.join("lib")));
}

#[test]
fn installed_provider_source_root_falls_back_to_src_when_no_manifest() {
    // No faber.toml → fall back to src/ or provider root.
    let dir = crate::test_support::test_temp_dir("provider-source-fallback-src");
    let provider = dir.path().join("my_provider");
    std::fs::create_dir_all(provider.join("src")).unwrap();

    let root = installed_provider_source_root(&provider);
    assert_eq!(root, Some(provider.join("src")));
}

#[test]
fn installed_provider_source_root_returns_none_when_configured_source_missing() {
    // faber.toml has [paths].source = "lib" but lib/ does not exist.
    // Must NOT silently fall back to src/ or provider root (F-114).
    let dir = crate::test_support::test_temp_dir("provider-source-missing");
    let provider = dir.path().join("my_provider");
    std::fs::create_dir_all(&provider).unwrap();
    // Write a faber.toml that declares a source that does not exist.
    // Also include a src/ dir to prove the configured-path check wins.
    std::fs::create_dir_all(provider.join("src")).unwrap();
    std::fs::write(provider.join("faber.toml"), "[paths]\nsource = \"lib\"\n").unwrap();

    let root = installed_provider_source_root(&provider);
    assert!(
        root.is_none(),
        "when configured source path is missing, must return None, got: {root:?}"
    );
}

#[test]
fn installed_provider_source_root_falls_back_to_src_when_source_is_dir() {
    // No faber.toml, no src/ dir → fall back to provider root.
    let dir = crate::test_support::test_temp_dir("provider-source-fallback-root");
    let provider = dir.path().join("my_provider");
    std::fs::create_dir_all(&provider).unwrap();

    let root = installed_provider_source_root(&provider);
    assert_eq!(root, Some(provider.clone()));
}
