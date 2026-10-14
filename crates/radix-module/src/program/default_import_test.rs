//! Default import names from `regio` (DECISIONS D7.7), radix file-namespace path.
//!
//! `importa ex "./geo"` binds the target's declared `regio` name; a target
//! without one keeps the last path segment; an explicit name always wins; and
//! two imports that would default to the same name are an error.

use std::fs;
use std::path::{Path, PathBuf};

use crate::import_resolve::library_home_test_support;


/// The built-in Latin (`la`) reader pack: the canonical surface these tests
/// are written in.
fn la_pack() -> crate::locale::LocalePack {
    crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("la"))
        .expect("installed la pack should load")
}

fn temp_dir(label: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = crate::test_support::test_temp_dir(&format!("default-import-{label}"));
    let path = dir.path().to_path_buf();
    (dir, path)
}

const GEO_WITH_REGIO: &str = "regio geometria\n\n@ publica\nfunctio unum() → numerus {\n    redde 1\n}\n";
const GEO_WITHOUT_REGIO: &str = "@ publica\nfunctio unum() → numerus {\n    redde 1\n}\n";

fn check(dir: &Path, app: &str) -> Vec<Option<String>> {
    let path = dir.join("app.fab");
    fs::write(&path, app).expect("app");
    let result = crate::services::check_diagnostics_for_source_with_locale_pack(
        &path.display().to_string(),
        app,
        false,
        Some(&la_pack()),
        None,
    );
    result
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.is_error())
        .map(|diagnostic| diagnostic.issue().map(str::to_owned))
        .collect()
}

fn write(dir: &Path, relative: &str, source: &str) {
    let path = dir.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("dir");
    }
    fs::write(path, source).expect("write");
}

#[test]
fn default_import_binds_declared_regio_name() {
    let (_td, dir) = temp_dir("declared");
    write(&dir, "geo.fab", GEO_WITH_REGIO);
    let errors = check(&dir, "importa ex \"./geo\"\n\nincipit {\n    nota geometria.unum()\n}\n");
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn default_import_of_regio_file_does_not_bind_path_segment() {
    let (_td, dir) = temp_dir("segment-gone");
    write(&dir, "geo.fab", GEO_WITH_REGIO);
    let errors = check(&dir, "importa ex \"./geo\"\n\nincipit {\n    nota geo.unum()\n}\n");
    assert!(!errors.is_empty(), "`geo` must not be bound when geo.fab declares regio");
}

#[test]
fn file_without_regio_keeps_path_segment_default() {
    let (_td, dir) = temp_dir("fallback");
    write(&dir, "geo.fab", GEO_WITHOUT_REGIO);
    let errors = check(&dir, "importa ex \"./geo\"\n\nincipit {\n    nota geo.unum()\n}\n");
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn explicit_import_name_overrides_declared_regio() {
    let (_td, dir) = temp_dir("explicit");
    write(&dir, "geo.fab", GEO_WITH_REGIO);
    let errors = check(&dir, "importa ex \"./geo\" geo\n\nincipit {\n    nota geo.unum()\n}\n");
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn two_imports_defaulting_to_one_regio_name_collide() {
    let (_td, dir) = temp_dir("collision");
    write(&dir, "plana.fab", GEO_WITH_REGIO);
    write(&dir, "solida.fab", GEO_WITH_REGIO);
    let errors = check(
        &dir,
        "importa ex \"./plana\"\nimporta ex \"./solida\"\n\nincipit {\n    nota geometria.unum()\n}\n",
    );
    assert!(
        errors
            .iter()
            .any(|issue| issue.as_deref() == Some("default_import_name_collision")),
        "{errors:?}"
    );
}

#[test]
fn aliasing_one_colliding_import_resolves_it() {
    let (_td, dir) = temp_dir("collision-aliased");
    write(&dir, "plana.fab", GEO_WITH_REGIO);
    write(&dir, "solida.fab", GEO_WITH_REGIO);
    let errors = check(
        &dir,
        "importa ex \"./plana\"\nimporta ex \"./solida\" solida\n\nincipit {\n    nota geometria.unum()\n    nota solida.unum()\n}\n",
    );
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn library_default_import_binds_declared_regio_name() {
    let (_td, home) = temp_dir("library");
    let provider = home.join("probe");
    write(
        &provider,
        "faber.toml",
        "[package]\nname = \"probe\"\n[library]\nprovider = \"probe\"\n[paths]\nsource = \"src\"\n[build]\nkind = \"lib\"\ntargets = [\"rust\"]\n",
    );
    write(&provider, "src/geo.fab", GEO_WITH_REGIO);
    let previous = library_home_test_support::set_thread_library_home(Some(home.clone()));
    let errors = check(&home, "importa ex \"probe:geo\"\n\nincipit {\n    nota geometria.unum()\n}\n");
    library_home_test_support::set_thread_library_home(previous);
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn driver_analysis_binds_declared_regio_name() {
    // The prepared-program driver path (emit, run) parses the consumer again;
    // the contract's default-name map must rewrite that parse too.
    let (_td, dir) = temp_dir("driver");
    write(&dir, "geo.fab", GEO_WITH_REGIO);
    let app = "importa ex \"./geo\"\n\nincipit {\n    nota geometria.unum()\n}\n";
    let path = dir.join("app.fab");
    fs::write(&path, app).expect("app");
    let session = crate::driver::Session::new(crate::driver::Config::default().with_latin_locale());
    let analysis = crate::program::analyze_prepared_program_path(&session, &path, app);
    assert!(
        analysis.is_ok(),
        "{:?}",
        analysis
            .err()
            .unwrap_or_default()
            .iter()
            .map(|diagnostic| diagnostic.issue().map(str::to_owned))
            .collect::<Vec<_>>()
    );
}
