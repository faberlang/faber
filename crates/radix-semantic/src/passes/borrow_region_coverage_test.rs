//! BRI-U5 totality receipt: region facts cover every HIR shape the real
//! sources contain.
//!
//! The assignment's measured surface is `corpus/`, `corpus-product/` and
//! `lib/norma/src/`. For every source unit that lowers to typed HIR the
//! region engine (`borrow_region::analyze_with_interner`) must produce
//! facts, and every statement site must carry a CFG location — otherwise
//! `borrow.rs` silently falls back to the block-scope approximation for the
//! marked-value decisions that route through regions. A construct the engine
//! cannot cover must instead fail closed with a named diagnostic; this
//! harness proves neither the gap nor the silence survives on the measured
//! surface.
//!
//! Files that never reach typed HIR (frontmatter-only doc artifacts, and
//! negative exempla whose parse/resolution errors stop the pipeline) carry no
//! borrow decisions and are counted separately; they are not part of the
//! measured surface.

use super::borrow_region;
use crate::locale::LocalePack;
use crate::test_support::analyze_source_with_config;
use radix_hir::visit::{self, HirVisitor};
use radix_hir::{HirModule, HirStatement, HirStatementKind};
use std::path::{Path, PathBuf};

/// Classification of one measured source unit.
enum Coverage {
    /// Typed HIR plus region facts: the engine owns the unit's decisions.
    Facts,
    /// No typed HIR: the unit carries no borrow decisions at all.
    NoHir,
    /// The engine rejected the unit's shapes: facts unavailable.
    Unavailable(String),
    /// Statements whose HIR identity has no CFG location: borrows bound
    /// there fall back to the block-scope approximation.
    Unlocated(Vec<String>),
}

/// The three measured source roots, in report order.
fn source_roots() -> Vec<(&'static str, PathBuf)> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    vec![
        ("corpus", manifest.join("../../corpus")),
        ("corpus-product", manifest.join("../../corpus-product")),
        ("lib/norma", manifest.join("../../lib/norma/src")),
    ]
}

fn walk_fab(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_fab(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "fab") {
            out.push(path);
        }
    }
}

/// Drop the leading `+++ … +++` TOML frontmatter block.
fn strip_frontmatter(raw: &str) -> &str {
    if let Some(rest) = raw.strip_prefix("+++")
        && let Some(end) = rest.find("+++")
    {
        return &rest[end + 3..];
    }
    raw
}

/// The file's declared reader locale (`locale = "…"` in frontmatter); the
/// product default is `en`.
fn frontmatter_locale(raw: &str) -> String {
    let Some(rest) = raw.strip_prefix("+++") else {
        return "en".to_string();
    };
    let Some(end) = rest.find("+++") else {
        return "en".to_string();
    };
    for line in rest[..end].lines() {
        let Some(value) = line.trim().strip_prefix("locale") else {
            continue;
        };
        let value = value.trim().strip_prefix('=').unwrap_or(value).trim();
        return value.trim_matches('"').to_string();
    }
    "en".to_string()
}

fn reader_pack(id: &str) -> Option<LocalePack> {
    LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path(id)).ok()
}

/// Statement-site census of one module: every statement's HIR identity plus
/// the construct name, in visitor order.
struct StatementSites {
    sites: Vec<(radix_hir::HirId, &'static str)>,
}

impl StatementSites {
    fn collect(hir: &HirModule) -> Self {
        let mut collector = StatementSites { sites: Vec::new() };
        collector.visit_program(hir);
        collector
    }
}

impl HirVisitor for StatementSites {
    fn visit_stmt(&mut self, statement: &HirStatement) {
        let construct = match &statement.kind {
            HirStatementKind::Local(_) => "local",
            HirStatementKind::Expr(_) => "expr",
            HirStatementKind::Redde(_) => "redde",
            HirStatementKind::ReddeFilum(_) => "redde-filum",
            HirStatementKind::Rumpe => "rumpe",
            HirStatementKind::Perge => "perge",
            HirStatementKind::Tacet => "tacet",
            HirStatementKind::IncDec(_) => "inc-dec",
            HirStatementKind::Custodi(_) => "custodi",
        };
        self.sites.push((statement.id, construct));
        visit::walk_stmt(self, statement);
    }
}

fn classify(path: &Path, locale: &str) -> Coverage {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Coverage::NoHir;
    };
    let source = strip_frontmatter(&raw);
    let Some(pack) = reader_pack(locale) else {
        return Coverage::NoHir;
    };
    let mut config = crate::PassConfig::for_target(radix_target::Target::HirRust);
    config.locale_pack = Some(pack);
    let result = analyze_source_with_config(source, config);

    let Some(hir) = result.hir.as_ref() else {
        return Coverage::NoHir;
    };
    let Some(facts) = result.access_facts.as_ref() else {
        return Coverage::NoHir;
    };
    match borrow_region::analyze_with_interner(hir, &result.types, facts, &result.interner) {
        Ok(regions) => {
            let unlocated: Vec<String> = StatementSites::collect(hir)
                .sites
                .into_iter()
                .filter(|(id, _)| regions.cfg().node_for_hir_id(*id).is_none())
                .map(|(id, construct)| format!("{construct}@HirId({})", id.0))
                .collect();
            if unlocated.is_empty() {
                Coverage::Facts
            } else {
                Coverage::Unlocated(unlocated)
            }
        }
        Err(error) => Coverage::Unavailable(error.to_string()),
    }
}

/// The measured report: per root, how many units produced facts, how many
/// never reached HIR, and the exact shapes that left facts unavailable.
#[test]
fn region_facts_cover_corpus_product_and_norma() {
    let mut report = String::new();
    let mut failures = 0usize;
    for (label, root) in source_roots() {
        let mut sources = Vec::new();
        walk_fab(&root, &mut sources);
        sources.sort();
        assert!(
            !sources.is_empty(),
            "measured source root {label} is missing at {}",
            root.display()
        );
        let mut facts = 0usize;
        let mut no_hir = 0usize;
        let mut unavailable: Vec<(String, String)> = Vec::new();
        let mut unlocated: Vec<(String, Vec<String>)> = Vec::new();
        for path in &sources {
            let Ok(raw) = std::fs::read_to_string(path) else {
                no_hir += 1;
                continue;
            };
            let locale = frontmatter_locale(&raw);
            match classify(path, &locale) {
                Coverage::Facts => facts += 1,
                Coverage::NoHir => no_hir += 1,
                Coverage::Unavailable(reason) => {
                    unavailable.push((path.display().to_string(), reason));
                }
                Coverage::Unlocated(sites) => {
                    unlocated.push((path.display().to_string(), sites));
                }
            }
        }
        failures += unavailable.len() + unlocated.len();
        report.push_str(&format!(
            "{label}: {} sources, {facts} with region facts, {no_hir} without typed HIR, {} with unavailable shapes, {} with unlocated statements\n",
            sources.len(),
            unavailable.len(),
            unlocated.len()
        ));
        for (path, reason) in unavailable {
            report.push_str(&format!("  unavailable: {path}: {reason}\n"));
        }
        for (path, sites) in unlocated {
            report.push_str(&format!("  unlocated: {path}: {sites:?}\n"));
        }
    }
    assert!(
        failures == 0,
        "region facts must cover every measured shape (or fail closed by name):\n{report}"
    );
    // The receipt stays observable on success: the measurement is the
    // deliverable, not only the failure detail.
    print!("{report}");
}

// ---------------------------------------------------------------------------
// Fail-closed identity
// ---------------------------------------------------------------------------

/// An engine gap maps to the named SEM008 `region_facts_not_built`
/// diagnostic, never to silence: identity is the issue name plus the
/// construct arg, asserted — never message prose.
#[test]
fn an_unsupported_shape_fails_closed_with_the_named_diagnostic() {
    let span = radix_lexer::Span::new(4, 8);
    let error = borrow_region::fail_closed_diagnostic(&borrow_region::RegionError::Cfg(
        super::borrow_region_cfg::CfgError::UnsupportedShape {
            construct: "widget",
            span,
        },
    ))
    .expect("unsupported shape must fail closed");
    assert_eq!(
        error.kind,
        radix_diagnostics::SemanticErrorKind::LoweringError
    );
    assert!(error.span == span);
    let issue = error
        .args
        .iter()
        .find(|arg| arg.name == "issue")
        .expect("issue arg");
    assert_eq!(issue.value, borrow_region::ISSUE_REGION_FACTS_NOT_BUILT);
    let construct = error
        .args
        .iter()
        .find(|arg| arg.name == "construct")
        .expect("construct arg");
    assert_eq!(construct.value, "widget");
    assert_eq!(
        crate::catalog_semantic::semantic_spec(error.kind).code,
        "SEM008"
    );
}

/// A missing borrow-start location is an engine gap like any other: it
/// fails closed under the same named issue.
#[test]
fn a_missing_location_fails_closed_with_the_named_diagnostic() {
    let error =
        borrow_region::fail_closed_diagnostic(&borrow_region::RegionError::MissingLocation {
            hir_id: radix_hir::HirId(7),
            span: radix_lexer::Span::new(1, 2),
        })
        .expect("missing location must fail closed");
    let issue = error
        .args
        .iter()
        .find(|arg| arg.name == "issue")
        .expect("issue arg");
    assert_eq!(issue.value, borrow_region::ISSUE_REGION_FACTS_NOT_BUILT);
}

/// A transfer outside its scope is the resolver's own named rejection
/// (SEM030/SEM031): the region engine adds no duplicate diagnostic.
#[test]
fn a_transfer_outside_its_scope_adds_no_duplicate_diagnostic() {
    for transfer in ["rumpe", "perge"] {
        let diagnostic = borrow_region::fail_closed_diagnostic(&borrow_region::RegionError::Cfg(
            super::borrow_region_cfg::CfgError::TransferOutsideLoop {
                transfer,
                span: radix_lexer::Span::new(1, 2),
            },
        ));
        assert!(
            diagnostic.is_none(),
            "{transfer} must stay with the resolver"
        );
    }
}

/// Every source-reachable shape the CFG builder refuses as an out-of-loop
/// transfer is already the resolver's own named rejection: at the top level
/// of a function and inside a closure body — even when the closure is created
/// inside a loop, since the resolver's scope chain and the builder's
/// components both stop at the function boundary. (Lane bodies are the third
/// component kind, but the Faber surface only admits expression-bodied
/// lanes, so no statement-form transfer can reach one.) Each case reports
/// SEM030/SEM031 and never the region engine's silent SEM008 absence: facts
/// stay unavailable, but the named rejection the engine leans on is present.
#[test]
fn every_out_of_loop_transfer_refusal_is_the_resolvers_named_rejection() {
    use crate::test_support::{analyze_source, issue};

    let cases = [
        (
            "top-level rumpe",
            "incipit {\n    rumpe\n}\n",
            "SEM030",
            "break_outside_breakable",
        ),
        (
            "top-level perge",
            "incipit {\n    perge\n}\n",
            "SEM031",
            "continue_outside_loop",
        ),
        (
            "rumpe inside a closure created in a loop",
            "incipit {\n    dum verum {\n        sit _ f ← () ∴ capta {\n            rumpe\n        }\n        rumpe\n    }\n}\n",
            "SEM030",
            "break_outside_breakable",
        ),
        (
            "perge inside a closure created in a loop",
            "incipit {\n    dum verum {\n        sit _ f ← (numerus a) ∴ fac {\n            perge\n            redde a\n        }\n        nota f(1)\n        rumpe\n    }\n}\n",
            "SEM031",
            "continue_outside_loop",
        ),
        (
            "perge inside a closure at function top level",
            "incipit {\n    sit _ f ← (numerus a) ∴ fac {\n        perge\n        redde a\n    }\n    nota f(1)\n}\n",
            "SEM031",
            "continue_outside_loop",
        ),
    ];

    for (label, source, code, issue_name) in cases {
        let result = analyze_source(source);
        assert!(
            result
                .diagnostics
                .iter()
                .any(
                    |diagnostic| crate::catalog_semantic::semantic_spec(diagnostic.kind).code
                        == code
                        && issue(diagnostic) == Some(issue_name)
                ),
            "{label} must report the resolver's {code}.{issue_name}; diagnostics: {:?}",
            result.diagnostics
        );
        assert!(
            result
                .diagnostics
                .iter()
                .all(|diagnostic| issue(diagnostic)
                    != Some(borrow_region::ISSUE_REGION_FACTS_NOT_BUILT)),
            "{label} must not surface the region engine's fail-closed issue; diagnostics: {:?}",
            result.diagnostics
        );
    }
}
