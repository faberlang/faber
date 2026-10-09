//! Declaration-only `@ nondum` functions at the code-generation boundary.
//!
//! A free function declared `@ nondum` without a body is part of the module's
//! interface and nothing else: semantic analysis refuses every use, so no
//! backend has anything to emit for it. Backends and MIR lowering therefore
//! read the module through [`HirModule::without_unavailable_declarations`]
//! (or test one item with [`HirItem::is_unavailable_declaration`]) instead of
//! each re-deriving the rule. Source re-emission (`radix-hir-faber`) and
//! interface extraction read the full module, because they must keep the
//! declaration.

use crate::{HirItem, HirItemKind, HirModule};
use radix_lexer::Interner;
use std::borrow::Cow;

/// Annotation family spelling of the availability marker.
const NONDUM_FAMILY: &str = "nondum";

impl HirItem {
    /// Whether this item is a free function with no body that carries an
    /// `@ nondum` annotation.
    #[must_use]
    pub fn is_unavailable_declaration(&self, interner: &Interner) -> bool {
        let HirItemKind::Function(function) = &self.kind else {
            return false;
        };
        function.body.is_none()
            && self
                .annotations
                .iter()
                .any(|annotation| interner.resolve(annotation.family) == NONDUM_FAMILY)
    }
}

impl HirModule {
    /// This module without its declaration-only `@ nondum` functions.
    ///
    /// Borrows when the module has none, so the common case costs one scan.
    #[must_use]
    pub fn without_unavailable_declarations(&self, interner: &Interner) -> Cow<'_, Self> {
        if !self
            .items
            .iter()
            .any(|item| item.is_unavailable_declaration(interner))
        {
            return Cow::Borrowed(self);
        }
        let mut pruned = self.clone();
        pruned.drop_unavailable_declarations(interner);
        Cow::Owned(pruned)
    }

    /// Remove the declaration-only `@ nondum` functions in place.
    pub fn drop_unavailable_declarations(&mut self, interner: &Interner) {
        self.items
            .retain(|item| !item.is_unavailable_declaration(interner));
    }
}
