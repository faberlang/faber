//! Genus member visibility at access sites (DECISIONS D5.2, D5.3, D5.4, D9.2).
//!
//! Members are public by default. `@ privata` limits a member to its type:
//! only the type's own methods (and field defaults) may read, write, or call
//! it, from any instance of the type. `@ interna` limits a member to code in
//! the declaring package; an imported genus reached through a cross-package
//! binding exposes no `@ interna` member.
//!
//! Construction literals never reach these checks: any construction literal,
//! from any file, may set a private field, and `Genus { … } ex p` copies it
//! (D5.4, D15.2). A write through a union overlap reaches every member's
//! field, so every member must declare the field with the same visibility.

#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for typechecking
use super::*;
use crate::passes::visibility_annotation::{hir_member_visibility, member_visibility_spelling};
use radix_syntax::MemberVisibility;

/// How an access site uses a member.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MemberAccess {
    Read,
    Write,
    Call,
}

const ISSUE_UNION_MEMBER_VISIBILITY_DIFFERS: &str = "union_member_visibility_differs";

fn denied_issue(visibility: MemberVisibility, access: MemberAccess) -> &'static str {
    match (visibility, access) {
        (MemberVisibility::Interna, MemberAccess::Read) => "member_interna_read",
        (MemberVisibility::Interna, MemberAccess::Write) => "member_interna_write",
        (MemberVisibility::Interna, MemberAccess::Call) => "member_interna_call",
        (_, MemberAccess::Read) => "member_private_read",
        (_, MemberAccess::Write) => "member_private_write",
        (_, MemberAccess::Call) => "member_private_call",
    }
}

impl TypeChecker<'_> {
    /// Visibility of a local `genus` member from its lowered annotations.
    pub(super) fn hir_member_visibility(
        &self,
        annotations: &[radix_hir::HirAnnotation],
    ) -> MemberVisibility {
        self.interner.map_or(MemberVisibility::Publica, |interner| {
            hir_member_visibility(annotations, interner)
        })
    }

    /// Whether code here may reach a member of `owner` with `visibility`.
    fn member_reachable(&self, owner: DefId, visibility: MemberVisibility) -> bool {
        match visibility {
            MemberVisibility::Publica => true,
            MemberVisibility::Privata => self.current_member_owner == Some(owner),
            MemberVisibility::Interna => !self.cross_package_structs.contains(&owner),
        }
    }

    fn nominal_display_name(&self, def_id: DefId) -> String {
        self.resolver
            .get_symbol(def_id)
            .and_then(|symbol| {
                self.interner
                    .map(|interner| interner.resolve(symbol.name).to_owned())
            })
            .unwrap_or_default()
    }

    /// Reject an access the member's visibility does not admit here (D5.3).
    pub(super) fn check_member_visibility(
        &mut self,
        owner: DefId,
        name: Symbol,
        visibility: MemberVisibility,
        access: MemberAccess,
        span: radix_lexer::Span,
    ) {
        if self.member_reachable(owner, visibility) {
            return;
        }
        let error = SemanticError::structured(SemanticErrorKind::MemberInaccessible, span)
            .with_arg("issue", denied_issue(visibility, access))
            .with_arg("member", self.member_display_name(name))
            .with_arg("owner", self.nominal_display_name(owner));
        self.errors.push(error);
    }

    /// Visibility of field `name` on the genus behind `ty`, with that genus.
    pub(super) fn struct_field_visibility(
        &self,
        ty: TypeId,
        name: Symbol,
    ) -> Option<(DefId, MemberVisibility)> {
        let struct_def = self.struct_def_from_type(ty)?;
        let field = self.structs.get(&struct_def)?.fields.get(&name)?;
        Some((struct_def, field.visibility))
    }

    /// Check a field access against the member visibility of every genus it
    /// reaches: the receiver itself, or each member of a union receiver.
    pub(super) fn check_field_visibility(
        &mut self,
        object_ty: TypeId,
        name: Symbol,
        access: MemberAccess,
        span: radix_lexer::Span,
    ) {
        let receivers = self
            .union_members_of(object_ty)
            .unwrap_or_else(|| vec![object_ty]);
        for receiver in receivers {
            if let Some((owner, visibility)) = self.struct_field_visibility(receiver, name) {
                self.check_member_visibility(owner, name, visibility, access, span);
            }
        }
    }

    /// Check a method call against the member visibility of every genus it
    /// dispatches to: the receiver itself, or each member of a union receiver.
    pub(super) fn check_method_visibility(
        &mut self,
        receiver_ty: TypeId,
        name: Symbol,
        span: radix_lexer::Span,
    ) {
        let receivers = self
            .union_members_of(receiver_ty)
            .unwrap_or_else(|| vec![receiver_ty]);
        for receiver in receivers {
            let Some(owner) = self.struct_def_from_type(receiver) else {
                continue;
            };
            let Some(info) = self.structs.get(&owner) else {
                continue;
            };
            if !info.methods.contains_key(&name) {
                continue;
            }
            let visibility = info
                .method_visibility
                .get(&name)
                .copied()
                .unwrap_or_default();
            self.check_member_visibility(owner, name, visibility, MemberAccess::Call, span);
        }
    }

    /// A write through a union overlap needs one visibility on every member
    /// that declares the field (D9.2); the diagnostic names each member's.
    pub(super) fn check_union_write_visibility(
        &mut self,
        members: &[TypeId],
        name: Symbol,
        span: radix_lexer::Span,
    ) {
        let declared: Vec<(TypeId, MemberVisibility)> = members
            .iter()
            .filter_map(|member| {
                self.struct_field_visibility(*member, name)
                    .map(|(_, visibility)| (*member, visibility))
            })
            .collect();
        let Some(&(_, first)) = declared.first() else {
            return;
        };
        if declared.iter().all(|(_, visibility)| *visibility == first) {
            return;
        }
        let detail = declared
            .iter()
            .map(|(member, visibility)| {
                format!(
                    "`{}` in {}",
                    member_visibility_spelling(*visibility),
                    self.display_union_constituent(*member)
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        let error = SemanticError::structured(SemanticErrorKind::MemberInaccessible, span)
            .with_arg("issue", ISSUE_UNION_MEMBER_VISIBILITY_DIFFERS)
            .with_arg("member", self.member_display_name(name))
            .with_arg("detail", detail);
        self.errors.push(error);
    }
}
