//! Validate visibility annotations for the file-namespace model.
//!
//! Top-level `@ publica` / `@ interna` / `@ privata` participate in package/file
//! namespace export filtering (see `tool::commands::compile`). Unmarked
//! declarations are module-private; `@ privata` on a declaration is an explicit
//! module-private marker (accepted, no warning). Member-level visibility is
//! tier-aware but not import-relevant in phase 1, so member annotations carry
//! no diagnostic. `@ protecta` is not part of the file-namespace visibility
//! model, so it is rejected as reserved instead of being treated as
//! documentation.

use crate::lexer::{Interner, Span};
use crate::semantic::{SemanticError, SemanticErrorKind};
use crate::syntax::{Annotation, AnnotationKind, ClassMemberKind, Program, Stmt, StmtKind};

/// Effective visibility tier for the file-namespace model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VisibilityTier {
    /// `@ publica` — exported; importable by any consumer.
    Publica,
    /// `@ interna` — package-internal; importable by same-package consumers only.
    Interna,
    /// `@ privata` — explicit module-private marker (the default).
    Privata,
}

fn is_protecta(annotation: &Annotation, interner: &Interner) -> bool {
    matches!(annotation.kind, AnnotationKind::Protecta) || crate::syntax::annotation_has_protecta(annotation, interner)
}

fn tier_for_name(name: &str) -> Option<VisibilityTier> {
    match name {
        "publica" => Some(VisibilityTier::Publica),
        "interna" => Some(VisibilityTier::Interna),
        "privata" => Some(VisibilityTier::Privata),
        _ => None,
    }
}

/// Resolve a visibility annotation to its tier. Handles keyword forms
/// (`@ publica` under the Latin surface, `@ public` under reader aliases) and
/// the braced/statement spellings preserved by reader locales.
fn visibility_tier(annotation: &Annotation, interner: &Interner) -> Option<VisibilityTier> {
    match &annotation.kind {
        AnnotationKind::Publica => Some(VisibilityTier::Publica),
        AnnotationKind::Interna => Some(VisibilityTier::Interna),
        AnnotationKind::Privata => Some(VisibilityTier::Privata),
        AnnotationKind::Braced(braced) => tier_for_name(interner.resolve(braced.name.name)),
        AnnotationKind::Statement(stmt) => tier_for_name(interner.resolve(stmt.name.name)),
        _ => None,
    }
}

fn tier_spelling(tier: VisibilityTier) -> &'static str {
    match tier {
        VisibilityTier::Publica => "@ publica",
        VisibilityTier::Interna => "@ interna",
        VisibilityTier::Privata => "@ privata",
    }
}

/// The export tier (`@ publica` / `@ interna`) of a top-level statement, if
/// any. Unmarked and `@ privata` declarations have no export tier. Shared with
/// the export-surface builders in `tool::commands::compile` and the faber
/// package machinery; a declaration mixing distinct tiers is separately
/// diagnosed by this pass.
#[must_use]
pub fn statement_export_tier(stmt: &Stmt, interner: &Interner) -> Option<VisibilityTier> {
    stmt.annotations.iter().find_map(|annotation| {
        let tier = visibility_tier(annotation, interner)?;
        (tier != VisibilityTier::Privata).then_some(tier)
    })
}

/// Whether a top-level statement carries an explicit `@ privata` marker.
///
/// Distinct from [`statement_export_tier`]: unmarked declarations also have no
/// export tier, but only explicitly private declarations must be excluded from
/// export surfaces that keep the legacy unmarked-exported default (the package
/// machinery's surface and the import-contract file interfaces it drives).
#[must_use]
pub fn has_private_visibility_annotation(stmt: &Stmt, interner: &Interner) -> bool {
    stmt.annotations
        .iter()
        .any(|annotation| visibility_tier(annotation, interner) == Some(VisibilityTier::Privata))
}

fn push_protecta_diagnostics(annotations: &[Annotation], interner: &Interner, errors: &mut Vec<SemanticError>) {
    for annotation in annotations {
        if is_protecta(annotation, interner) {
            errors.push(
                SemanticError::structured(SemanticErrorKind::InvalidAnnotation, annotation.span)
                    .with_arg("issue", "protecta_reserved")
                    .with_arg("annotation", "@ protecta")
                    .with_arg("publica", "@ publica")
                    .with_arg("privata", "@ privata"),
            );
        }
    }
}

/// Reject `@ protecta` and report a declaration that mixes distinct
/// visibility tiers (`@ publica @ privata`). Duplicate same-tier annotations
/// are harmless and stay silent.
fn check_declaration_visibility(annotations: &[Annotation], interner: &Interner, errors: &mut Vec<SemanticError>) {
    push_protecta_diagnostics(annotations, interner, errors);
    let mut tiers: Vec<(VisibilityTier, Span)> = Vec::new();
    for annotation in annotations {
        if let Some(tier) = visibility_tier(annotation, interner) {
            if !tiers.iter().any(|(seen, _)| *seen == tier) {
                tiers.push((tier, annotation.span));
            }
        }
    }
    if tiers.len() > 1 {
        let (first, _) = tiers[0];
        let (second, span) = tiers[1];
        errors.push(
            SemanticError::structured(SemanticErrorKind::ConflictingVisibility, span)
                .with_arg("issue", "conflicting_visibility")
                .with_arg("first", tier_spelling(first))
                .with_arg("second", tier_spelling(second)),
        );
    }
}

fn check_member_annotations(annotations: &[Annotation], interner: &Interner, errors: &mut Vec<SemanticError>) {
    // Member visibility is tier-aware but not import-relevant in phase 1;
    // only the reserved `@ protecta` is rejected here.
    push_protecta_diagnostics(annotations, interner, errors);
}

fn check_stmt(stmt: &Stmt, interner: &Interner, errors: &mut Vec<SemanticError>) {
    check_declaration_visibility(&stmt.annotations, interner, errors);
    match &stmt.kind {
        StmtKind::Func(func) => check_declaration_visibility(&func.annotations, interner, errors),
        StmtKind::Class(class) => {
            for member in &class.members {
                check_member_annotations(&member.annotations, interner, errors);
                if let ClassMemberKind::Method(method) = &member.kind {
                    check_member_annotations(&method.annotations, interner, errors);
                }
            }
        }
        StmtKind::Interface(iface) => {
            for method in &iface.methods {
                check_member_annotations(&method.annotations, interner, errors);
            }
        }
        _ => {}
    }
}

/// Scan parsed declarations for reserved or conflicting visibility annotations.
///
/// # Errors
///
/// Returns a list of [`SemanticError`]s for reserved `@ protecta` annotations
/// and conflicting visibility tiers on one declaration.
pub fn check(program: &Program, interner: &Interner) -> Result<(), Vec<SemanticError>> {
    let mut errors = Vec::new();
    for stmt in &program.statements {
        check_stmt(stmt, interner, &mut errors);
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

#[cfg(test)]
#[path = "visibility_annotation_test.rs"]
mod tests;
