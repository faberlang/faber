//! Coherence and fail-closed tests for the HIR v1 analyzed application contract.

use crate::analyzed_application::{
    AnalyzedApplication, AnalyzedApplicationError, AnalyzedApplicationParts, QualificationStatus,
    qualify_for_target,
};
use crate::{
    PassConfig, QualifiedDeclKind, QualifiedIdentity, QualifiedIdentityTable, ResolvedUseKind,
    SemanticError, SemanticErrorKind, analyze, analyze_resolved_uses,
};
use radix_lexer::Interner;
use radix_target::Target;

fn la_pack() -> crate::locale::LocalePack {
    crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("la"))
        .expect("load Latin locale pack")
}

fn analyze_parts(source: &str) -> (AnalyzedApplicationParts, Interner) {
    let lex = radix_lexer::lex_with_locale_pack(source, &la_pack());
    let mut parsed = radix_parser::parse(lex);
    assert!(
        parsed.success(),
        "parse should succeed: errors={:?}",
        parsed.errors
    );
    let program = parsed.program.take().expect("program");
    let config = PassConfig::base();
    let result = analyze(&program, &config, &mut parsed.interner);
    assert!(
        result.is_ok(),
        "semantic analysis should succeed: {:?}",
        result.diagnostics
    );
    let parts = AnalyzedApplicationParts {
        hir: result.hir.expect("hir"),
        types: result.types,
        resolver: result.resolver,
        annotation_contracts: result.annotation_contracts,
        libraries: radix_hir::LibraryRegistry::default(),
        radix_lanes: result.radix_lanes,
        gpu_builtins: result.gpu_builtins,
        tensor_edge_policies: result.tensor_edge_policies,
        kernel_requirements: result.kernel_requirements,
        access_facts: result.access_facts.expect("access facts"),
        region_facts: result.region_facts,
        semantic_diagnostics: result.diagnostics,
    };
    (parts, parsed.interner)
}

#[test]
fn analyzed_application_builds_coherent_snapshot() {
    let source = r"
functio add(numerus a, numerus b) → numerus {
  redde a + b
}
incipit {
  sit numerus x ← add(1, 2)
}
";
    let (parts, interner) = analyze_parts(source);
    let app = AnalyzedApplication::build(parts, &interner).expect("build application");
    assert!(app.stamp().get() > 0);
    assert!(app.hir().entry.is_some());
    assert!(
        app.function_facts().has_fact(
            app.hir()
                .items
                .iter()
                .find(|item| matches!(item.kind, radix_hir::HirItemKind::Function(_)))
                .expect("function item")
                .def_id
        )
    );
}

#[test]
fn analyzed_application_records_failable_closure() {
    // Handler required when calling failable functions from non-failable callers.
    let source = r#"
functio boom() → vacuum ⇥ textus {
  iace "nope"
}
functio caller() → vacuum ⇥ textus {
  boom()
}
incipit { }
"#;
    let (parts, interner) = analyze_parts(source);
    let app = AnalyzedApplication::build(parts, &interner).expect("build");
    let failable = app.function_facts().failable_defs();
    assert!(
        !failable.is_empty(),
        "expected failable functions from iace + call graph"
    );
}

#[test]
fn analyzed_application_records_async_flag() {
    let source = r"
@ futura
functio work() → numerus {
  redde 1
}
incipit { }
";
    let (parts, interner) = analyze_parts(source);
    let app = AnalyzedApplication::build(parts, &interner).expect("build");
    let async_count = app
        .function_facts()
        .iter()
        .filter(|(_, f)| f.is_async)
        .count();
    assert!(async_count >= 1, "expected async function fact");
}

#[test]
fn synchronous_cursor_yield_does_not_record_async_flag() {
    let source = r"
@ cursor
functio numbers() → numerus {
  cede 1
}
incipit { }
";
    let (parts, interner) = analyze_parts(source);
    let app = AnalyzedApplication::build(parts, &interner).expect("build");
    let (_, fact) = app
        .function_facts()
        .iter()
        .find(|(def_id, _)| app.hir().items.iter().any(|item| item.def_id == *def_id))
        .expect("cursor function fact");

    assert!(fact.is_generator);
    assert!(!fact.is_async);
    assert!(!fact.requires_await);
}

#[test]
fn qualify_same_base_for_rust_ts_go() {
    let source = r"
functio id(numerus x) → numerus {
  redde x
}
incipit {
  sit numerus y ← id(1)
}
";
    let (parts, interner) = analyze_parts(source);
    let app = AnalyzedApplication::build(parts, &interner).expect("build");
    let stamp = app.stamp();

    for target in [Target::HirRust, Target::HirTypeScript, Target::HirGo] {
        let q = qualify_for_target(&app, target, &[]);
        assert_eq!(q.base_stamp, stamp);
        assert_eq!(q.status, QualificationStatus::Supported);
    }
}

#[test]
fn stamp_mismatch_is_fail_closed() {
    let source = "incipit { }";
    let (parts_a, interner_a) = analyze_parts(source);
    let (parts_b, interner_b) = analyze_parts(source);
    let app_a = AnalyzedApplication::build(parts_a, &interner_a).expect("a");
    let app_b = AnalyzedApplication::build(parts_b, &interner_b).expect("b");
    let err = app_a
        .require_stamp(app_b.stamp())
        .expect_err("stamps must differ");
    assert!(matches!(
        err,
        AnalyzedApplicationError::StampMismatch { .. }
    ));
}

#[test]
fn resolved_uses_record_calls() {
    let source = r"
functio helper() → numerus {
  redde 1
}
functio main() → numerus {
  redde helper()
}
incipit {
  sit numerus z ← main()
}
";
    let (parts, interner) = analyze_parts(source);
    let app = AnalyzedApplication::build(parts, &interner).expect("build");
    assert!(
        !app.resolved_uses().is_empty(),
        "expected resolved uses for call graph"
    );
}

#[test]
fn resolved_uses_retain_local_ids_for_portable_calls_and_signature_types() {
    let source = r#"
genus Persona {
  varia textus name
}
functio identity(Persona value) → Persona {
  redde value
}
incipit {
  sit Persona result ← identity(Persona { name = "salve" })
}
"#;
    let (parts, _interner) = analyze_parts(source);
    let persona_def = parts
        .hir
        .items
        .iter()
        .find_map(|item| {
            matches!(item.kind, radix_hir::HirItemKind::Struct(_)).then_some(item.def_id)
        })
        .expect("Persona definition");
    let identity_def = parts
        .hir
        .items
        .iter()
        .find_map(|item| {
            matches!(item.kind, radix_hir::HirItemKind::Function(_)).then_some(item.def_id)
        })
        .expect("identity definition");

    let mut qualified = QualifiedIdentityTable::new();
    qualified.insert(
        persona_def,
        QualifiedIdentity {
            provider: "package".to_owned(),
            package: Some("demo".to_owned()),
            module_path: vec!["models".to_owned()],
            export_name: "Persona".to_owned(),
            kind: QualifiedDeclKind::Struct,
        },
    );
    qualified.insert(
        identity_def,
        QualifiedIdentity {
            provider: "package".to_owned(),
            package: Some("demo".to_owned()),
            module_path: vec!["models".to_owned()],
            export_name: "identity".to_owned(),
            kind: QualifiedDeclKind::Function,
        },
    );

    let uses = analyze_resolved_uses(&parts.hir, &parts.types, &qualified);
    assert!(uses.locals_used().contains(&persona_def));
    assert!(uses.locals_used().contains(&identity_def));
    assert!(
        uses.uses_of_portable("package::demo::models::Persona")
            .any(|use_site| use_site.kind == ResolvedUseKind::TypeRef)
    );
    assert!(
        uses.uses_of_portable("package::demo::models::identity")
            .any(|use_site| use_site.kind == ResolvedUseKind::Call)
    );
}

#[test]
fn function_facts_cover_all_functions() {
    let source = r"
functio a() → numerus {
  redde 1
}
functio b() → numerus {
  redde a()
}
incipit { }
";
    let (parts, interner) = analyze_parts(source);
    let app = AnalyzedApplication::build(parts, &interner).expect("build");
    for item in &app.hir().items {
        if matches!(item.kind, radix_hir::HirItemKind::Function(_)) {
            assert!(app.function_facts().has_fact(item.def_id));
        }
    }
}

// PRP-028: build() must reject parts containing hard semantic errors.
// The driver already gates on semantic_result.is_ok(), but build() is a
// public API that direct callers can bypass.
#[test]
fn build_rejects_hard_semantic_errors() {
    let source = r"
functio add(numerus a, numerus b) → numerus {
  redde a + b
}
incipit {
  sit numerus x ← add(1, 2)
}
";
    let (mut parts, interner) = analyze_parts(source);

    // Inject a hard error into semantic_diagnostics.
    parts.semantic_diagnostics.push(SemanticError::structured(
        SemanticErrorKind::VariableUndefined,
        radix_lexer::Span::default(),
    ));

    let result = AnalyzedApplication::build(parts, &interner);
    assert!(
        matches!(
            result,
            Err(AnalyzedApplicationError::HardSemanticErrors { count: 1 })
        ),
        "build() with a hard error must reject"
    );
}

// ── SCR-04 durable provenance ───────────────────────────────────────────────

#[test]
fn build_carries_durable_provenance_into_driver_fields() {
    let source = r"
functio utitur() → numerus {
    varia lista<numerus> xs ← [1, 2, 3]
    varia in lista<numerus> view ← xs
    redde view.longitudo()
}
incipit { }
";
    let (parts, interner) = analyze_parts(source);
    let app = AnalyzedApplication::build(parts, &interner).expect("build");
    let function_def = app
        .hir()
        .items
        .iter()
        .find(|item| matches!(item.kind, radix_hir::HirItemKind::Function(_)))
        .expect("function item")
        .def_id;
    let row = app
        .provenance()
        .get(function_def)
        .expect("provenance row")
        .clone();
    assert_eq!(row.function, function_def);
    assert!(
        !row.last_uses.is_empty(),
        "body borrow carries a last-use fact"
    );

    // Signature provenance and last-use facts survive the driver handoff.
    let fields = app.into_driver_fields();
    assert_eq!(fields.provenance.get(function_def), Some(&row));
}

#[test]
fn build_fails_closed_on_incoherent_provenance() {
    // Regions from a different snapshot must not satisfy a borrow-returning
    // function: build fails closed instead of freezing cross-run facts.
    let (borrowing, interner) = analyze_parts(
        r"
functio mutator() → in lista<numerus> {
    varia lista<numerus> xs ← [1, 2, 3]
    varia in lista<numerus> view ← xs
    redde view
}
incipit { }
",
    );
    let (other, _other_interner) = analyze_parts(
        r"
functio alia(numerus n) → numerus {
    redde n
}
incipit { }
",
    );
    let other_regions =
        crate::passes::borrow_region::analyze(&other.hir, &other.types, &other.access_facts).ok();

    let result = AnalyzedApplication::build(
        AnalyzedApplicationParts {
            region_facts: other_regions,
            ..borrowing
        },
        &interner,
    );
    let outcome = result.err();
    assert!(
        matches!(
            outcome,
            Some(AnalyzedApplicationError::IncoherentProvenance { .. })
        ),
        "foreign region facts must fail closed, got {outcome:?}"
    );
}

#[test]
fn unavailable_region_facts_are_an_absent_required_summary() {
    // The reducer's Unavailable path (no region facts for a borrow-returning
    // function) is what build maps to AbsentRequiredFacts; prove the mapping
    // at the reduction boundary it composes from (build now only sees absent
    // facts when the engine itself could not run).
    let (parts, _interner) = analyze_parts(
        r"
functio mutator() → in lista<numerus> {
    varia lista<numerus> xs ← [1, 2, 3]
    varia in lista<numerus> view ← xs
    redde view
}
incipit { }
",
    );
    let error = crate::passes::borrow_region::reduce(&parts.hir, &parts.types, None)
        .expect_err("absent regions are not an empty summary");
    assert!(matches!(
        error,
        crate::passes::borrow_region::ProvenanceError::Unavailable { .. }
    ));
}
