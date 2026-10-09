//! Declaration-only `@ nondum` functions and methods at the code-generation
//! boundary.
//!
//! A free function or `genus` method declared `@ nondum` without a body is part
//! of the module's
//! interface and nothing else: semantic analysis refuses every use, so no
//! backend has anything to emit for it. Backends and MIR lowering therefore
//! read the module through [`HirModule::without_unavailable_declarations`]
//! (or test one item with [`HirItem::is_unavailable_declaration`]) instead of
//! each re-deriving the rule. Source re-emission (`radix-hir-faber`) and
//! interface extraction read the full module, because they must keep the
//! declaration.

use crate::{HirItem, HirItemKind, HirMethod, HirModule};
use radix_lexer::Interner;
use std::borrow::Cow;

/// Annotation family spelling of the availability marker.
const NONDUM_FAMILY: &str = "nondum";

/// Whether `annotations` carry the `@ nondum` family.
fn has_nondum(annotations: &[crate::HirAnnotation], interner: &Interner) -> bool {
    annotations
        .iter()
        .any(|annotation| interner.resolve(annotation.family) == NONDUM_FAMILY)
}

impl HirItem {
    /// Whether this item is a free function with no body that carries an
    /// `@ nondum` annotation.
    #[must_use]
    pub fn is_unavailable_declaration(&self, interner: &Interner) -> bool {
        let HirItemKind::Function(function) = &self.kind else {
            return false;
        };
        function.body.is_none() && has_nondum(&self.annotations, interner)
    }
}

impl HirMethod {
    /// Whether this method has no body and carries an `@ nondum` annotation.
    #[must_use]
    pub fn is_unavailable_declaration(&self, interner: &Interner) -> bool {
        self.func.body.is_none() && has_nondum(&self.annotations, interner)
    }
}

impl HirModule {
    /// This module without its declaration-only `@ nondum` functions and
    /// methods.
    ///
    /// Borrows when the module has none, so the common case costs one scan.
    #[must_use]
    pub fn without_unavailable_declarations(&self, interner: &Interner) -> Cow<'_, Self> {
        if !self.has_unavailable_declarations(interner) {
            return Cow::Borrowed(self);
        }
        let mut pruned = self.clone();
        pruned.drop_unavailable_declarations(interner);
        Cow::Owned(pruned)
    }

    /// Remove the declaration-only `@ nondum` functions and methods in place.
    pub fn drop_unavailable_declarations(&mut self, interner: &Interner) {
        self.items
            .retain(|item| !item.is_unavailable_declaration(interner));
        for item in &mut self.items {
            if let HirItemKind::Struct(strukt) = &mut item.kind {
                strukt
                    .methods
                    .retain(|method| !method.is_unavailable_declaration(interner));
            }
        }
    }

    fn has_unavailable_declarations(&self, interner: &Interner) -> bool {
        self.items.iter().any(|item| match &item.kind {
            HirItemKind::Struct(strukt) => strukt
                .methods
                .iter()
                .any(|method| method.is_unavailable_declaration(interner)),
            _ => item.is_unavailable_declaration(interner),
        })
    }
}
