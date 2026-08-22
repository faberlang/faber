//! Canonical Faber type emission.
//!
//! The Faber backend prints semantic [`Type`] values back into source-level
//! type syntax. This is a grammar boundary: declarations elsewhere in the
//! backend rely on this module to produce type-first Faber spellings such as
//! `textus`, `lista<T>`, references as `de T`/`in T`, and function types with
//! Faber arrows.

use crate::surface::{KeywordSurface, TypeBindingKind};
use radix_hir::DefId;
use radix_lexer::{Interner, Symbol};
use radix_types::{
    CollectionKind, Primitive, SemanticMutability, SemanticParamMode, TensorFamilyKind, Type,
    TypeId, TypeTable,
};
use rustc_hash::FxHashMap;

impl super::FaberCodegen {
    /// Strip nested semantic option wrappers before printing nullable syntax.
    pub(super) fn flatten_option(&self, mut type_id: TypeId, types: &TypeTable) -> TypeId {
        while let Type::Option(inner) = types.get(type_id) {
            type_id = *inner;
        }
        type_id
    }

    pub(super) fn type_to_faber(
        &self,
        type_id: TypeId,
        types: &TypeTable,
        names: &FxHashMap<DefId, Symbol>,
        interner: &Interner,
        surface: &impl KeywordSurface,
    ) -> String {
        let ty = types.get(type_id);

        match ty {
            Type::Primitive(prim) => surface
                .type_surface(TypeBindingKind::Primitive(*prim))
                .to_owned(),

            Type::SizedInstans(precision) => format!(
                "{}<{}>",
                surface.type_surface(TypeBindingKind::Primitive(Primitive::Instans)),
                precision.faber_marker()
            ),

            Type::Array(elem) => self.format_unary_type(
                surface.type_surface(TypeBindingKind::Collection(CollectionKind::Lista)),
                *elem,
                types,
                names,
                interner,
                surface,
            ),

            Type::Map(key, value) => self.format_binary_type(
                surface.type_surface(TypeBindingKind::Collection(CollectionKind::Tabula)),
                *key,
                *value,
                types,
                names,
                interner,
                surface,
            ),

            Type::Record(fields) => {
                self.format_record_type(fields, types, names, interner, surface)
            }

            Type::Set(elem) => self.format_unary_type(
                surface.type_surface(TypeBindingKind::Collection(CollectionKind::Copia)),
                *elem,
                types,
                names,
                interner,
                surface,
            ),

            Type::Promissum(inner) => self.format_unary_type(
                surface.type_surface(TypeBindingKind::Collection(CollectionKind::Promissum)),
                *inner,
                types,
                names,
                interner,
                surface,
            ),

            Type::PromissumFailable(success, alternate) => format!(
                "{}<{} ⇥ {}>",
                surface.type_surface(TypeBindingKind::Collection(CollectionKind::Promissum)),
                self.type_to_faber(*success, types, names, interner, surface),
                self.type_to_faber(*alternate, types, names, interner, surface)
            ),

            Type::Cursor(inner) => self.format_unary_type(
                surface.type_surface(TypeBindingKind::Collection(CollectionKind::Cursor)),
                *inner,
                types,
                names,
                interner,
                surface,
            ),

            Type::AsyncCursor(item, alternate) => format!(
                "async cursor<{} ⇥ {}>",
                self.type_to_faber(*item, types, names, interner, surface),
                self.type_to_faber(*alternate, types, names, interner, surface)
            ),

            Type::Tensor(elem, shape) => {
                self.format_tensor_type(*elem, *shape, types, names, interner, surface)
            }

            Type::Vector(elem, width) => format!(
                "{}<{}, {}>",
                surface.type_surface(TypeBindingKind::TensorFamily(TensorFamilyKind::Vector)),
                self.type_to_faber(*elem, types, names, interner, surface),
                crate::figura::format_figura(*width, types, interner)
            ),

            Type::Matrix(elem, shape) => format!(
                "{}<{}, {}>",
                surface.type_surface(TypeBindingKind::TensorFamily(TensorFamilyKind::Matrix)),
                self.type_to_faber(*elem, types, names, interner, surface),
                crate::figura::format_figura(*shape, types, interner)
            ),

            Type::Sparsa(elem, shape) => {
                self.format_sparsa_type(*elem, *shape, types, names, interner, surface)
            }

            Type::Atomic(elem) => self.format_unary_type(
                surface.type_surface(TypeBindingKind::TensorFamily(TensorFamilyKind::Atomic)),
                *elem,
                types,
                names,
                interner,
                surface,
            ),

            Type::Intervallum(bound) => self.format_unary_type(
                surface.type_surface(TypeBindingKind::TensorFamily(TensorFamilyKind::Intervallum)),
                *bound,
                types,
                names,
                interner,
                surface,
            ),

            Type::SizedNumeric(prim, width) => match prim {
                Primitive::Numerus => format!(
                    "{}<{}>",
                    surface.type_surface(TypeBindingKind::Primitive(Primitive::Numerus)),
                    width.faber_marker()
                ),
                Primitive::Fractus => width.faber_marker().to_owned(),
                _ => surface
                    .type_surface(TypeBindingKind::Primitive(Primitive::Ignotum))
                    .to_owned(),
            },

            Type::ModularWord(width) => format!(
                "{}<{}>",
                surface.contextual_keyword("modulus"),
                width.faber_marker()
            ),

            Type::Option(inner) => {
                let inner = self.type_to_faber(
                    self.flatten_option(*inner, types),
                    types,
                    names,
                    interner,
                    surface,
                );
                format!("{inner} ∪ {}", surface.contextual_keyword("nihil"))
            }

            Type::Ref(mutability, inner) => {
                let prefix = match mutability {
                    SemanticMutability::Immutable => surface.keyword(radix_lexer::TokenKind::De),
                    SemanticMutability::Mutable => surface.keyword(radix_lexer::TokenKind::In),
                    SemanticMutability::Own => surface.keyword(radix_lexer::TokenKind::Own),
                    SemanticMutability::Copy => surface.keyword(radix_lexer::TokenKind::Copy),
                };
                format!(
                    "{} {}",
                    prefix,
                    self.type_to_faber(*inner, types, names, interner, surface)
                )
            }

            Type::Struct(def_id) | Type::Enum(def_id) | Type::Interface(def_id) => self
                .imported_type_spellings
                .get(&type_id)
                .cloned()
                .unwrap_or_else(|| self.name_for_def(*def_id, names, interner)),

            Type::Alias(def_id, resolved) => names.get(def_id).map_or_else(
                || self.type_to_faber(*resolved, types, names, interner, surface),
                |sym| self.symbol_to_string(*sym, interner),
            ),

            Type::Func(sig) => {
                let params: Vec<String> = sig
                    .params
                    .iter()
                    .map(|p| {
                        let ty = self.type_to_faber(p.ty, types, names, interner, surface);
                        // A type-position marker already rides the param type
                        // (`de T` / `in T`); synthesize that marker only when
                        // an imported signature carries the mode separately.
                        let type_carries_mode = matches!(types.get(p.ty), Type::Ref(..));
                        match p.mode {
                            SemanticParamMode::Owned => ty,
                            SemanticParamMode::Ref if !type_carries_mode => {
                                format!("{} {}", surface.keyword(radix_lexer::TokenKind::De), ty)
                            }
                            SemanticParamMode::MutRef if !type_carries_mode => {
                                format!("{} {}", surface.keyword(radix_lexer::TokenKind::In), ty)
                            }
                            SemanticParamMode::Move => {
                                format!("{} {}", surface.keyword(radix_lexer::TokenKind::Own), ty)
                            }
                            _ => ty,
                        }
                    })
                    .collect();
                let ret = self.type_to_faber(sig.ret, types, names, interner, surface);
                if let Some(err) = sig.err {
                    let err = self.type_to_faber(err, types, names, interner, surface);
                    format!("({}) → {} ⇥ {}", params.join(", "), ret, err)
                } else {
                    format!("({}) → {}", params.join(", "), ret)
                }
            }

            Type::Param(sym) => self.symbol_to_string(*sym, interner),

            Type::Applied(base, args) => {
                let base_str = self.type_to_faber(*base, types, names, interner, surface);
                let args_str: Vec<String> = args
                    .iter()
                    .map(|a| self.type_to_faber(*a, types, names, interner, surface))
                    .collect();
                format!("{}<{}>", base_str, args_str.join(", "))
            }

            Type::Infer(_) => "_".to_owned(),
            Type::InferUnion(_) => "∪".to_owned(),
            Type::MarkerHole { .. } => "numerus<_>".to_owned(),
            Type::Tuple(members) => {
                let members = members
                    .iter()
                    .map(|member| self.type_to_faber(*member, types, names, interner, surface))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{}<{members}>", surface.contextual_keyword("iuncta"))
            }
            Type::Union(members) => {
                self.format_union_type(members, types, names, interner, surface)
            }
            Type::Error => "<error>".to_owned(),
            Type::BoundedArray { element, capacity } => format!(
                "lista<{}, {}>",
                self.type_to_faber(*element, types, names, interner, surface),
                crate::figura::format_figura(*capacity, types, interner)
            ),
            Type::BoundedTextus { capacity } => {
                format!(
                    "textus<{}>",
                    crate::figura::format_figura(*capacity, types, interner)
                )
            }
            Type::BoundedAscii { capacity } => {
                format!(
                    "ascii<{}>",
                    crate::figura::format_figura(*capacity, types, interner)
                )
            }
            Type::BoundedOcteti { capacity } => {
                format!(
                    "octeti<{}>",
                    crate::figura::format_figura(*capacity, types, interner)
                )
            }
        }
    }

    fn format_unary_type(
        &self,
        head: &str,
        inner: TypeId,
        types: &TypeTable,
        names: &FxHashMap<DefId, Symbol>,
        interner: &Interner,
        surface: &impl KeywordSurface,
    ) -> String {
        format!(
            "{head}<{}>",
            self.type_to_faber(inner, types, names, interner, surface)
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn format_binary_type(
        &self,
        head: &str,
        left: TypeId,
        right: TypeId,
        types: &TypeTable,
        names: &FxHashMap<DefId, Symbol>,
        interner: &Interner,
        surface: &impl KeywordSurface,
    ) -> String {
        format!(
            "{head}<{}, {}>",
            self.type_to_faber(left, types, names, interner, surface),
            self.type_to_faber(right, types, names, interner, surface)
        )
    }

    fn format_union_type(
        &self,
        members: &[TypeId],
        types: &TypeTable,
        names: &FxHashMap<DefId, Symbol>,
        interner: &Interner,
        surface: &impl KeywordSurface,
    ) -> String {
        if members.is_empty() {
            return surface
                .type_surface(TypeBindingKind::Primitive(Primitive::Ignotum))
                .to_owned();
        }
        members
            .iter()
            .map(|member| self.type_to_faber(*member, types, names, interner, surface))
            .collect::<Vec<_>>()
            .join(" ∪ ")
    }

    fn format_record_type(
        &self,
        fields: &rustc_hash::FxHashMap<Symbol, TypeId>,
        types: &TypeTable,
        names: &FxHashMap<DefId, Symbol>,
        interner: &Interner,
        surface: &impl KeywordSurface,
    ) -> String {
        let entries: Vec<String> = sorted_record_fields(fields, interner)
            .into_iter()
            .map(|(name, ty)| {
                format!(
                    "{}: {}",
                    self.symbol_to_string(name, interner),
                    self.type_to_faber(ty, types, names, interner, surface)
                )
            })
            .collect();
        format!(
            "{}<{}>",
            surface.contextual_keyword("ratio"),
            entries.join(", ")
        )
    }

    fn format_tensor_type(
        &self,
        elem: TypeId,
        shape: radix_types::IndexId,
        types: &TypeTable,
        names: &FxHashMap<DefId, Symbol>,
        interner: &Interner,
        surface: &impl KeywordSurface,
    ) -> String {
        // Long form only: `tensor<f32, [2, 2]>` is the documented default
        // surface and re-parses in every locale. The `tf32[2, 2]` width sugar
        // remains valid input but is not emitted canonically, so canonical
        // round-trips keep the author's type spelling.
        format!(
            "{}<{}, {}>",
            surface.type_surface(TypeBindingKind::TensorFamily(TensorFamilyKind::Tensor)),
            self.type_to_faber(elem, types, names, interner, surface),
            crate::figura::format_figura(shape, types, interner)
        )
    }

    fn format_sparsa_type(
        &self,
        elem: TypeId,
        shape: radix_types::IndexId,
        types: &TypeTable,
        names: &FxHashMap<DefId, Symbol>,
        interner: &Interner,
        surface: &impl KeywordSurface,
    ) -> String {
        format!(
            "{}<{}, {}>",
            surface.type_surface(TypeBindingKind::TensorFamily(TensorFamilyKind::Sparsa)),
            self.type_to_faber(elem, types, names, interner, surface),
            crate::figura::format_figura(shape, types, interner)
        )
    }
}

/// Canonical ratio field order: sorted by label spelling.
fn sorted_record_fields(
    fields: &FxHashMap<Symbol, TypeId>,
    interner: &Interner,
) -> Vec<(Symbol, TypeId)> {
    let mut ordered: Vec<_> = fields.iter().map(|(&name, &ty)| (name, ty)).collect();
    ordered.sort_by(|(left, _), (right, _)| interner.resolve(*left).cmp(interner.resolve(*right)));
    ordered
}

#[cfg(test)]
#[path = "types_test.rs"]
mod tests;
