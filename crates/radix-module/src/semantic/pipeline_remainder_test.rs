//! Driver/codegen remainders that cannot live in `radix-semantic`.

use crate::Compiler;
use crate::codegen::Target;
use crate::driver::{AnalyzeRequest, Config, Session, analyze_source};

#[cfg(feature = "hir-rust")]
#[test]
fn lege_corpus_still_checks() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/lege/lege.fab");
    let source = std::fs::read_to_string(&path).expect("lege corpus fixture");
    let config = Config::default();
    let result = Compiler::new(config).compile_str("lege.fab", &source);
    assert!(result.success(), "lege.fab must still compile: {:?}", result.diagnostics);
}

#[test]
fn th_th_sample_still_checks_without_locale002_advisories() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus-product/reader-locale/th-TH/src/main.fab");
    let source = std::fs::read_to_string(&path).expect("th-TH example");
    let pack_path = crate::locale::workspace_locale_pack_path("th-TH");
    let pack = crate::locale::LocalePack::from_toml_path(&pack_path).expect("installed th-TH pack should load");
    let result =
        crate::services::check_diagnostics_for_source_with_locale_pack("main.fab", &source, false, Some(&pack), None);
    assert!(result.success, "th-TH example must still check: {:?}", result.diagnostics);
    let locale002 = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some("LOCALE002"))
        .count();
    // `ผ่าน` is a valid identifier that only edit distance ties to the keyword
    // `อ่าน`; with no error at its span (D-2) it draws no advisory.
    assert_eq!(
        locale002, 0,
        "th-TH example must draw no LOCALE002 advisory on valid code: {:?}",
        result.diagnostics
    );
    // The accent/width match (phase 1) stays always on: the same keyword
    // without its tone mark folds to the keyword and is still suggested.
    assert_eq!(
        pack.keyword_suggestion("อาน").as_deref(),
        Some("อ่าน"),
        "phase-1 accent match must still fire"
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
fn top_level_const_vacua_rejects_as_top_level_binding() {
    // Used to prove: a top-level const `vacua` with a declared element type
    // must not emit a static slice, and instead fails closed with CODEGEN001.
    // D5.7 retired top-level `fixum`/`varia` outright (SEM062), which now
    // fires before codegen ever sees the vacua initializer. `vacua` is also
    // not part of the `generis` static fold evaluator's compile-time-constant
    // surface (D5.9), so there is no static equivalent to redirect this
    // fixture to; assert the SEM062 rejection instead.
    let result = Compiler::new(Config::default().with_target(Target::HirRust))
        .compile_str("vacua.fab", "fixum lista<numerus> name ← vacua");
    assert!(
        !result.success(),
        "top-level fixum vacua must fail closed, got: {:?}",
        result.diagnostics
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diag| diag.is_error() && diag.issue() == Some("top_level_binding")),
        "expected top_level_binding, got: {:?}",
        result.diagnostics
    );
}
