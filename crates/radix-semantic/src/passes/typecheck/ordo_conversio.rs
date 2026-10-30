//! Built-in `ordo` conversion rows (DECISIONS D9.4).
//!
//! An `ordo` converts without user code or methods:
//!
//! - `Ordo ↦ numerus` — the member's discriminant; infallible.
//! - `numerus ↦ Ordo` — the first member whose discriminant equals the value;
//!   failable when none matches (`⊥` default, or `textus` propagation).
//! - `Ordo ↦ textus` — the member name; infallible.
//!
//! A discriminant is the authored number, or the previous member's plus one;
//! the first member defaults to 0. A string-valued member has no discriminant,
//! so an `ordo` with one has only the `textus` row. The selected row is
//! recorded on the `Conversio` node ([`HirConversioRoute::Ordo`]) so every
//! backend lowers the same decision. Other pairs involving an `ordo` fall
//! through to the ordinary `unsupported_conversio` rejection.

use super::{
    DefId, FxHashMap, HirExpression, HirItemKind, HirModule, Primitive, Symbol, Type, TypeChecker,
    TypeId,
};
use radix_hir::{HirConversioRoute, HirOrdoRouteMember, HirOrdoRow, HirOrdoValue, HirPresentation};

pub(super) const ISSUE_ORDO_CONVERSIO_STRING_MEMBER: &str = "ordo_conversio_string_member";
pub(super) const ISSUE_ORDO_CONVERSIO_RECOVERY_UNSUPPORTED: &str =
    "ordo_conversio_recovery_unsupported";

/// Authored facts for one `ordo` member, gathered before checking.
#[derive(Debug, Clone)]
pub struct OrdoMemberFact {
    pub variant: DefId,
    pub name: Symbol,
    pub value: Option<HirOrdoValue>,
}

impl TypeChecker<'_> {
    /// Index every local `ordo` by its enum definition. Member names come from
    /// the lowered enum item; discriminants ride the presentation sidecar.
    pub fn collect_ordo_members(&mut self, hir: &HirModule, presentation: &HirPresentation) {
        for ordo in &presentation.ordo_enums {
            let Some(names) = hir.items.iter().find_map(|item| match &item.kind {
                HirItemKind::Enum(enum_item) if item.def_id == ordo.def_id => Some(
                    enum_item
                        .variants
                        .iter()
                        .map(|variant| (variant.def_id, variant.name))
                        .collect::<FxHashMap<_, _>>(),
                ),
                _ => None,
            }) else {
                continue;
            };
            let facts = ordo
                .members
                .iter()
                .filter_map(|member| {
                    Some(OrdoMemberFact {
                        variant: member.def_id,
                        name: *names.get(&member.def_id)?,
                        value: member.value.clone(),
                    })
                })
                .collect();
            self.ordo_members.insert(ordo.def_id, facts);
        }
    }

    /// The `ordo` definition behind `ty`, when it names one.
    pub(super) fn ordo_def(&self, ty: TypeId) -> Option<DefId> {
        match self.types.get(self.resolve_inferred_type(ty)) {
            Type::Enum(def_id) if self.ordo_members.contains_key(def_id) => Some(*def_id),
            _ => None,
        }
    }

    fn is_plain_numerus(&self, ty: TypeId) -> bool {
        matches!(
            self.types.get(self.resolve_inferred_type(ty)),
            Type::Primitive(Primitive::Numerus)
        )
    }

    fn is_plain_textus(&self, ty: TypeId) -> bool {
        matches!(
            self.types.get(self.resolve_inferred_type(ty)),
            Type::Primitive(Primitive::Textus)
        )
    }

    /// Whether `(source, target)` is the failable `numerus ↦ Ordo` row.
    pub(super) fn is_ordo_from_numerus(&self, source_ty: TypeId, target_ty: TypeId) -> bool {
        self.is_plain_numerus(source_ty) && self.ordo_def(target_ty).is_some()
    }

    /// Check a conversion against the built-in `ordo` rows.
    ///
    /// Returns `None` when the pair is not an `ordo` row, so the caller keeps
    /// its ordinary rules. Otherwise returns the result type and the route to
    /// record (absent when the row was rejected).
    pub(super) fn check_ordo_conversio(
        &mut self,
        source_ty: TypeId,
        target_resolved: TypeId,
        recovery: Option<&mut HirExpression>,
        span: radix_lexer::Span,
    ) -> Option<(TypeId, Option<HirConversioRoute>)> {
        let (row, ordo) = if let Some(ordo) = self.ordo_def(source_ty) {
            if self.is_plain_numerus(target_resolved) {
                (HirOrdoRow::ToNumerus, ordo)
            } else if self.is_plain_textus(target_resolved) {
                (HirOrdoRow::ToTextus, ordo)
            } else {
                return None;
            }
        } else if self.is_ordo_from_numerus(source_ty, target_resolved) {
            (HirOrdoRow::FromNumerus, self.ordo_def(target_resolved)?)
        } else {
            return None;
        };

        match (row, recovery) {
            (HirOrdoRow::FromNumerus, Some(recovery)) => {
                self.note_conversio_recovery_checked(recovery, target_resolved);
            }
            (HirOrdoRow::FromNumerus, None) => {
                self.check_conversio_propagation_channel(source_ty, target_resolved, span);
            }
            (_, Some(_)) => {
                self.invalid_conversion(ISSUE_ORDO_CONVERSIO_RECOVERY_UNSUPPORTED, span);
                return Some((self.error_type, None));
            }
            (_, None) => {}
        }

        let Some(members) = self.ordo_route_members(ordo, row) else {
            self.invalid_conversion(ISSUE_ORDO_CONVERSIO_STRING_MEMBER, span);
            return Some((self.error_type, None));
        };
        Some((
            target_resolved,
            Some(HirConversioRoute::Ordo { row, members }),
        ))
    }

    /// Resolve discriminants for `ordo`. `None` when a numeric row meets a
    /// string-valued member.
    fn ordo_route_members(&self, ordo: DefId, row: HirOrdoRow) -> Option<Vec<HirOrdoRouteMember>> {
        let facts = self.ordo_members.get(&ordo)?;
        let mut next = 0i64;
        let mut members = Vec::with_capacity(facts.len());
        for fact in facts {
            let value = match fact.value {
                Some(HirOrdoValue::String(_)) => {
                    if row != HirOrdoRow::ToTextus {
                        return None;
                    }
                    None
                }
                Some(HirOrdoValue::Integer(value)) => Some(value),
                None => Some(next),
            };
            if let Some(value) = value {
                next = value.wrapping_add(1);
            }
            // `numerus ↦ Ordo` selects the first member with the value; a
            // later duplicate can never be chosen, so it carries no probe.
            let value = if row == HirOrdoRow::FromNumerus
                && value.is_some()
                && members
                    .iter()
                    .any(|member: &HirOrdoRouteMember| member.value == value)
            {
                None
            } else {
                value
            };
            members.push(HirOrdoRouteMember {
                variant: fact.variant,
                name: fact.name,
                value,
            });
        }
        Some(members)
    }
}
