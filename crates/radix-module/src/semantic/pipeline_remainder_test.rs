//! Driver/codegen remainders that cannot live in `radix-semantic`.

use crate::codegen::Target;
use crate::driver::{analyze_source, AnalyzeRequest, Config, Session};
use crate::Compiler;

#[test]
fn lege_corpus_still_checks() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/lege/lege.fab");
    let source = std::fs::read_to_string(&path).expect("lege corpus fixture");
    let config =
        Config::default().with_stdlib(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stdlib"));
    let result = Compiler::new(config).compile_str("lege.fab", &source);
    assert!(result.success(), "lege.fab must still compile: {:?}", result.diagnostics);
}

#[test]
fn th_th_example_still_checks_with_locale002_advisories() {
    // Probe ancestors for the examples repo so lane packets under
    // `worktrees/<lane>/` (which lack a sibling `examples/`) still resolve.
    let example = ["reader-locale/th-TH/src/main.fab"]
        .into_iter()
        .map(std::path::Path::new)
        .find_map(|rel| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .and_then(std::path::Path::parent)
                .and_then(std::path::Path::parent)
                .and_then(|root| {
                    root.ancestors().find(|dir| {
                        !dir.components()
                            .any(|c| c.as_os_str() == std::ffi::OsStr::new("worktrees"))
                            && dir.join("examples").join(rel).is_file()
                    })
                })
        });
    let Some(examples_root) = example else {
        panic!("examples repo (reader-locale/th-TH) not found near this checkout");
    };
    let path = examples_root
        .join("examples")
        .join("reader-locale/th-TH/src/main.fab");
    let source = std::fs::read_to_string(&path).expect("th-TH example");
    let pack_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../stdlib/locale/th-TH/pack.toml");
    let pack = crate::locale::LocalePack::from_toml_path(&pack_path).expect("installed th-TH pack should load");
    let result =
        crate::services::check_diagnostics_for_source_with_locale_pack("main.fab", &source, false, Some(&pack), None);
    assert!(result.success, "th-TH example must still check: {:?}", result.diagnostics);
    let locale002 = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some("LOCALE002"))
        .count();
    assert_eq!(
        locale002, 2,
        "th-TH example must keep exactly two LOCALE002 advisories: {:?}",
        result.diagnostics
    );
}

#[test]
fn driver_analyzed_unit_carries_contract_facts() {
    let session = Session::new(Config::default().with_target(Target::HirRust));
    let unit = analyze_source(
        &session,
        "g1.fab",
        r"
functio n() → numerus {
  redde 7
}
incipit {
  fixum numerus v ← n()
}
",
        AnalyzeRequest::default(),
    )
    .expect("analyze");
    assert!(unit.analysis_stamp.get() > 0);
    assert!(!unit.function_facts.is_empty() || unit.function_facts.entry.is_some());
    let _ = unit.annotation_contracts;
    let _ = unit.resolved_uses;
    let _ = unit.qualified_identities;
    let view = unit.application_view();
    assert_eq!(view.stamp, unit.analysis_stamp);

    // SCR-04: durable provenance rides the driver fields and the borrowed
    // application view with the same stamp; F-CAP-04/05 answer from it.
    assert!(!unit.provenance.is_empty(), "every function carries a row");
    assert_eq!(view.provenance, &unit.provenance);
}

#[test]
fn annotated_const_provides_context_for_vacua() {
    let result = Compiler::new(Config::default().with_target(Target::HirRust))
        .compile_str("vacua.fab", "fixum lista<numerus> name ← vacua");
    assert!(
        !result.success(),
        "const vacua must not emit a static slice, got: {:?}",
        result.diagnostics
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diag| diag.is_error() && diag.code == Some("CODEGEN001")),
        "expected CODEGEN001, got: {:?}",
        result.diagnostics
    );
}
