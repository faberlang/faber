//! Validate visibility annotations for the file-namespace model.
//!
//! Top-level `@ publica` / `@ interna` / `@ privata` participate in package/file
//! namespace export filtering (see `radix_module::program::export_surface`). Unmarked
//! declarations are module-private; `@ privata` on a declaration is an explicit
//! module-private marker (accepted, no warning).
//!
//! `genus` members are public by default (D5.2). `@ privata` / `@ interna` on
//! a member set its [`MemberVisibility`], which typecheck enforces at every
//! read, write, and call (D5.3). `@ publica` on a member is a redundancy
//! warning (WARN028) because members are already public.
//! `@ protecta` is not part of the file-namespace visibility model, so it is
//! rejected as reserved instead of being treated as documentation.

use crate::{SemanticError, SemanticErrorKind, WarningKind};
use radix_lexer::{Interner, Span};
use radix_syntax::{
    Annotation, AnnotationKind, ClassMemberKind, MemberVisibility, Program, Stmt, StmtKind,
};

const ISSUE_REDUNDANT_MEMBER_PUBLICA: &str = "redundant_member_publica";

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
    matches!(annotation.kind, AnnotationKind::Protecta)
        || radix_syntax::annotation_has_protecta(annotation, interner)
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
/// any. Unmarked and `@ privata` declarations have no export tier. The input
/// of the one export-visibility resolver
/// (`radix_module::program::export_surface`); a declaration mixing distinct
/// tiers is separately diagnosed by this pass.
#[must_use]
pub fn statement_export_tier(stmt: &Stmt, interner: &Interner) -> Option<VisibilityTier> {
    stmt.annotations.iter().find_map(|annotation| {
        let tier = visibility_tier(annotation, interner)?;
        (tier != VisibilityTier::Privata).then_some(tier)
    })
}

/// Whether a top-level statement carries an explicit `@ privata` marker.
///
/// Not an importability test: importable names come only from the one
/// export-visibility resolver (`radix_module::program::export_surface`), where
/// unmarked and `@ privata` are both module-private. This predicate serves the
/// type-snapshot carriers (file interfaces, library interface items), which
/// keep unmarked nominals so a public signature that names a same-module
/// unmarked type still resolves in consumers.
#[must_use]
pub fn has_private_visibility_annotation(stmt: &Stmt, interner: &Interner) -> bool {
    stmt.annotations
        .iter()
        .any(|annotation| visibility_tier(annotation, interner) == Some(VisibilityTier::Privata))
}

fn push_protecta_diagnostics(
    annotations: &[Annotation],
    interner: &Interner,
    errors: &mut Vec<SemanticError>,
) {
    for annotation in annotations {
        if is_protecta(annotation, interner) {
            errors.push(
                SemanticError::structured(SemanticErrorKind::AnnotationInvalid, annotation.span)
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
fn check_declaration_visibility(
    annotations: &[Annotation],
    interner: &Interner,
    errors: &mut Vec<SemanticError>,
) {
    push_protecta_diagnostics(annotations, interner, errors);
    let mut tiers: Vec<(VisibilityTier, Span)> = Vec::new();
    for annotation in annotations {
        if let Some(tier) = visibility_tier(annotation, interner)
            && !tiers.iter().any(|(seen, _)| *seen == tier)
        {
            tiers.push((tier, annotation.span));
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

/// Warn on redundant member `@ publica` and reject a `genus` member that
/// mixes distinct visibility tiers. `@ privata` / `@ interna` are enforced
/// by typecheck (D5.3).
fn check_genus_member_annotations(
    annotations: &[Annotation],
    interner: &Interner,
    errors: &mut Vec<SemanticError>,
    public_is_abi_entry: bool,
) {
    check_declaration_visibility(annotations, interner, errors);
    // In the opt-in compiler contract profile, explicit public methods select
    // ABI entries. Their marker carries meaning beyond ordinary visibility.
    if public_is_abi_entry {
        return;
    }
    for annotation in annotations {
        if visibility_tier(annotation, interner) == Some(VisibilityTier::Publica) {
            errors.push(
                SemanticError::structured(
                    SemanticErrorKind::Warning(WarningKind::RedundantMemberVisibility),
                    annotation.span,
                )
                .with_arg("issue", ISSUE_REDUNDANT_MEMBER_PUBLICA)
                .with_arg("annotation", tier_spelling(VisibilityTier::Publica)),
            );
        }
    }
}

fn member_visibility_for_tier(tier: VisibilityTier) -> MemberVisibility {
    match tier {
        VisibilityTier::Publica => MemberVisibility::Publica,
        VisibilityTier::Interna => MemberVisibility::Interna,
        VisibilityTier::Privata => MemberVisibility::Privata,
    }
}

/// Visibility of a `genus` member from its parsed annotations (D5.2, D5.3).
/// Unmarked members are public; the first visibility annotation wins (a
/// member mixing tiers is separately diagnosed by this pass).
#[must_use]
pub fn member_visibility(annotations: &[Annotation], interner: &Interner) -> MemberVisibility {
    annotations
        .iter()
        .find_map(|annotation| visibility_tier(annotation, interner))
        .map_or(MemberVisibility::Publica, member_visibility_for_tier)
}

/// Visibility of a `genus` member from its lowered HIR annotations. Lowering
/// normalizes every visibility spelling to the canonical family name.
#[must_use]
pub fn hir_member_visibility(
    annotations: &[radix_hir::HirAnnotation],
    interner: &Interner,
) -> MemberVisibility {
    annotations
        .iter()
        .find_map(|annotation| tier_for_name(interner.resolve(annotation.family)))
        .map_or(MemberVisibility::Publica, member_visibility_for_tier)
}

/// Canonical annotation spelling of a member visibility, for diagnostics.
#[must_use]
pub fn member_visibility_spelling(visibility: MemberVisibility) -> &'static str {
    match visibility {
        MemberVisibility::Publica => tier_spelling(VisibilityTier::Publica),
        MemberVisibility::Interna => tier_spelling(VisibilityTier::Interna),
        MemberVisibility::Privata => tier_spelling(VisibilityTier::Privata),
    }
}

fn check_stmt(stmt: &Stmt, interner: &Interner, errors: &mut Vec<SemanticError>) {
    check_declaration_visibility(&stmt.annotations, interner, errors);
    match &stmt.kind {
        StmtKind::Func(func) => check_declaration_visibility(&func.annotations, interner, errors),
        StmtKind::Class(class) => {
            let is_contract = stmt.annotations.iter().any(|annotation| {
                crate::evm_contract::is_contract_annotation(annotation, interner)
            });
            for member in &class.members {
                let public_is_abi_entry =
                    is_contract && matches!(member.kind, ClassMemberKind::Method(_));
                check_genus_member_annotations(
                    &member.annotations,
                    interner,
                    errors,
                    public_is_abi_entry,
                );
                if let ClassMemberKind::Method(method) = &member.kind {
                    check_genus_member_annotations(
                        &method.annotations,
                        interner,
                        errors,
                        public_is_abi_entry,
                    );
                }
            }
        }
        StmtKind::Interface(iface) => {
            for method in &iface.methods {
                push_protecta_diagnostics(&method.annotations, interner, errors);
            }
        }
        _ => {}
    }
}

/// Scan parsed declarations for reserved or conflicting visibility annotations.
///
/// # Errors
///
/// Returns a list of [`SemanticError`]s for reserved `@ protecta` annotations,
/// conflicting visibility tiers on one declaration or member, and redundant
/// member `@ publica` (a warning).
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
