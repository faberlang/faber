//! Checker-side position-keyed `iuncta` element labels.
//!
//! Tuple labels are an identity-erased alias layer: they never enter
//! [`Type::Tuple`](crate::Type::Tuple). This table maps a binding `DefId` to the
//! optional label at each element position so member access (`.gx`) can lower
//! to the same constant-index node as `i[0]`.

use crate::Resolver;
use radix_hir::DefId;
use radix_lexer::{Interner, Symbol};
use radix_syntax::{GenericArgExpr, TypeExpr, TypeExprKind};

/// Extract per-position labels from an `iuncta<…>` type annotation.
///
/// Returns `None` when `ty` is not an `iuncta` named type. Unlabeled slots are
/// `None` in the returned vector; absence of a label is the unlabeled form.
#[must_use]
pub fn from_type_expr(ty: &TypeExpr, interner: &Interner) -> Option<Vec<Option<Symbol>>> {
    let TypeExprKind::Named(name, params) = &ty.kind else {
        return None;
    };
    if interner.resolve(name.name) != "iuncta" {
        return None;
    }
    Some(
        params
            .iter()
            .map(|param| match param {
                GenericArgExpr::Type(ty) => ty.iuncta_label.as_ref().map(|ident| ident.name),
                GenericArgExpr::Size(_) | GenericArgExpr::Name(_) | GenericArgExpr::Range(_) => {
                    None
                }
            })
            .collect(),
    )
}

/// Record labels from `ty` onto `def_id` when `ty` is an `iuncta` annotation.
pub fn record_from_type_expr(
    resolver: &mut Resolver,
    interner: &Interner,
    def_id: DefId,
    ty: &TypeExpr,
) {
    if let Some(labels) = from_type_expr(ty, interner) {
        resolver.record_tuple_element_labels(def_id, labels);
    }
}

/// Position of `label` in a binding's `iuncta` annotation, if declared.
#[must_use]
pub fn position_of(labels: &[Option<Symbol>], label: Symbol) -> Option<usize> {
    labels.iter().position(|slot| *slot == Some(label))
}
