//! Map-key and set-element eligibility (D3.4).
//!
//! A `tabula` key or `copia` element must be hashable: no `fractus` of any
//! width (NaN breaks equality, and ±0 hash apart on some targets), no
//! mutable collection, no dynamic `valor`/`json` document. `iuncta`,
//! `genus` and `discretio` keys are hashable when every part is (the
//! campaign's settled default). The rule fires at each declared type —
//! binding, parameter, field — so a bad key is a Faber diagnostic instead
//! of a late `rustc` failure or a runner trap. Generic parameters and
//! unresolved holes are admitted: their instantiation owns the fact.

#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for typechecking
use super::*;
use radix_lexer::Span;

pub(super) const ISSUE_TABULA_KEY_NOT_HASHABLE: &str = "tabula_key_not_hashable";
pub(super) const ISSUE_COPIA_ELEMENT_NOT_HASHABLE: &str = "copia_element_not_hashable";

impl TypeChecker<'_> {
    /// Reject every `tabula`/`copia` inside `ty` whose key is not hashable.
    pub(super) fn check_hashable_keys(&mut self, ty: TypeId, span: Span) {
        let mut found = Vec::new();
        self.collect_key_violations(ty, &mut FxHashSet::default(), &mut found);
        for issue in found {
            self.error_issue(SemanticErrorKind::TypeMismatch, issue, span);
        }
    }

    /// Walk the structural children of `ty` (not nominal fields: a field's
    /// own collections are checked at the field) collecting violations.
    fn collect_key_violations(
        &self,
        ty: TypeId,
        seen: &mut FxHashSet<TypeId>,
        found: &mut Vec<&'static str>,
    ) {
        if !seen.insert(ty) {
            return;
        }
        let children: Vec<TypeId> = match self.types.get(ty) {
            Type::Map(key, value) | Type::Ledger(key, value) => {
                if !self.is_hashable(*key, &mut FxHashSet::default()) {
                    found.push(ISSUE_TABULA_KEY_NOT_HASHABLE);
                }
                vec![*key, *value]
            }
            Type::Set(element) => {
                if !self.is_hashable(*element, &mut FxHashSet::default()) {
                    found.push(ISSUE_COPIA_ELEMENT_NOT_HASHABLE);
                }
                vec![*element]
            }
            Type::Array(inner)
            | Type::Queue(inner)
            | Type::Stack(inner)
            | Type::Option(inner)
            | Type::Cursor(inner)
            | Type::Promissum(inner)
            | Type::Ref(_, inner)
            | Type::Alias(_, inner)
            | Type::BoundedArray { element: inner, .. }
            | Type::RangedArray { element: inner, .. }
            | Type::BoundedQueue { element: inner, .. }
            | Type::BoundedStack { element: inner, .. } => vec![*inner],
            Type::Tuple(parts) | Type::Union(parts) => parts.clone(),
            Type::Applied(base, args) => std::iter::once(*base)
                .chain(args.iter().filter_map(|arg| match arg {
                    radix_types::AppliedArg::Type(id) => Some(*id),
                    radix_types::AppliedArg::Size(_) => None,
                }))
                .collect(),
            _ => Vec::new(),
        };
        for child in children {
            self.collect_key_violations(child, seen, found);
        }
    }

    /// Whether values of `ty` may key a map or populate a set. `seen`
    /// breaks recursion through self-referential nominal types (a cycle
    /// is admitted; its other fields still decide).
    fn is_hashable(&self, ty: TypeId, seen: &mut FxHashSet<TypeId>) -> bool {
        if !seen.insert(ty) {
            return true;
        }
        match self.types.get(ty).clone() {
            Type::Primitive(
                Primitive::Fractus
                | Primitive::Valor
                | Primitive::Json
                | Primitive::Regex
                | Primitive::Match,
            )
            | Type::SizedNumeric(Primitive::Fractus, _)
            | Type::Array(_)
            | Type::BoundedArray { .. }
            | Type::RangedArray { .. }
            | Type::Map(_, _)
            | Type::Ledger(_, _)
            | Type::Set(_)
            | Type::Queue(_)
            | Type::BoundedQueue { .. }
            | Type::Stack(_)
            | Type::BoundedStack { .. }
            | Type::Tensor(_, _)
            | Type::Vector(_, _)
            | Type::Matrix(_, _)
            | Type::Sparsa(_, _)
            | Type::Atomic(_)
            | Type::Cursor(_)
            | Type::AsyncCursor(_, _)
            | Type::Promissum(_)
            | Type::PromissumFailable(_, _)
            | Type::Func(_) => false,
            Type::Option(inner) | Type::Ref(_, inner) | Type::Alias(_, inner) => {
                self.is_hashable(inner, seen)
            }
            Type::Tuple(parts) | Type::Union(parts) => {
                parts.iter().all(|part| self.is_hashable(*part, seen))
            }
            Type::Struct(def_id) => self.nominal_fields_hashable(def_id, seen),
            Type::Enum(def_id) => self.variant_fields_hashable(def_id, seen),
            Type::Applied(base, args) => {
                self.is_hashable(base, seen)
                    && args.iter().all(|arg| match arg {
                        radix_types::AppliedArg::Type(id) => self.is_hashable(*id, seen),
                        radix_types::AppliedArg::Size(_) => true,
                    })
            }
            _ => true,
        }
    }

    fn nominal_fields_hashable(&self, def_id: DefId, seen: &mut FxHashSet<TypeId>) -> bool {
        let Some(info) = self.structs.get(&def_id) else {
            return true;
        };
        info.fields
            .values()
            .all(|field| self.is_hashable(field.ty, seen))
    }

    fn variant_fields_hashable(&self, enum_def: DefId, seen: &mut FxHashSet<TypeId>) -> bool {
        self.variant_parent
            .iter()
            .filter(|(_, parent)| **parent == enum_def)
            .filter_map(|(variant, _)| self.variant_fields.get(variant))
            .flatten()
            .copied()
            .all(|field| self.is_hashable(field, seen))
    }
}
