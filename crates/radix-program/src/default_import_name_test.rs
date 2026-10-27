//! Default import names from `module` (DECISIONS D7.7), package-analysis path.
//!
//! The package loader binds a default-named import to its target's declared
//! `module` name, for local siblings and library modules alike; a target
//! without one keeps the last path segment.

use crate::analyze::analyze_package;
use crate::test_support::{test_analysis_seams, test_temp_dir};
use radix_module::driver::Config;
use std::fs;
use std::path::Path;

const GEO_WITH_MODULE: &str = "module geometry\n\n@ public\nfn one() → int {\n    return 1\n}\n";
const GEO_WITHOUT_MODULE: &str = "@ public\nfn one() → int {\n    return 1\n}\n";

fn write(dir: &Path, relative: &str, source: &str) {
    let path = dir.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("dir");
    }
    fs::write(path, source).expect("write");
}

type ErrorFacts = (Option<String>, Vec<(String, String)>);

fn error_facts(diagnostic: &radix_module::Diagnostic) -> ErrorFacts {
    (
        diagnostic.issue().map(str::to_owned),
        diagnostic
            .args
            .iter()
            .map(|arg| (arg.name.to_owned(), arg.value.clone()))
            .collect(),
    )
}

fn errors(entry: &Path) -> Vec<ErrorFacts> {
    let diagnostics = match analyze_package(&Config::default(), entry, &test_analysis_seams()) {
        Ok(program) => program.diagnostics,
        Err(diagnostics) => diagnostics,
    };
    diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.is_error())
        .map(error_facts)
        .collect()
}

#[test]
fn local_default_import_binds_declared_module_name() {
    let dir = test_temp_dir("default-import-local");
    write(dir.path(), "geo.fab", GEO_WITH_MODULE);
    let entry = dir.path().join("main.fab");
    write(
        dir.path(),
        "main.fab",
        "import from \"./geo\"\n\nmain {\n    print geometry.one()\n}\n",
    );
    let program = analyze_package(&Config::default(), &entry, &test_analysis_seams())
        .expect("analyze package");
    let entry_id = program.entry.clone().expect("entry");
    let local = program.imports.local_edges(&entry_id);
    assert_eq!(local.len(), 1);
    assert_eq!(local[0].0, "geometry");
    assert!(
        !program
            .diagnostics
            .iter()
            .any(radix_module::Diagnostic::is_error),
        "{:?}",
        program.diagnostics
    );
}

#[test]
fn local_default_import_without_module_keeps_path_segment() {
    let dir = test_temp_dir("default-import-fallback");
    write(dir.path(), "geo.fab", GEO_WITHOUT_MODULE);
    let entry = dir.path().join("main.fab");
    write(
        dir.path(),
        "main.fab",
        "import from \"./geo\"\n\nmain {\n    print geo.one()\n}\n",
    );
    assert_eq!(errors(&entry), Vec::new());
}

#[test]
fn local_default_import_does_not_bind_path_segment_when_module_declared() {
    let dir = test_temp_dir("default-import-segment-gone");
    write(dir.path(), "geo.fab", GEO_WITH_MODULE);
    let entry = dir.path().join("main.fab");
    write(
        dir.path(),
        "main.fab",
        "import from \"./geo\"\n\nmain {\n    print geo.one()\n}\n",
    );
    assert!(!errors(&entry).is_empty());
}

#[test]
fn two_default_imports_of_one_module_name_collide() {
    let dir = test_temp_dir("default-import-collision");
    write(dir.path(), "plane.fab", GEO_WITH_MODULE);
    write(dir.path(), "solid.fab", GEO_WITH_MODULE);
    let entry = dir.path().join("main.fab");
    write(
        dir.path(),
        "main.fab",
        "import from \"./plane\"\nimport from \"./solid\"\n\nmain {\n    print geometry.one()\n}\n",
    );
    let errors = errors(&entry);
    assert!(
        errors
            .iter()
            .any(|(issue, _)| issue.as_deref() == Some("default_import_name_collision")),
        "{errors:?}"
    );
}

#[test]
fn library_default_import_binds_declared_module_name() {
    // The provider package declares the Latin reader locale for its source.
    let home = test_temp_dir("default-import-library-home");
    write(
        home.path(),
        "probe/faber.toml",
        "[package]\nname = \"probe\"\n[reader]\nlocale = \"la\"\n[library]\nprovider = \"probe\"\n[paths]\nsource = \"src\"\n[build]\nkind = \"lib\"\ntargets = [\"rust\"]\n",
    );
    write(
        home.path(),
        "probe/src/geo.fab",
        "regio geometria\n\n@ publica\nfunctio unum() → numerus {\n    redde 1\n}\n",
    );
    let app = test_temp_dir("default-import-library-app");
    write(
        app.path(),
        "main.fab",
        "import from \"probe:geo\"\n\nmain {\n    print geometria.unum()\n}\n",
    );
    let config = Config::default().with_library_home(home.path().to_path_buf());
    let result = analyze_package(
        &config,
        &app.path().join("main.fab"),
        &test_analysis_seams(),
    );
    let diagnostics = match result {
        Ok(program) => program.diagnostics,
        Err(diagnostics) => diagnostics,
    };
    let errors = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.is_error())
        .map(error_facts)
        .collect::<Vec<_>>();
    assert_eq!(errors, Vec::<ErrorFacts>::new());
}

#[test]
fn library_internal_default_import_binds_declared_module_name() {
    // One library module default-imports a sibling that declares `regio`.
    let home = test_temp_dir("default-import-library-inner-home");
    write(
        home.path(),
        "probe/faber.toml",
        "[package]\nname = \"probe\"\n[reader]\nlocale = \"la\"\n[library]\nprovider = \"probe\"\n[paths]\nsource = \"src\"\n[build]\nkind = \"lib\"\ntargets = [\"rust\"]\n",
    );
    write(
        home.path(),
        "probe/src/geo.fab",
        "regio geometria\n\n@ publica\nfunctio unum() → numerus {\n    redde 1\n}\n",
    );
    write(
        home.path(),
        "probe/src/util.fab",
        "importa ex \"probe:geo\"\n\n@ publica\nfunctio duo() → numerus {\n    redde geometria.unum() + 1\n}\n",
    );
    let app = test_temp_dir("default-import-library-inner-app");
    write(
        app.path(),
        "main.fab",
        "import from \"probe:util\"\n\nmain {\n    print util.duo()\n}\n",
    );
    let config = Config::default().with_library_home(home.path().to_path_buf());
    let result = analyze_package(
        &config,
        &app.path().join("main.fab"),
        &test_analysis_seams(),
    );
    let diagnostics = match result {
        Ok(program) => program.diagnostics,
        Err(diagnostics) => diagnostics,
    };
    let errors = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.is_error())
        .map(error_facts)
        .collect::<Vec<_>>();
    assert_eq!(errors, Vec::<ErrorFacts>::new());
}
