//! The FHIR artifact is the neutral form: analyzed and typechecked, but not
//! folded, not evaluated and not bound to a build target. A consumer
//! qualifies the loaded unit for its target ([`qualify_for_target`]) and gets
//! what direct compilation for that target produces.
#![cfg(all(feature = "runner", feature = "hir-fhir"))]

use crate::codegen::Target;
use crate::driver::{AnalyzeRequest, AnalyzedModule, Config, Session, analyze_source, qualify_for_target};
use crate::hir::artifact::load_fhir;
use crate::hir::serialize::from_analyzed;
use crate::hir::visit::HirVisitor;
use crate::hir::{HirExpression, HirExpressionKind, HirLiteral, HirLocal};
use crate::semantic::passes::praefixum::evaluation_order;

/// A module constant from a `praefixum` body, a float expression of literals,
/// and a literal float expression behind a constant.
const SOURCE: &str = "fixum numerus CINQUE = praefixum {\n    redde 2 + 3\n}\n\
incipit {\n    sit fractus y ← 0.1 + 0.2\n    nota y\n    nota CINQUE\n}\n";

const PYTHON_IMPORT: &str = "importa python \"math\" m\n\nincipit {\n    scribe m.pi\n}\n";

fn config(target: Target) -> Config {
    Config::default()
        .with_latin_locale()
        .with_dev_locale_root()
        .with_target(target)
}

fn analyze(target: Target, source: &str) -> Result<AnalyzedModule, Vec<crate::diagnostics::Diagnostic>> {
    analyze_source(&Session::new(config(target)), "qualify.fab", source, AnalyzeRequest::default())
}

/// The artifact round trip a consumer sees: serialize, then load.
fn loaded(unit: &AnalyzedModule) -> AnalyzedModule {
    let artifact = from_analyzed(unit, "hash", "qualify.fab");
    let bytes = postcard::to_allocvec(&artifact).expect("serialize artifact");
    load_fhir(&bytes).expect("load artifact")
}

fn pending_sites(unit: &AnalyzedModule) -> usize {
    evaluation_order(&unit.hir, &unit.types).map_or(0, |sites| sites.len())
}

fn local_init(unit: &AnalyzedModule, name: &str) -> HirExpression {
    struct Find<'a> {
        unit: &'a AnalyzedModule,
        name: &'a str,
        found: Option<HirExpression>,
    }
    impl HirVisitor for Find<'_> {
        fn visit_local(&mut self, local: &HirLocal) {
            if self.unit.interner.resolve(local.name) == self.name {
                self.found = local.init.clone();
            }
            radix_hir::visit::walk_local(self, local);
        }
    }
    let mut find = Find { unit, name, found: None };
    find.visit_program(&unit.hir);
    find.found
        .unwrap_or_else(|| panic!("local {name} not found"))
}

fn is_float_literal(expr: &HirExpression) -> bool {
    matches!(
        &expr.kind,
        HirExpressionKind::Literal(HirLiteral::Float(_) | HirLiteral::FloatText { .. })
    )
}

#[test]
fn a_loaded_artifact_is_unevaluated_and_unfolded_until_it_is_qualified() {
    let neutral = analyze(Target::HirFhir, SOURCE).expect("analysis for the artifact");
    assert_eq!(pending_sites(&neutral), 1, "the writer must not evaluate praefixum");
    assert!(
        !is_float_literal(&local_init(&neutral, "y")),
        "the writer must not fold the float expression"
    );

    let mut loaded = loaded(&neutral);
    assert_eq!(pending_sites(&loaded), 1, "the load must not evaluate praefixum");
    assert!(!is_float_literal(&local_init(&loaded, "y")));

    qualify_for_target(&mut loaded, &config(Target::HirRust), "qualify.fab", "").expect("qualify for rust");
    assert_eq!(pending_sites(&loaded), 0, "qualification evaluates praefixum");
    assert!(is_float_literal(&local_init(&loaded, "y")), "qualification folds floats");

    // Qualifying again changes nothing.
    qualify_for_target(&mut loaded, &config(Target::HirRust), "qualify.fab", "").expect("requalify");
    assert_eq!(pending_sites(&loaded), 0);
}

#[test]
fn a_qualified_loaded_unit_matches_direct_compilation() {
    let direct = analyze(Target::HirRust, SOURCE).expect("direct analysis");
    let mut via = loaded(&analyze(Target::HirFhir, SOURCE).expect("analysis for the artifact"));
    qualify_for_target(&mut via, &config(Target::HirRust), "qualify.fab", "").expect("qualify for rust");
    assert_eq!(
        format!("{:?}", local_init(&via, "y").kind),
        format!("{:?}", local_init(&direct, "y").kind)
    );
    assert_eq!(pending_sites(&via), pending_sites(&direct));
}

#[test]
fn faber_re_emission_keeps_the_authored_forms_after_load() {
    let mut via = loaded(&analyze(Target::HirFhir, SOURCE).expect("analysis for the artifact"));
    qualify_for_target(&mut via, &config(Target::HirFaber), "qualify.fab", "").expect("qualify for faber");
    assert_eq!(pending_sites(&via), 1);
    assert!(!is_float_literal(&local_init(&via, "y")));
}

#[test]
fn a_target_native_import_binds_the_build_not_the_artifact() {
    // The artifact is written for no build target: the import is not refused.
    let neutral = analyze(Target::HirFhir, PYTHON_IMPORT).expect("analysis for the artifact");

    // Python accepts its own import.
    let mut python = loaded(&neutral);
    qualify_for_target(&mut python, &config(Target::HirPython), "qualify.fab", "").expect("python accepts");

    // Any other build refuses it by name, at load, Faber re-emission included.
    for build in [
        Target::HirRust,
        Target::HirFaber,
        Target::HirTypeScript,
        Target::MirStepper,
    ] {
        let mut unit = loaded(&neutral);
        let refused = qualify_for_target(&mut unit, &config(build), "qualify.fab", "")
            .err()
            .expect("a target-native import binds the program to its target");
        let found: Vec<_> = refused
            .iter()
            .filter(|d| d.code == Some("SEM017") && d.issue() == Some("import_target_bound"))
            .collect();
        assert_eq!(found.len(), 1, "{build}: {refused:?}");
    }
}

#[test]
fn direct_analysis_for_another_target_still_refuses_the_import() {
    for build in [Target::HirRust, Target::HirFaber] {
        let diagnostics = analyze(build, PYTHON_IMPORT)
            .err()
            .expect("refused at analysis");
        assert!(
            diagnostics
                .iter()
                .any(|d| d.code == Some("SEM017") && d.issue() == Some("import_target_bound")),
            "{build}: {diagnostics:?}"
        );
    }
}
