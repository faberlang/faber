//! DFV2-2 orphaned-annotation admission tests (`device-fragments-v2` DFV2-2).
//!
//! An annotated `@ nucleum` function that yields zero concrete device entries
//! in its unit — a size-generic with no same-unit call site — fails closed at
//! check stage with the named `nucleum_orphaned_annotation` issue at the
//! annotation span, carrying the definition span as the secondary span. The
//! old generic zero-kernel codegen message must not be the first diagnostic.
//!
//! PKG-ADM (`device-fragments-v2` package seam): package analysis widens that
//! boundary with an explicit admission fact for an exported `@ publica`
//! size-generic kernel template. The admission is demand-only and
//! diagnostic-only; every other shape stays fail-closed.

use super::compile;
use crate::codegen::Target;
use crate::diagnostics::DiagnosticArg;
use crate::driver::{
    AnalyzeRequest, ModuleCompileRequest, PackageKernelTemplateAdmissions, analyze_source,
    analyze_source_with_kernel_template_admissions,
};
use crate::hir::HirItemKind;
use crate::semantic::passes::visibility_annotation::VisibilityTier;
use crate::test_support::test_session;
use std::collections::BTreeMap;

/// The spike's uninstantiated generic shape: annotated, public, never called.
const ORPHAN_GENERIC: &str = r#"+++
locale = "en"
+++

@ kernel
@ public
fn kadd<size M, size N>(tensor<f32, [M, N]> a, tensor<f32, [M, N]> b) → tensor<f32, [M, N]> {
    return a + b
}
"#;

/// The spike's green control: the same generic entry monomorphized by a
/// same-unit host caller.
const CONCRETE_LOCAL_GENERIC: &str = r#"+++
locale = "en"
+++

@ kernel
@ public
fn kadd<size M, size N>(tensor<f32, [M, N]> a, tensor<f32, [M, N]> b) → tensor<f32, [M, N]> {
    return a + b
}

fn main() → void {
    let _ a ← [[1.0, 2.0], [3.0, 4.0]] ↦ tensor<f32, [2, 2]>
    let _ b ← [[10.0, 20.0], [30.0, 40.0]] ↦ tensor<f32, [2, 2]>
    let _ r ← kadd(a, b)
    print "ok"
}
"#;

const ORPHAN_ISSUE: &str = "nucleum_orphaned_annotation";

fn arg_value<'a>(args: &'a [DiagnosticArg], name: &str) -> Option<&'a str> {
    args.iter()
        .find(|arg| arg.name == name)
        .map(|arg| arg.value.as_str())
}

/// Driver spans are relative to the peeled body (frontmatter removed).
fn peeled_body(source: &str) -> &str {
    let after_open = source.split_once("+++\n").expect("frontmatter open");
    after_open
        .1
        .split_once("+++\n")
        .expect("frontmatter close")
        .1
}

#[test]
fn uninstantiated_generic_annotation_fails_check_with_named_orphan_issue() {
    let result = analyze_source(
        &test_session(Target::MirWgsl),
        "orphan-generic.fab",
        ORPHAN_GENERIC,
        AnalyzeRequest::default(),
    );
    let Err(diagnostics) = result else {
        panic!("orphaned generic annotation must fail at check stage");
    };
    let first = diagnostics.first().expect("at least one diagnostic");
    assert!(
        first.is_error(),
        "orphan admission must be an error, never a warning: {first:?}"
    );
    // The orphan issue being first is itself the ordering proof: the generic
    // zero-kernel codegen diagnostic carries a different issue identity
    // (hygiene: `non_renderer_tests_do_not_assert_diagnostic_text`).
    assert_eq!(arg_value(&first.args, "issue"), Some(ORPHAN_ISSUE), "{first:?}");
}

#[test]
fn orphan_diagnostic_points_at_the_annotation_with_definition_secondary_span() {
    let result = analyze_source(
        &test_session(Target::MirWgsl),
        "orphan-generic.fab",
        ORPHAN_GENERIC,
        AnalyzeRequest::default(),
    );
    let Err(diagnostics) = result else {
        panic!("orphaned generic annotation must fail at check stage");
    };
    let first = diagnostics.first().expect("at least one diagnostic");
    let span = first.span.expect("primary span at the annotation");
    let body = peeled_body(ORPHAN_GENERIC);
    let primary = &body[span.start as usize..span.end as usize];
    assert!(primary.contains("kernel"), "primary span covers the annotation: {primary:?}");

    let definition_start = arg_value(&first.args, "definition_start").expect("definition_start");
    let start: usize = definition_start.parse().expect("definition_start numeric");
    assert!(
        body[start..].starts_with("fn kadd"),
        "secondary span points at the function definition"
    );
    assert!(start >= span.end as usize, "secondary span is distinct from the primary");
    assert!(arg_value(&first.args, "definition_end").is_some(), "definition_end present");
}

#[test]
fn concrete_local_generic_stays_green_at_check() {
    let result = analyze_source(
        &test_session(Target::MirWgsl),
        "concrete-generic.fab",
        CONCRETE_LOCAL_GENERIC,
        AnalyzeRequest::default(),
    );
    match result {
        Ok(unit) => {
            assert!(
                unit.diagnostics
                    .iter()
                    .all(|diag| arg_value(&diag.args, "issue") != Some(ORPHAN_ISSUE)),
                "no orphan diagnostic for the concrete local generic: {:?}",
                unit.diagnostics
            );
        }
        Err(diagnostics) => {
            panic!("a locally instantiated generic entry is admissible: {diagnostics:?}");
        }
    }
}

#[cfg(feature = "mir-metal")]
#[test]
fn orphan_is_not_reported_as_the_generic_zero_kernel_codegen_message_first() {
    let session = crate::driver::Session::new(
        crate::driver::Config::default()
            .with_target(Target::MirMetal)
            .with_dev_locale_root(),
    );
    let result = compile(&session, "orphan-generic.fab", ORPHAN_GENERIC, ModuleCompileRequest::default());
    assert!(!result.success(), "orphan must fail closed");
    let first = result.diagnostics.first().expect("at least one diagnostic");
    assert_eq!(arg_value(&first.args, "issue"), Some(ORPHAN_ISSUE), "{first:?}");
}

/// A `@ kernel` generic method on a class, sharing its name with a top-level
/// export — the collision shape the admission's top-level guard must not
/// silently forgive.
const METHOD_COLLISION: &str = r#"+++
locale = "en"
+++

@ kernel
@ public
fn kadd<size M, size N>(tensor<f32, [M, N]> a, tensor<f32, [M, N]> b) → tensor<f32, [M, N]> {
    return a + b
}

class Repo {
    @ kernel
    fn kadd<size M, size N>(tensor<f32, [M, N]> a) → tensor<f32, [M, N]> {
        return a
    }
}
"#;

/// PKG-ADM: the publica-tier export surface package analysis threads in for
/// the unit being analyzed.
fn package_admissions(entries: &[(&str, VisibilityTier)]) -> PackageKernelTemplateAdmissions {
    PackageKernelTemplateAdmissions::from_export_surface(
        &entries
            .iter()
            .map(|(name, tier)| ((*name).to_owned(), *tier))
            .collect::<BTreeMap<String, VisibilityTier>>(),
    )
}

fn orphan_issue_count(diagnostics: &[crate::diagnostics::Diagnostic]) -> usize {
    diagnostics
        .iter()
        .filter(|diag| arg_value(&diag.args, "issue") == Some(ORPHAN_ISSUE))
        .count()
}

/// The green half of PKG-ADM: an exported `@ publica` size-generic template
/// with no same-unit caller is demand-admitted, and the admission is
/// diagnostic-only — no synthesized caller, instantiation twin, or entry.
#[test]
fn publica_template_admission_suppresses_orphan_without_side_effects() {
    let result = analyze_source_with_kernel_template_admissions(
        &test_session(Target::MirWgsl),
        "pkg-template.fab",
        ORPHAN_GENERIC,
        AnalyzeRequest::default(),
        &package_admissions(&[("kadd", VisibilityTier::Publica)]),
    );
    let Ok(unit) = result else {
        panic!("exported publica size-generic template is demand-admissible");
    };
    assert_eq!(orphan_issue_count(&unit.diagnostics), 0, "{:?}", unit.diagnostics);
    // Demand admission only: the HIR keeps exactly the declared template.
    assert!(unit.hir.entry.is_none(), "admission mints no launch entry");
    assert_eq!(unit.hir.items.len(), 1, "admission synthesizes no items");
    let HirItemKind::Function(function) = &unit.hir.items[0].kind else {
        panic!("declared template is the only item");
    };
    assert!(function.is_kernel);
    assert_eq!(function.type_params.len(), 2, "no monomorphization twin is recorded");
}

/// Tier discipline: `@ interna` exports stay fail-closed — the admission is
/// the publica package API fact, not a name match.
#[test]
fn interna_tier_export_is_not_admitted() {
    let result = analyze_source_with_kernel_template_admissions(
        &test_session(Target::MirWgsl),
        "pkg-template-interna.fab",
        ORPHAN_GENERIC,
        AnalyzeRequest::default(),
        &package_admissions(&[("kadd", VisibilityTier::Interna)]),
    );
    let Err(diagnostics) = result else {
        panic!("@ interna exports must stay fail-closed");
    };
    assert_eq!(orphan_issue_count(&diagnostics), 1, "{diagnostics:?}");
}

/// Name-collision guard: struct methods are never package exports, so a
/// method sharing an exported name must keep its orphan diagnostic while the
/// same-named top-level template is admitted.
#[test]
fn method_sharing_an_exported_name_stays_orphan() {
    let standalone = analyze_source(
        &test_session(Target::MirWgsl),
        "method-collision.fab",
        METHOD_COLLISION,
        AnalyzeRequest::default(),
    );
    assert_eq!(
        orphan_issue_count(&collected_diagnostics(standalone)),
        2,
        "both the template and the method are orphans without package facts"
    );

    let admitted = analyze_source_with_kernel_template_admissions(
        &test_session(Target::MirWgsl),
        "method-collision.fab",
        METHOD_COLLISION,
        AnalyzeRequest::default(),
        &package_admissions(&[("kadd", VisibilityTier::Publica)]),
    );
    let Err(diagnostics) = admitted else {
        panic!("the kernel method keeps its orphan diagnostic under admission");
    };
    assert_eq!(orphan_issue_count(&diagnostics), 1, "{diagnostics:?}");
}

/// Result-shape helper: both arms carry the same diagnostic slice type.
fn collected_diagnostics(
    result: Result<crate::driver::AnalyzedModule, Vec<crate::diagnostics::Diagnostic>>,
) -> Vec<crate::diagnostics::Diagnostic> {
    match result {
        Ok(unit) => unit.diagnostics,
        Err(diagnostics) => diagnostics,
    }
}
