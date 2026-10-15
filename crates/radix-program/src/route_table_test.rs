//! A3/U4: the program-wide `@ ad` route table across modules and packages.

use std::fs;
use std::path::Path;

use radix_module::diagnostics::Diagnostic;
use radix_module::driver::Config;

use crate::analyze::{AnalyzedProgram, analyze_package};
use crate::test_support::{test_analysis_seams, test_temp_dir};

const SERVE_ORDERS: &str = "+++\nlocale = \"en\"\n+++\n\n@ call 'orders:list'\nfn list_orders(int limit) → int {\n    return limit\n}\n\n@ public\nfn marker() → int {\n    return 1\n}\n";

/// Library package `shop` (provider `shop`) with `modules`, and consumer
/// package `app` whose `main.fab` is `main_source`, lock-pinned to `shop`.
fn write_two_packages(dir: &Path, modules: &[(&str, &str)], main_source: &str) {
    let library = dir.join("libhome").join("shop");
    fs::create_dir_all(library.join("src")).expect("create library src");
    fs::write(
        library.join("faber.toml"),
        "[package]\nname = \"shop\"\nversion = \"1.0.0\"\nedition = \"2026\"\n\n[library]\nprovider = \"shop\"\n\n[paths]\nsource = \"src\"\n\n[build]\nkind = \"lib\"\n\n[locale]\nlocale = \"en\"\n",
    )
    .expect("write library manifest");
    for (name, body) in modules {
        fs::write(library.join("src").join(format!("{name}.fab")), body)
            .expect("write library module");
    }
    let package = dir.join("app");
    fs::create_dir_all(package.join("src")).expect("create package src");
    fs::write(
        package.join("faber.toml"),
        "[package]\nname = \"app\"\nversion = \"1.0.0\"\nedition = \"2026\"\n\n[paths]\nsource = \"src\"\nentry = \"main.fab\"\n\n[dependencies]\nshop = \"1.0.0\"\n\n[build]\nkind = \"bin\"\n\n[locale]\nlocale = \"en\"\n",
    )
    .expect("write package manifest");
    let lib = library.display();
    fs::write(
        package.join("faber.lock"),
        format!(
            "[[package]]\nname = \"shop\"\nversion = \"1.0.0\"\nsource = \"path:{lib}\"\npackage_root = \"{lib}\"\nkind = \"source\"\ntarget_language = \"\"\ntarget_triple = \"\"\ntarget_manifest = \"\"\ninterface_root = \"{lib}/src\"\nartifact = \"\"\ncrate = \"\"\nrustc = \"\"\n"
        ),
    )
    .expect("write lock");
    fs::write(package.join("src").join("main.fab"), main_source).expect("write main");
}

fn analyze(dir: &Path) -> Result<AnalyzedProgram, Vec<Diagnostic>> {
    let config = Config::default().with_library_home(dir.join("libhome"));
    analyze_package(&config, &dir.join("app"), &test_analysis_seams())
}

fn duplicate(diagnostics: &[Diagnostic]) -> &Diagnostic {
    diagnostics
        .iter()
        .find(|diagnostic| diagnostic.issue() == Some("ad_route_duplicate"))
        .unwrap_or_else(|| panic!("expected ad_route_duplicate, got {diagnostics:?}"))
}

fn arg<'a>(diagnostic: &'a Diagnostic, name: &str) -> &'a str {
    diagnostic
        .args
        .iter()
        .find(|arg| arg.name == name)
        .map(|arg| arg.value.as_str())
        .unwrap_or_else(|| panic!("missing arg {name}: {:?}", diagnostic.args))
}

const CALLER: &str = "+++\nlocale = \"en\"\n+++\n\nimport from \"shop:serve\" serve\n\nmain {\n    print serve.marker()\n    let int n ← call 'orders:list' (3) ↦ int\n    print n\n}\n";

#[test]
fn library_handler_joins_the_program_table() {
    let dir = test_temp_dir("route-table-library");
    write_two_packages(&dir, &[("serve", SERVE_ORDERS)], CALLER);
    let program = analyze(&dir).unwrap_or_else(|diagnostics| panic!("{diagnostics:?}"));
    let entry = program
        .routes
        .get("orders:list")
        .expect("library route served");
    assert_eq!(entry.export.handler, "list_orders");
    assert_eq!(entry.origin.package.as_deref(), Some("shop"));
    assert!(entry.origin.path.ends_with("serve.fab"));
    assert_eq!(program.routes.len(), 1);
}

#[test]
fn local_handler_is_in_file_interface_and_table() {
    let dir = test_temp_dir("route-table-local");
    write_two_packages(
        &dir,
        &[],
        "+++\nlocale = \"en\"\n+++\n\n@ call 'clock:now'\nfn now() → int {\n    return 1\n}\n\nmain {\n    let int n ← call 'clock:now' ↦ int\n    print n\n}\n",
    );
    let program = analyze(&dir).unwrap_or_else(|diagnostics| panic!("{diagnostics:?}"));
    let entry = program.entry_node().expect("entry node");
    assert_eq!(
        entry.file_interface.routes.keys().collect::<Vec<_>>(),
        ["clock:now"]
    );
    let served = program.routes.get("clock:now").expect("local route served");
    assert_eq!(served.origin.package.as_deref(), Some("app"));
}

#[test]
fn duplicate_route_across_packages_names_both_definitions() {
    let dir = test_temp_dir("route-table-duplicate-packages");
    let main = format!(
        "{}\n@ call 'orders:list'\nfn mine(int limit) → int {{\n    return limit\n}}\n",
        CALLER
    );
    write_two_packages(&dir, &[("serve", SERVE_ORDERS)], &main);
    let diagnostics = match analyze(&dir) {
        Ok(_) => panic!("duplicate route across packages must fail"),
        Err(diagnostics) => diagnostics,
    };
    let diagnostic = duplicate(&diagnostics);
    assert_eq!(diagnostic.code, Some("SEM005"));
    assert_eq!(arg(diagnostic, "route"), "orders:list");
    let both = format!("{} {}", arg(diagnostic, "first"), arg(diagnostic, "second"));
    assert!(
        both.contains("`app`") && both.contains("main.fab"),
        "{both}"
    );
    assert!(
        both.contains("`shop`") && both.contains("serve.fab"),
        "{both}"
    );
}

#[test]
fn duplicate_route_across_local_modules_is_rejected() {
    let dir = test_temp_dir("route-table-duplicate-modules");
    write_two_packages(
        &dir,
        &[],
        "+++\nlocale = \"en\"\n+++\n\nimport from \"./other\" other\n\n@ call 'x:y'\nfn first() {\n}\n\nmain {\n    print other.marker()\n}\n",
    );
    fs::write(
        dir.join("app").join("src").join("other.fab"),
        "+++\nlocale = \"en\"\n+++\n\n@ call 'x:y'\nfn second() {\n}\n\n@ public\nfn marker() → int {\n    return 1\n}\n",
    )
    .expect("write sibling");
    let diagnostics = match analyze(&dir) {
        Ok(_) => panic!("duplicate route across modules must fail"),
        Err(diagnostics) => diagnostics,
    };
    assert_eq!(arg(duplicate(&diagnostics), "route"), "x:y");
}

// ---- A5/U10: imported handlers type the caller's conversation (D6.11) ----

fn issue_present(diagnostics: &[Diagnostic], issue: &str) -> bool {
    diagnostics
        .iter()
        .any(|diagnostic| diagnostic.issue() == Some(issue))
}

fn caller_body(body: &str) -> String {
    format!(
        "+++\nlocale = \"en\"\n+++\n\nimport from \"shop:serve\" serve\n\nmain {{\n    print serve.marker()\n{body}\n}}\n"
    )
}

#[test]
fn imported_handler_types_the_opener() {
    let dir = test_temp_dir("route-typing-imported-opener");
    write_two_packages(
        &dir,
        &[("serve", SERVE_ORDERS)],
        &caller_body("    let int n ← call 'orders:list' (\"three\") ↦ int\n    print n"),
    );
    let Err(diagnostics) = analyze(&dir) else {
        panic!("opener mismatch must fail");
    };
    assert!(
        issue_present(&diagnostics, "ad_route_opener_type_mismatch"),
        "{diagnostics:?}"
    );
}

#[test]
fn imported_handler_types_the_reply() {
    let dir = test_temp_dir("route-typing-imported-reply");
    write_two_packages(
        &dir,
        &[("serve", SERVE_ORDERS)],
        &caller_body("    let string n ← call 'orders:list' (3) ↦ string\n    print n"),
    );
    let Err(diagnostics) = analyze(&dir) else {
        panic!("reply mismatch must fail");
    };
    assert!(
        issue_present(&diagnostics, "sermo_reply_type_mismatch"),
        "{diagnostics:?}"
    );
}
