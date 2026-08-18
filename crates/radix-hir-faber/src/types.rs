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
    CollectionKind, Primitive, SemanticMutability, TensorFamilyKind, Type, TypeId, TypeTable,
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
                    .map(|p| self.type_to_faber(p.ty, types, names, interner, surface))
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
        let tabula = surface.type_surface(TypeBindingKind::Collection(CollectionKind::Tabula));
        if fields.is_empty() {
            return format!(
                "{tabula}<{}, {}>",
                surface.type_surface(TypeBindingKind::Primitive(Primitive::Textus)),
                surface.type_surface(TypeBindingKind::Primitive(Primitive::Valor))
            );
        }

        // Parenthesize `name: type` fields so the record stays one type in
        // every position; comma-splitting would break precedence on re-parse.
        let entries: Vec<String> = fields
            .iter()
            .map(|(name, ty)| {
                format!(
                    "{}: {}",
                    self.symbol_to_string(*name, interner),
                    self.type_to_faber(*ty, types, names, interner, surface)
                )
            })
            .collect();
        format!("({})", entries.join(", "))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{empty_presentation, FaberCodegen, KeywordSurface, TypeBindingKind};
    use radix_hir::{
        DefId, HirAnnotation, HirBlock, HirCallArg, HirCape, HirCasuArm, HirEnum, HirExpression,
        HirExpressionKind, HirFunction, HirId, HirImport, HirImportItem, HirItem, HirItemKind,
        HirLiteral, HirLocal, HirModule, HirPattern, HirStatement, HirStatementKind, HirStruct,
        HirVariant,
    };
    use radix_lexer::{Interner, Span, TokenKind};

    /// Minimal Latin surface so the leaf crate can unit-test without the façade pack.
    struct LatinSurface;

    impl KeywordSurface for LatinSurface {
        fn keyword(&self, kind: TokenKind) -> &str {
            match kind {
                TokenKind::Incipit => "incipit",
                TokenKind::Incipiet => "incipiet",
                TokenKind::Argumenta => "argumenta",
                TokenKind::De => "de",
                TokenKind::In => "in",
                TokenKind::Functio => "functio",
                TokenKind::Redde => "redde",
                TokenKind::Sic => "sic",
                TokenKind::Secus => "secus",
                TokenKind::Si => "si",
                TokenKind::Sin => "sin",
                TokenKind::Ergo => "ergo",
                TokenKind::Fixum => "fixum",
                TokenKind::Cape => "cape",
                TokenKind::Fac => "fac",
                TokenKind::Dum => "dum",
                TokenKind::Discerne => "discerne",
                TokenKind::Omnia => "omnia",
                TokenKind::Casu => "casu",
                TokenKind::Ut => "ut",
                TokenKind::Verum => "verum",
                TokenKind::Falsum => "falsum",
                TokenKind::Importa => "importa",
                TokenKind::Ex => "ex",
                TokenKind::Publica => "publica",
                TokenKind::Privata => "privata",
                TokenKind::Sparge => "sparge",
                TokenKind::Lege => "lege",
                TokenKind::Rumpe => "rumpe",
                TokenKind::Vide => "vide",
                TokenKind::Mone => "mone",
                TokenKind::Nota => "nota",
                TokenKind::Adfirma => "adfirma",
                TokenKind::Requirit => "requirit",
                TokenKind::Mori => "mori",
                TokenKind::Iace => "iace",
                TokenKind::Finge => "finge",
                TokenKind::Cede => "cede",
                TokenKind::Tacet => "tacet",
                TokenKind::Perge => "perge",
                TokenKind::Custodi => "custodi",
                TokenKind::Varia => "varia",
                TokenKind::Itera => "itera",
                TokenKind::Nihil => "nihil",
                TokenKind::Non => "non",
                TokenKind::Est => "est",
                TokenKind::Sponte => "sponte",
                TokenKind::Vel => "vel",
                TokenKind::Scriptum => "scriptum",
                TokenKind::Proba => "proba",
                TokenKind::Omitte => "omitte",
                TokenKind::Futurum => "futurum",
                TokenKind::Solum => "solum",
                TokenKind::Tag => "tag",
                TokenKind::Temporis => "temporis",
                TokenKind::Metior => "metior",
                TokenKind::Repete => "repete",
                TokenKind::Fragilis => "fragilis",
                TokenKind::SolumIn => "solum_in",
                TokenKind::Magnitudo => "magnitudo",
                TokenKind::Per => "per",
                TokenKind::Ad => "ad",
                TokenKind::Genus => "genus",
                TokenKind::Discretio => "discretio",
                TokenKind::Implendum => "implendum",
                TokenKind::Typus => "typus",
                TokenKind::Sub => "sub",
                TokenKind::Implet => "implet",
                TokenKind::Generis => "generis",
                _ => "?",
            }
        }

        fn type_surface(&self, kind: TypeBindingKind) -> &str {
            match kind {
                TypeBindingKind::Primitive(Primitive::Textus) => "textus",
                TypeBindingKind::Primitive(Primitive::Numerus) => "numerus",
                TypeBindingKind::Primitive(Primitive::Fractus) => "fractus",
                TypeBindingKind::Primitive(Primitive::Bivalens) => "bivalens",
                TypeBindingKind::Primitive(Primitive::Nihil) => "nihil",
                TypeBindingKind::Primitive(Primitive::Vacuum) => "vacuum",
                TypeBindingKind::Primitive(Primitive::Valor) => "valor",
                TypeBindingKind::Collection(CollectionKind::Lista) => "lista",
                TypeBindingKind::Collection(CollectionKind::Tabula) => "tabula",
                TypeBindingKind::Collection(CollectionKind::Copia) => "copia",
                TypeBindingKind::Collection(CollectionKind::Promissum) => "promissum",
                TypeBindingKind::Collection(CollectionKind::Cursor) => "cursor",
                TypeBindingKind::TensorFamily(TensorFamilyKind::Tensor) => "tensor",
                TypeBindingKind::TensorFamily(TensorFamilyKind::Vector) => "vector",
                TypeBindingKind::TensorFamily(TensorFamilyKind::Matrix) => "matrix",
                TypeBindingKind::TensorFamily(TensorFamilyKind::Sparsa) => "sparsa",
                TypeBindingKind::TensorFamily(TensorFamilyKind::Atomic) => "atomic",
                TypeBindingKind::TensorFamily(TensorFamilyKind::Intervallum) => "intervallum",
                TypeBindingKind::ModularWord => "modulus",
                _ => "ignotum",
            }
        }
    }

    #[test]
    fn type_to_faber_prints_imported_qualified_spelling() {
        // Regression: an imported nominal (`base.Vector3`) has no local def
        // name, so the emitter printed the `def_N` fallback. The qualified
        // spelling carried from the import contract must win.
        let mut codegen = FaberCodegen::default();
        let mut types = TypeTable::new();
        let names = FxHashMap::default();
        let interner = Interner::new();
        let surface = LatinSurface;

        let type_id = types.intern(Type::Struct(DefId(0x9000_0001)));
        codegen
            .imported_type_spellings
            .insert(type_id, "base.Vector3".to_owned());
        assert_eq!(
            codegen.type_to_faber(type_id, &types, &names, &interner, &surface),
            "base.Vector3"
        );
    }

    #[test]
    fn type_to_faber_renders_primitives_lista_and_option() {
        let codegen = FaberCodegen::default();
        let mut types = TypeTable::new();
        let names = FxHashMap::default();
        let interner = Interner::new();
        let surface = LatinSurface;

        let textus = types.primitive(Primitive::Textus);
        assert_eq!(
            codegen.type_to_faber(textus, &types, &names, &interner, &surface),
            "textus"
        );

        let numerus = types.primitive(Primitive::Numerus);
        let lista = types.array(numerus);
        assert_eq!(
            codegen.type_to_faber(lista, &types, &names, &interner, &surface),
            "lista<numerus>"
        );

        let option = types.option(textus);
        assert_eq!(
            codegen.type_to_faber(option, &types, &names, &interner, &surface),
            "textus ∪ nihil"
        );

        let i32_ty = types.sized_numeric(Primitive::Numerus, radix_types::NumericWidth::I32);
        assert_eq!(
            codegen.type_to_faber(i32_ty, &types, &names, &interner, &surface),
            "numerus<i32>"
        );
    }

    #[test]
    fn flatten_option_strips_nested_wrappers() {
        let codegen = FaberCodegen::default();
        let mut types = TypeTable::new();
        let textus = types.primitive(Primitive::Textus);
        let once = types.option(textus);
        let twice = types.option(once);
        assert_eq!(codegen.flatten_option(twice, &types), textus);
        assert_eq!(codegen.flatten_option(textus, &types), textus);
    }

    #[test]
    fn emit_empty_hir_program_succeeds() {
        let hir = HirModule {
            items: Vec::new(),
            entry: None,
            entry_annotations: Vec::new(),
            entry_args_name: None,
            entry_is_async: false,
        };
        let types = TypeTable::new();
        let interner = Interner::new();
        let out = crate::emit(
            &hir,
            &empty_presentation(),
            &types,
            &interner,
            &LatinSurface,
            &FxHashMap::default(),
            &FxHashMap::default(),
        )
        .expect("empty HIR must emit");
        assert!(
            out.code.is_empty(),
            "empty program should emit empty source, got {:?}",
            out.code
        );
    }

    // ── Tensor-family + vacua surface routing probe ───────────────────────

    /// Probe surface that maps tensor-family and vacua identities to markers.
    ///
    /// If the emitter printed hardcoded Latin, the markers would never appear.
    struct ProbeSurface;

    impl KeywordSurface for ProbeSurface {
        fn keyword(&self, _kind: TokenKind) -> &'static str {
            "kw"
        }

        fn contextual_keyword(&self, latin_key: &'static str) -> &str {
            match latin_key {
                "vacua" => "VACUA_MARKER",
                key => key,
            }
        }

        fn type_surface(&self, kind: TypeBindingKind) -> &str {
            match kind {
                TypeBindingKind::TensorFamily(TensorFamilyKind::Tensor) => "TENSOR_MARKER",
                TypeBindingKind::TensorFamily(TensorFamilyKind::Vector) => "VECTOR_MARKER",
                TypeBindingKind::TensorFamily(TensorFamilyKind::Matrix) => "MATRIX_MARKER",
                TypeBindingKind::TensorFamily(TensorFamilyKind::Sparsa) => "SPARSA_MARKER",
                TypeBindingKind::TensorFamily(TensorFamilyKind::Atomic) => "ATOMIC_MARKER",
                TypeBindingKind::TensorFamily(TensorFamilyKind::Intervallum) => {
                    "INTERVALLUM_MARKER"
                }
                TypeBindingKind::Primitive(Primitive::Numerus) => "numerus",
                _ => "ignotum",
            }
        }
    }

    #[test]
    fn tensor_family_type_emit_routes_through_type_surface() {
        let codegen = FaberCodegen::default();
        let mut types = TypeTable::new();
        let names = FxHashMap::default();
        let interner = Interner::new();
        let surface = ProbeSurface;

        let i32_ty = types.sized_numeric(Primitive::Numerus, radix_types::NumericWidth::I32);
        let rank0 = types.rank0_shape();
        let lit4 = types.intern_index(radix_types::IndexExpr::Literal(4));
        let width4 = types.intern_index(radix_types::IndexExpr::Tuple(vec![lit4]));
        let lit2 = types.intern_index(radix_types::IndexExpr::Literal(2));
        let square = types.intern_index(radix_types::IndexExpr::Tuple(vec![lit2, lit2]));

        assert_eq!(
            codegen.type_to_faber(
                types.tensor_with_shape(i32_ty, rank0),
                &types,
                &names,
                &interner,
                &surface
            ),
            "TENSOR_MARKER<numerus<i32>, []>"
        );
        assert_eq!(
            codegen.type_to_faber(
                types.vector_with_width(i32_ty, width4),
                &types,
                &names,
                &interner,
                &surface
            ),
            "VECTOR_MARKER<numerus<i32>, [4]>"
        );
        assert_eq!(
            codegen.type_to_faber(
                types.matrix_with_shape(i32_ty, square),
                &types,
                &names,
                &interner,
                &surface
            ),
            "MATRIX_MARKER<numerus<i32>, [2, 2]>"
        );
        assert_eq!(
            codegen.type_to_faber(
                types.sparsa_with_shape(i32_ty, rank0),
                &types,
                &names,
                &interner,
                &surface
            ),
            "SPARSA_MARKER<numerus<i32>, []>"
        );
        assert_eq!(
            codegen.type_to_faber(types.atomic(i32_ty), &types, &names, &interner, &surface),
            "ATOMIC_MARKER<numerus<i32>>"
        );
        assert_eq!(
            codegen.type_to_faber(
                types.intervallum(types.primitive(Primitive::Numerus)),
                &types,
                &names,
                &interner,
                &surface
            ),
            "INTERVALLUM_MARKER<numerus>"
        );
    }

    #[test]
    fn vacua_emit_routes_through_contextual_keyword() {
        // A bare entry block containing only the empty-collection marker: the
        // emitted source must carry the probe marker, proving the Vacua arm
        // consults the surface instead of printing the literal "vacua".
        let interner = Interner::new();
        let hir = HirModule {
            items: Vec::new(),
            entry: Some(HirBlock::simple(
                vec![HirStatement {
                    id: HirId(1),
                    kind: HirStatementKind::Expr(HirExpression {
                        id: HirId(2),
                        kind: HirExpressionKind::Vacua,
                        ty: None,
                        span: span(),
                    }),
                    span: span(),
                }],
                None,
                span(),
            )),
            entry_annotations: Vec::new(),
            entry_args_name: None,
            entry_is_async: false,
        };
        let types = TypeTable::new();
        let out = crate::emit(
            &hir,
            &empty_presentation(),
            &types,
            &interner,
            &ProbeSurface,
            &FxHashMap::default(),
            &FxHashMap::default(),
        )
        .expect("vacua program must emit");
        assert!(
            out.code.contains("VACUA_MARKER"),
            "Vacua emit must route through contextual_keyword, got:\n{}",
            out.code
        );
        assert!(
            !out.code.contains("vacua"),
            "Vacua emit must not print the literal Latin spelling, got:\n{}",
            out.code
        );
    }

    fn span() -> Span {
        Span::default()
    }

    fn generate_faber(program: &HirModule, types: &TypeTable, interner: &Interner) -> String {
        FaberCodegen::default()
            .generate(
                program,
                &empty_presentation(),
                types,
                interner,
                &LatinSurface,
            )
            .expect("codegen")
            .code
    }

    #[test]
    fn multi_import_emits_all_items() {
        let mut interner = Interner::new();
        let path_sym = interner.intern("pkg");
        let name_a = interner.intern("alpha");
        let name_b = interner.intern("beta");

        let types = TypeTable::new();

        let import = HirImport {
            path: path_sym,
            visibility: radix_syntax::Visibility::Publica,
            items: vec![
                HirImportItem {
                    def_id: DefId(1),
                    name: name_a,
                    alias: None,
                },
                HirImportItem {
                    def_id: DefId(2),
                    name: name_b,
                    alias: None,
                },
            ],
        };

        let program = HirModule {
            items: vec![HirItem {
                id: HirId(0),
                def_id: DefId(radix_types::USER_DEF_ID_BASE),
                kind: HirItemKind::Import(import),
                span: span(),
            }],
            entry: None,
            entry_annotations: Vec::new(),
            entry_args_name: None,
            entry_is_async: false,
        };

        let code = generate_faber(&program, &types, &interner);
        assert!(
            code.contains("alpha") && code.contains("beta"),
            "multi-import must emit all items, got:\n{code}"
        );
    }

    #[test]
    fn discerne_omnia_preserves_exhaustiveness() {
        let mut interner = Interner::new();
        let name_f = interner.intern("f");
        let name_x = interner.intern("x");

        let types = TypeTable::new();
        let bivalens = types.primitive(Primitive::Bivalens);

        let discerne = HirExpression {
            id: HirId(10),
            kind: HirExpressionKind::Discerne {
                scrutinees: vec![HirExpression {
                    id: HirId(11),
                    kind: HirExpressionKind::Path(DefId(1)),
                    ty: Some(bivalens),
                    span: span(),
                }],
                arms: vec![HirCasuArm {
                    patterns: vec![HirPattern::Variant(
                        DefId(2),
                        vec![HirPattern::Binding(DefId(3), name_x)],
                    )],
                    guard: None,
                    body: HirExpression {
                        id: HirId(12),
                        kind: HirExpressionKind::Block(HirBlock {
                            statements: vec![],
                            expr: Some(Box::new(HirExpression {
                                id: HirId(13),
                                kind: HirExpressionKind::Literal(HirLiteral::Bool(true)),
                                ty: Some(bivalens),
                                span: span(),
                            })),
                            span: span(),
                            ..Default::default()
                        }),
                        ty: Some(bivalens),
                        span: span(),
                    },
                    span: span(),
                }],
                exhaustive: true,
            },
            ty: Some(bivalens),
            span: span(),
        };

        let function = HirFunction {
            cli_args: None,
            name: name_f,
            type_params: Vec::new(),
            params: Vec::new(),
            ret_ty: Some(bivalens),
            err_ty: None,
            body: Some(HirBlock {
                statements: vec![HirStatement {
                    id: HirId(20),
                    kind: HirStatementKind::Expr(discerne),
                    span: span(),
                }],
                expr: None,
                span: span(),
                ..Default::default()
            }),
            posture: radix_syntax::CallablePosture::SyncFinite,
            is_async: false,
            is_generator: false,
            is_kernel: false,
            nucleum_fragment: false,
            is_vertex: false,
            is_fragment: false,
            annotations: Vec::new(),
            test: None,
        };

        let program = HirModule {
            items: vec![HirItem {
                id: HirId(0),
                def_id: DefId(radix_types::USER_DEF_ID_BASE),
                kind: HirItemKind::Function(Box::new(function)),
                span: span(),
            }],
            entry: None,
            entry_annotations: Vec::new(),
            entry_args_name: None,
            entry_is_async: false,
        };

        let code = generate_faber(&program, &types, &interner);
        assert!(
            code.contains("omnia"),
            "exhaustive discerne must emit `omnia` keyword, got:\n{code}"
        );
    }

    #[test]
    fn si_with_cape_preserves_cape_keyword() {
        let mut interner = Interner::new();
        let name_f = interner.intern("f");
        let name_err = interner.intern("err");

        let types = TypeTable::new();
        let textus = types.primitive(Primitive::Textus);
        let bivalens = types.primitive(Primitive::Bivalens);

        let then_block = HirBlock {
            statements: vec![HirStatement {
                id: HirId(20),
                kind: HirStatementKind::Redde(Some(HirExpression {
                    id: HirId(21),
                    kind: HirExpressionKind::Literal(HirLiteral::String(interner.intern("ok"))),
                    ty: Some(textus),
                    span: span(),
                })),
                span: span(),
            }],
            expr: None,
            span: span(),
            ..Default::default()
        };

        let else_block = HirBlock {
            statements: vec![HirStatement {
                id: HirId(30),
                kind: HirStatementKind::Redde(Some(HirExpression {
                    id: HirId(31),
                    kind: HirExpressionKind::Literal(HirLiteral::String(interner.intern("nope"))),
                    ty: Some(textus),
                    span: span(),
                })),
                span: span(),
            }],
            expr: None,
            span: span(),
            ..Default::default()
        };

        let catch = HirCape {
            binding_def_id: DefId(100),
            binding_name: name_err,
            binding_ty: Some(textus),
            body: HirBlock {
                statements: vec![HirStatement {
                    id: HirId(40),
                    kind: HirStatementKind::Redde(Some(HirExpression {
                        id: HirId(41),
                        kind: HirExpressionKind::Literal(HirLiteral::String(
                            interner.intern("caught"),
                        )),
                        ty: Some(textus),
                        span: span(),
                    })),
                    span: span(),
                }],
                expr: None,
                span: span(),
                ..Default::default()
            },
            span: span(),
        };

        let expr = HirExpression {
            id: HirId(10),
            kind: HirExpressionKind::Si {
                cond: Box::new(HirExpression {
                    id: HirId(11),
                    kind: HirExpressionKind::Literal(HirLiteral::Bool(true)),
                    ty: Some(bivalens),
                    span: span(),
                }),
                then_block,
                then_catch: Some(Box::new(catch)),
                else_block: Some(else_block),
            },
            ty: Some(textus),
            span: span(),
        };

        let function = HirFunction {
            cli_args: None,
            name: name_f,
            type_params: Vec::new(),
            params: Vec::new(),
            ret_ty: Some(textus),
            err_ty: None,
            body: Some(HirBlock {
                statements: vec![HirStatement {
                    id: HirId(22),
                    kind: HirStatementKind::Expr(expr),
                    span: span(),
                }],
                expr: None,
                span: span(),
                ..Default::default()
            }),
            posture: radix_syntax::CallablePosture::SyncFinite,
            is_async: false,
            is_generator: false,
            is_kernel: false,
            nucleum_fragment: false,
            is_vertex: false,
            is_fragment: false,
            annotations: Vec::new(),
            test: None,
        };

        let program = HirModule {
            items: vec![HirItem {
                id: HirId(0),
                def_id: DefId(radix_types::USER_DEF_ID_BASE),
                kind: HirItemKind::Function(Box::new(function)),
                span: span(),
            }],
            entry: None,
            entry_annotations: Vec::new(),
            entry_args_name: None,
            entry_is_async: false,
        };

        let code = generate_faber(&program, &types, &interner);
        assert!(
            code.contains("cape"),
            "si with then_catch + else_block must emit `cape` keyword, got:\n{code}"
        );
        assert!(
            code.contains("err"),
            "cape binding name must appear in output, got:\n{code}"
        );
    }

    #[test]
    fn si_cape_else_with_compact_else_preserves_cape() {
        // Si with then_catch AND an else_block that is sic/secus compatible
        // (single-expression return). The cape must still appear.
        let mut interner = Interner::new();
        let name_f = interner.intern("f");
        let name_err = interner.intern("err");

        let types = TypeTable::new();
        let textus = types.primitive(Primitive::Textus);
        let bivalens = types.primitive(Primitive::Bivalens);

        let then_block = HirBlock {
            statements: vec![HirStatement {
                id: HirId(20),
                kind: HirStatementKind::Redde(Some(HirExpression {
                    id: HirId(21),
                    kind: HirExpressionKind::Literal(HirLiteral::String(interner.intern("ok"))),
                    ty: Some(textus),
                    span: span(),
                })),
                span: span(),
            }],
            expr: None,
            span: span(),
            ..Default::default()
        };

        let else_block = HirBlock {
            statements: vec![HirStatement {
                id: HirId(30),
                kind: HirStatementKind::Redde(Some(HirExpression {
                    id: HirId(31),
                    kind: HirExpressionKind::Literal(HirLiteral::String(interner.intern("nope"))),
                    ty: Some(textus),
                    span: span(),
                })),
                span: span(),
            }],
            expr: None,
            span: span(),
            ..Default::default()
        };

        let catch = HirCape {
            binding_def_id: DefId(100),
            binding_name: name_err,
            binding_ty: Some(textus),
            body: HirBlock {
                statements: vec![],
                expr: Some(Box::new(HirExpression {
                    id: HirId(41),
                    kind: HirExpressionKind::Literal(HirLiteral::String(interner.intern("caught"))),
                    ty: Some(textus),
                    span: span(),
                })),
                span: span(),
                ..Default::default()
            },
            span: span(),
        };

        let expr = HirExpression {
            id: HirId(10),
            kind: HirExpressionKind::Si {
                cond: Box::new(HirExpression {
                    id: HirId(11),
                    kind: HirExpressionKind::Literal(HirLiteral::Bool(true)),
                    ty: Some(bivalens),
                    span: span(),
                }),
                then_block,
                then_catch: Some(Box::new(catch)),
                else_block: Some(else_block),
            },
            ty: Some(textus),
            span: span(),
        };

        let function = HirFunction {
            cli_args: None,
            name: name_f,
            type_params: Vec::new(),
            params: Vec::new(),
            ret_ty: Some(textus),
            err_ty: None,
            body: Some(HirBlock {
                statements: vec![HirStatement {
                    id: HirId(22),
                    kind: HirStatementKind::Expr(expr),
                    span: span(),
                }],
                expr: None,
                span: span(),
                ..Default::default()
            }),
            posture: radix_syntax::CallablePosture::SyncFinite,
            is_async: false,
            is_generator: false,
            is_kernel: false,
            nucleum_fragment: false,
            is_vertex: false,
            is_fragment: false,
            annotations: Vec::new(),
            test: None,
        };

        let program = HirModule {
            items: vec![HirItem {
                id: HirId(0),
                def_id: DefId(radix_types::USER_DEF_ID_BASE),
                kind: HirItemKind::Function(Box::new(function)),
                span: span(),
            }],
            entry: None,
            entry_annotations: Vec::new(),
            entry_args_name: None,
            entry_is_async: false,
        };

        let code = generate_faber(&program, &types, &interner);
        assert!(
            code.contains("cape"),
            "si with then_catch + else_block must emit `cape` keyword, got:\n{code}"
        );
        assert!(
            code.contains("caught"),
            "cape body value must appear, got:\n{code}"
        );
        assert!(
            code.contains("nope"),
            "else_block value must appear, got:\n{code}"
        );
        let cape_pos = code.find("cape").unwrap();
        let secus_pos = code.find("secus").unwrap();
        assert!(
            cape_pos < secus_pos,
            "cape must appear before secus, got cape at {cape_pos}, secus at {secus_pos}\n{code}"
        );
    }

    #[test]
    fn fac_loop_detection_long_body() {
        // A `fac ... dum` where the body has multiple statements before the
        // final rumpe+not pattern. `as_fac_loop` must still recognize it.
        let mut interner = Interner::new();
        let name_f = interner.intern("f");

        let mut types = TypeTable::new();
        let numerus = types.primitive(Primitive::Numerus);
        let bivalens = types.primitive(Primitive::Bivalens);
        let i32_ty = types.sized_numeric(Primitive::Numerus, radix_types::NumericWidth::I32);

        let rumpe_si = HirExpression {
            id: HirId(30),
            kind: HirExpressionKind::Si {
                cond: Box::new(HirExpression {
                    id: HirId(31),
                    kind: HirExpressionKind::Unary(
                        radix_hir::HirUnOp::Not,
                        Box::new(HirExpression {
                            id: HirId(32),
                            kind: HirExpressionKind::Literal(HirLiteral::Bool(true)),
                            ty: Some(bivalens),
                            span: span(),
                        }),
                    ),
                    ty: Some(bivalens),
                    span: span(),
                }),
                then_block: HirBlock {
                    statements: vec![HirStatement {
                        id: HirId(33),
                        kind: HirStatementKind::Rumpe,
                        span: span(),
                    }],
                    expr: None,
                    span: span(),
                    ..Default::default()
                },
                then_catch: None,
                else_block: None,
            },
            ty: Some(bivalens),
            span: span(),
        };

        let loop_block = HirBlock {
            statements: vec![
                HirStatement {
                    id: HirId(40),
                    kind: HirStatementKind::Local(radix_hir::HirLocal {
                        def_id: DefId(50),
                        name: interner.intern("x"),
                        ty: Some(i32_ty),
                        mutable: true,
                        init: Some(HirExpression {
                            id: HirId(41),
                            kind: HirExpressionKind::Literal(HirLiteral::Int(0)),
                            ty: Some(numerus),
                            span: span(),
                        }),
                        runtime_provided: false,
                        is_await: false,
                    }),
                    span: span(),
                },
                HirStatement {
                    id: HirId(42),
                    kind: HirStatementKind::Expr(rumpe_si),
                    span: span(),
                },
            ],
            expr: None,
            span: span(),
            ..Default::default()
        };

        let expr = HirExpression {
            id: HirId(10),
            kind: HirExpressionKind::Loop(loop_block),
            ty: Some(numerus),
            span: span(),
        };

        let function = HirFunction {
            cli_args: None,
            name: name_f,
            type_params: Vec::new(),
            params: Vec::new(),
            ret_ty: Some(numerus),
            err_ty: None,
            body: Some(HirBlock {
                statements: vec![HirStatement {
                    id: HirId(22),
                    kind: HirStatementKind::Expr(expr),
                    span: span(),
                }],
                expr: None,
                span: span(),
                ..Default::default()
            }),
            posture: radix_syntax::CallablePosture::SyncFinite,
            is_async: false,
            is_generator: false,
            is_kernel: false,
            nucleum_fragment: false,
            is_vertex: false,
            is_fragment: false,
            annotations: Vec::new(),
            test: None,
        };

        let program = HirModule {
            items: vec![HirItem {
                id: HirId(0),
                def_id: DefId(radix_types::USER_DEF_ID_BASE),
                kind: HirItemKind::Function(Box::new(function)),
                span: span(),
            }],
            entry: None,
            entry_annotations: Vec::new(),
            entry_args_name: None,
            entry_is_async: false,
        };

        let code = generate_faber(&program, &types, &interner);
        assert!(
            code.contains("fac"),
            "multi-statement loop body must be recognized as fac, got:\n{code}"
        );
        assert!(
            code.contains("dum"),
            "loop must contain dum keyword, got:\n{code}"
        );
    }

    #[test]
    fn record_type_preserves_field_names() {
        // Record types must emit field names, not just value type de-duplication.
        let codegen = FaberCodegen::default();
        let mut types = TypeTable::new();
        let names = FxHashMap::default();
        let mut interner = Interner::new();
        let surface = LatinSurface;

        let textus = types.primitive(Primitive::Textus);
        let numerus = types.primitive(Primitive::Numerus);

        let name_sym = interner.intern("name");
        let desc_sym = interner.intern("desc");
        let age_sym = interner.intern("age");
        let mut fields = FxHashMap::default();
        fields.insert(name_sym, textus);
        fields.insert(desc_sym, textus);
        fields.insert(age_sym, numerus);
        let record_id = types.intern(radix_types::Type::Record(fields));

        let result = codegen.type_to_faber(record_id, &types, &names, &interner, &surface);
        assert!(
            result.contains("name"),
            "record type must contain field name 'name', got: {result}"
        );
        assert!(
            result.contains("desc"),
            "record type must contain field name 'desc', got: {result}"
        );
        assert!(
            result.contains("age"),
            "record type must contain field name 'age', got: {result}"
        );
    }

    #[test]
    fn closure_params_preserve_sponte_and_defaults() {
        let mut interner = Interner::new();
        let name_f = interner.intern("f");
        let name_x = interner.intern("x");
        let name_y = interner.intern("y");

        let types = TypeTable::new();
        let textus = types.primitive(Primitive::Textus);

        let closure = HirExpression {
            id: HirId(10),
            kind: HirExpressionKind::Clausura(
                vec![
                    radix_hir::HirParam {
                        def_id: DefId(1),
                        name: name_x,
                        ty: textus,
                        mode: radix_hir::HirParamMode::Owned,
                        optional: false,
                        sponte: false,
                        default: None,
                        span: span(),
                    },
                    radix_hir::HirParam {
                        def_id: DefId(2),
                        name: name_y,
                        ty: textus,
                        mode: radix_hir::HirParamMode::Owned,
                        optional: true,
                        sponte: true,
                        default: Some(HirExpression {
                            id: HirId(20),
                            kind: HirExpressionKind::Literal(HirLiteral::String(
                                interner.intern("default"),
                            )),
                            ty: Some(textus),
                            span: span(),
                        }),
                        span: span(),
                    },
                ],
                Some(textus),
                None,
                Box::new(HirExpression {
                    id: HirId(30),
                    kind: HirExpressionKind::Block(HirBlock {
                        statements: vec![],
                        expr: Some(Box::new(HirExpression {
                            id: HirId(31),
                            kind: HirExpressionKind::Path(DefId(1)),
                            ty: Some(textus),
                            span: span(),
                        })),
                        span: span(),
                        ..Default::default()
                    }),
                    ty: Some(textus),
                    span: span(),
                }),
            ),
            ty: Some(textus),
            span: span(),
        };

        let function = HirFunction {
            cli_args: None,
            name: name_f,
            type_params: Vec::new(),
            params: Vec::new(),
            ret_ty: Some(textus),
            err_ty: None,
            body: Some(HirBlock {
                statements: vec![HirStatement {
                    id: HirId(22),
                    kind: HirStatementKind::Expr(closure),
                    span: span(),
                }],
                expr: None,
                span: span(),
                ..Default::default()
            }),
            posture: radix_syntax::CallablePosture::SyncFinite,
            is_async: false,
            is_generator: false,
            is_kernel: false,
            nucleum_fragment: false,
            is_vertex: false,
            is_fragment: false,
            annotations: Vec::new(),
            test: None,
        };

        let program = HirModule {
            items: vec![HirItem {
                id: HirId(0),
                def_id: DefId(radix_types::USER_DEF_ID_BASE),
                kind: HirItemKind::Function(Box::new(function)),
                span: span(),
            }],
            entry: None,
            entry_annotations: Vec::new(),
            entry_args_name: None,
            entry_is_async: false,
        };

        let code = generate_faber(&program, &types, &interner);
        assert!(
            code.contains("sponte"),
            "closure param with sponte must emit 'sponte', got:\n{code}"
        );
        assert!(
            code.contains("vel"),
            "closure param with default must emit 'vel', got:\n{code}"
        );
        assert!(
            code.contains("default"),
            "closure param default value must appear, got:\n{code}"
        );
    }

    // ── Emitter convert-defect regressions (VM-U3 / gradus S1) ──────────────

    #[test]
    fn import_privata_emits_no_visibility_keyword() {
        // The parser retired explicit import `privata` (VM-U3); the canonical
        // emitter must not re-emit it. `faber convert` on gradus emitted
        // `import from "gradus:dtype" private dtype`, which fails re-parse.
        let mut interner = Interner::new();
        let path_sym = interner.intern("gradus:dtype");
        let name = interner.intern("dtype");

        let types = TypeTable::new();

        let import = HirImport {
            path: path_sym,
            visibility: radix_syntax::Visibility::Privata,
            items: vec![HirImportItem {
                def_id: DefId(1),
                name,
                alias: None,
            }],
        };

        let program = HirModule {
            items: vec![HirItem {
                id: HirId(0),
                def_id: DefId(radix_types::USER_DEF_ID_BASE),
                kind: HirItemKind::Import(import),
                span: span(),
            }],
            entry: None,
            entry_annotations: Vec::new(),
            entry_args_name: None,
            entry_is_async: false,
        };

        let code = generate_faber(&program, &types, &interner);
        assert_eq!(
            code.trim(),
            r#"importa ex "gradus:dtype" dtype"#,
            "private import must emit without a visibility keyword"
        );
        assert!(
            !code.contains("privata"),
            "import must not re-emit the retired `private` visibility: {code}"
        );
    }

    #[test]
    fn union_last_variant_has_no_trailing_comma() {
        // Canonical union layout separates variants with a comma; the last
        // variant must not carry one (PARSE030 on re-parse of the converted
        // output under the source locale).
        let mut interner = Interner::new();
        let types = TypeTable::new();

        let enum_item = HirEnum {
            name: interner.intern("DType"),
            type_params: Vec::new(),
            annotations: Vec::new(),
            variants: vec![
                HirVariant {
                    def_id: DefId(1),
                    name: interner.intern("F32"),
                    fields: vec![],
                    span: span(),
                },
                HirVariant {
                    def_id: DefId(2),
                    name: interner.intern("F16"),
                    fields: vec![],
                    span: span(),
                },
            ],
        };

        let program = HirModule {
            items: vec![HirItem {
                id: HirId(0),
                def_id: DefId(radix_types::USER_DEF_ID_BASE),
                kind: HirItemKind::Enum(enum_item),
                span: span(),
            }],
            entry: None,
            entry_annotations: Vec::new(),
            entry_args_name: None,
            entry_is_async: false,
        };

        let code = generate_faber(&program, &types, &interner);
        assert!(
            code.contains("F32,\n    F16\n"),
            "earlier variants keep the comma separator, last carries none:\n{code}"
        );
        assert!(
            !code.contains("F16,\n"),
            "last variant must not carry a trailing comma:\n{code}"
        );
    }

    #[test]
    fn struct_publica_annotation_reemits() {
        // S1-U2 convert defect: the canonical emitter dropped `@ publica` on
        // `genus` declarations (dtype 16->14 publica decls through convert).
        // Item-level annotations must re-emit ahead of the declaration so the
        // converted module keeps its export tier.
        let mut interner = Interner::new();
        let types = TypeTable::new();
        let publica = interner.intern("publica");

        let struct_item = HirItem {
            id: HirId(0),
            def_id: DefId(radix_types::USER_DEF_ID_BASE),
            kind: HirItemKind::Struct(HirStruct {
                name: interner.intern("Artifactus"),
                type_params: Vec::new(),
                annotations: vec![HirAnnotation::with_fields(publica, Vec::new(), span())],
                fields: Vec::new(),
                methods: Vec::new(),
                extends: None,
                implements: Vec::new(),
            }),
            span: span(),
        };

        let program = HirModule {
            items: vec![struct_item],
            entry: None,
            entry_annotations: Vec::new(),
            entry_args_name: None,
            entry_is_async: false,
        };

        let code = generate_faber(&program, &types, &interner);
        let publica_line = code
            .lines()
            .find(|line| line.contains("publica"))
            .unwrap_or_else(|| panic!("struct must re-emit its @ publica annotation:\n{code}"));
        let struct_line = code
            .lines()
            .position(|line| line.contains("genus Artifactus"))
            .expect("struct declaration must be emitted");
        assert!(
            code.lines().position(|line| line == publica_line).unwrap() < struct_line,
            "@ publica must precede the struct declaration:\n{code}"
        );
    }

    #[test]
    fn enum_publica_annotation_reemits() {
        // S1-U2 convert defect: the canonical emitter dropped `@ publica` on
        // `discretio` declarations. `HirEnum` now carries item annotations so
        // the converted module keeps its export tier.
        let mut interner = Interner::new();
        let types = TypeTable::new();
        let publica = interner.intern("publica");

        let enum_item = HirItem {
            id: HirId(0),
            def_id: DefId(radix_types::USER_DEF_ID_BASE),
            kind: HirItemKind::Enum(HirEnum {
                name: interner.intern("DType"),
                type_params: Vec::new(),
                annotations: vec![HirAnnotation::with_fields(publica, Vec::new(), span())],
                variants: vec![HirVariant {
                    def_id: DefId(1),
                    name: interner.intern("F32"),
                    fields: vec![],
                    span: span(),
                }],
            }),
            span: span(),
        };

        let program = HirModule {
            items: vec![enum_item],
            entry: None,
            entry_annotations: Vec::new(),
            entry_args_name: None,
            entry_is_async: false,
        };

        let code = generate_faber(&program, &types, &interner);
        let publica_line = code
            .lines()
            .find(|line| line.contains("publica"))
            .unwrap_or_else(|| panic!("union must re-emit its @ publica annotation:\n{code}"));
        let discretio_line = code
            .lines()
            .position(|line| line.contains("discretio DType"))
            .expect("union declaration must be emitted");
        assert!(
            code.lines().position(|line| line == publica_line).unwrap() < discretio_line,
            "@ publica must precede the union declaration:\n{code}"
        );
    }

    #[test]
    fn imported_variant_pattern_uses_qualified_spelling() {
        // S1-U2 convert defect: a `discerne` arm matching an imported union
        // variant (`manifestum.Cognita`) re-emitted as the `def_N` fallback,
        // which the converted module re-parse rejected with SEM001. The
        // import contract's qualified spelling must win.
        let mut codegen = FaberCodegen::default();
        let names = FxHashMap::default();
        let interner = Interner::new();
        let surface = LatinSurface;
        let mut writer = radix_codegen_shared::CodeWriter::new();
        let variant_def = DefId(0x9000_0001);
        codegen
            .imported_def_spellings
            .insert(variant_def, "manifestum.Cognita".to_owned());
        codegen.write_pattern(
            &HirPattern::Variant(variant_def, Vec::new()),
            &names,
            &interner,
            &surface,
            &mut writer,
        );
        assert_eq!(
            writer.as_str(),
            "manifestum.Cognita",
            "imported variant pattern must use the qualified spelling, not def_N"
        );
    }

    #[test]
    fn call_to_local_variant_reemits_finge_construction() {
        // `finge F32` lowers to `Call(Path(variant_def), [], [])`; the emitter
        // must recover the `finge` keyword. A bare `F32()` re-emit is a retired
        // type-call constructor (PARSE001).
        let mut interner = Interner::new();
        let types = TypeTable::new();

        let variant_def = DefId(7);
        let enum_item = HirEnum {
            name: interner.intern("DType"),
            type_params: Vec::new(),
            annotations: Vec::new(),
            variants: vec![HirVariant {
                def_id: variant_def,
                name: interner.intern("F32"),
                fields: vec![],
                span: span(),
            }],
        };

        let function = HirFunction {
            cli_args: None,
            name: interner.intern("f"),
            type_params: Vec::new(),
            params: Vec::new(),
            ret_ty: None,
            err_ty: None,
            body: Some(HirBlock {
                statements: vec![HirStatement {
                    id: HirId(2),
                    kind: HirStatementKind::Redde(Some(HirExpression {
                        id: HirId(3),
                        kind: HirExpressionKind::Call(
                            Box::new(HirExpression {
                                id: HirId(4),
                                kind: HirExpressionKind::Path(variant_def),
                                ty: None,
                                span: span(),
                            }),
                            Vec::new(),
                            vec![],
                        ),
                        ty: None,
                        span: span(),
                    })),
                    span: span(),
                }],
                expr: None,
                span: span(),
                ..Default::default()
            }),
            posture: radix_syntax::CallablePosture::SyncFinite,
            is_async: false,
            is_generator: false,
            is_kernel: false,
            nucleum_fragment: false,
            is_vertex: false,
            is_fragment: false,
            annotations: Vec::new(),
            test: None,
        };

        let program = HirModule {
            items: vec![
                HirItem {
                    id: HirId(0),
                    def_id: DefId(radix_types::USER_DEF_ID_BASE),
                    kind: HirItemKind::Enum(enum_item),
                    span: span(),
                },
                HirItem {
                    id: HirId(1),
                    def_id: DefId(radix_types::USER_DEF_ID_BASE + 1),
                    kind: HirItemKind::Function(Box::new(function)),
                    span: span(),
                },
            ],
            entry: None,
            entry_annotations: Vec::new(),
            entry_args_name: None,
            entry_is_async: false,
        };

        let code = generate_faber(&program, &types, &interner);
        assert!(
            code.contains("redde finge F32"),
            "variant construction must re-emit the `finge` keyword:\n{code}"
        );
        assert!(
            !code.contains("F32()"),
            "variant construction must not re-emit as a retired type call:\n{code}"
        );
    }

    #[test]
    fn finge_with_named_fields_reemits_braced_fields() {
        let mut interner = Interner::new();
        let types = TypeTable::new();

        let variant_def = DefId(7);
        let causa = interner.intern("causa");
        let enum_item = HirEnum {
            name: interner.intern("TensorError"),
            type_params: Vec::new(),
            annotations: Vec::new(),
            variants: vec![HirVariant {
                def_id: variant_def,
                name: interner.intern("FormaInvalida"),
                fields: vec![],
                span: span(),
            }],
        };

        let function = HirFunction {
            cli_args: None,
            name: interner.intern("f"),
            type_params: Vec::new(),
            params: Vec::new(),
            ret_ty: None,
            err_ty: None,
            body: Some(HirBlock {
                statements: vec![HirStatement {
                    id: HirId(2),
                    kind: HirStatementKind::Expr(HirExpression {
                        id: HirId(3),
                        kind: HirExpressionKind::Throw(Box::new(HirExpression {
                            id: HirId(30),
                            kind: HirExpressionKind::Call(
                                Box::new(HirExpression {
                                    id: HirId(4),
                                    kind: HirExpressionKind::Path(variant_def),
                                    ty: None,
                                    span: span(),
                                }),
                                Vec::new(),
                                vec![HirCallArg {
                                    name: Some(causa),
                                    spread: false,
                                    expr: HirExpression {
                                        id: HirId(5),
                                        kind: HirExpressionKind::Path(DefId(9)),
                                        ty: None,
                                        span: span(),
                                    },
                                    span: span(),
                                }],
                            ),
                            ty: None,
                            span: span(),
                        })),
                        ty: None,
                        span: span(),
                    }),
                    span: span(),
                }],
                expr: None,
                span: span(),
                ..Default::default()
            }),
            posture: radix_syntax::CallablePosture::SyncFinite,
            is_async: false,
            is_generator: false,
            is_kernel: false,
            nucleum_fragment: false,
            is_vertex: false,
            is_fragment: false,
            annotations: Vec::new(),
            test: None,
        };

        let program = HirModule {
            items: vec![
                HirItem {
                    id: HirId(0),
                    def_id: DefId(radix_types::USER_DEF_ID_BASE),
                    kind: HirItemKind::Enum(enum_item),
                    span: span(),
                },
                HirItem {
                    id: HirId(1),
                    def_id: DefId(radix_types::USER_DEF_ID_BASE + 1),
                    kind: HirItemKind::Function(Box::new(function)),
                    span: span(),
                },
            ],
            entry: None,
            entry_annotations: Vec::new(),
            entry_args_name: None,
            entry_is_async: false,
        };

        let code = generate_faber(&program, &types, &interner);
        assert!(
            code.contains("iace finge FormaInvalida {causa = def_9}"),
            "named finge fields must re-emit in the braced form:\n{code}"
        );
    }

    /// Probe surface that localizes one intrinsic canonical name.
    struct IntrinsicProbeSurface;

    impl KeywordSurface for IntrinsicProbeSurface {
        fn keyword(&self, kind: TokenKind) -> &'static str {
            match kind {
                TokenKind::Finge => "variant",
                _ => LatinSurface.keyword(kind),
            }
        }

        fn intrinsic_surface(&self, canonical: &str) -> Option<String> {
            (canonical == "divide").then(|| "split".to_owned())
        }

        fn type_surface(&self, kind: TypeBindingKind) -> &'static str {
            LatinSurface.type_surface(kind)
        }
    }

    #[test]
    fn intrinsic_method_call_localizes_through_surface() {
        // HIR method symbols are canonical registry spellings; under a
        // non-`la` reader pack the canonical Latin name is a compile error on
        // re-parse. The emitter must project them through the target pack's
        // intrinsic surface rows — for genuine intrinsic-method receivers
        // (a collection, tensor, or primitive value); nominal and module-
        // import receivers keep the canonical symbol.
        let mut interner = Interner::new();
        let types = TypeTable::new();
        let textus = types.primitive(Primitive::Textus);
        let receiver_def = DefId(5);

        let body = HirBlock {
            statements: vec![HirStatement {
                id: HirId(2),
                kind: HirStatementKind::Expr(HirExpression {
                    id: HirId(3),
                    kind: HirExpressionKind::MethodCall(
                        Box::new(HirExpression {
                            id: HirId(4),
                            kind: HirExpressionKind::Path(receiver_def),
                            ty: Some(textus),
                            span: span(),
                        }),
                        interner.intern("divide"),
                        Vec::new(),
                        vec![HirCallArg {
                            name: None,
                            spread: false,
                            expr: HirExpression {
                                id: HirId(5),
                                kind: HirExpressionKind::Literal(HirLiteral::String(
                                    interner.intern("/"),
                                )),
                                ty: None,
                                span: span(),
                            },
                            span: span(),
                        }],
                    ),
                    ty: None,
                    span: span(),
                }),
                span: span(),
            }],
            expr: None,
            span: span(),
            ..Default::default()
        };

        let function = HirFunction {
            cli_args: None,
            name: interner.intern("f"),
            type_params: Vec::new(),
            params: Vec::new(),
            ret_ty: None,
            err_ty: None,
            body: Some(body),
            posture: radix_syntax::CallablePosture::SyncFinite,
            is_async: false,
            is_generator: false,
            is_kernel: false,
            nucleum_fragment: false,
            is_vertex: false,
            is_fragment: false,
            annotations: Vec::new(),
            test: None,
        };

        let program = HirModule {
            items: vec![HirItem {
                id: HirId(0),
                def_id: DefId(radix_types::USER_DEF_ID_BASE),
                kind: HirItemKind::Function(Box::new(function)),
                span: span(),
            }],
            entry: None,
            entry_annotations: Vec::new(),
            entry_args_name: None,
            entry_is_async: false,
        };

        let code = FaberCodegen::default()
            .generate(
                &program,
                &empty_presentation(),
                &types,
                &interner,
                &IntrinsicProbeSurface,
            )
            .expect("codegen")
            .code;
        assert!(
            code.contains(".split("),
            "canonical intrinsic must localize through the surface:\n{code}"
        );
    }

    #[test]
    fn cape_handler_keeps_leading_author_local() {
        // A `cape err { fixum textus c ← …; … }` handler whose first authored
        // statement is a real local must keep it: the binding-local skip only
        // applies to the synthetic runtime-provided binding.
        let mut interner = Interner::new();
        let types = TypeTable::new();
        let textus = types.primitive(Primitive::Textus);

        let err = interner.intern("err");
        let c = interner.intern("c");
        let catch = HirCape {
            binding_def_id: DefId(100),
            binding_name: err,
            binding_ty: Some(textus),
            body: HirBlock {
                statements: vec![
                    HirStatement {
                        id: HirId(40),
                        kind: HirStatementKind::Local(HirLocal {
                            def_id: DefId(101),
                            name: c,
                            ty: Some(textus),
                            init: Some(HirExpression {
                                id: HirId(41),
                                kind: HirExpressionKind::Literal(HirLiteral::String(
                                    interner.intern("x"),
                                )),
                                ty: Some(textus),
                                span: span(),
                            }),
                            mutable: false,
                            is_await: false,
                            runtime_provided: false,
                        }),
                        span: span(),
                    },
                    HirStatement {
                        id: HirId(42),
                        kind: HirStatementKind::Redde(Some(HirExpression {
                            id: HirId(43),
                            kind: HirExpressionKind::Path(DefId(101)),
                            ty: Some(textus),
                            span: span(),
                        })),
                        span: span(),
                    },
                ],
                expr: None,
                span: span(),
                ..Default::default()
            },
            span: span(),
        };

        let expr = HirExpression {
            id: HirId(10),
            kind: HirExpressionKind::Handled {
                body: HirBlock {
                    statements: vec![],
                    expr: Some(Box::new(HirExpression {
                        id: HirId(11),
                        kind: HirExpressionKind::Literal(HirLiteral::String(interner.intern("ok"))),
                        ty: Some(textus),
                        span: span(),
                    })),
                    span: span(),
                    ..Default::default()
                },
                catch: Box::new(catch),
            },
            ty: Some(textus),
            span: span(),
        };

        let function = HirFunction {
            cli_args: None,
            name: interner.intern("f"),
            type_params: Vec::new(),
            params: Vec::new(),
            ret_ty: Some(textus),
            err_ty: None,
            body: Some(HirBlock {
                statements: vec![HirStatement {
                    id: HirId(20),
                    kind: HirStatementKind::Expr(expr),
                    span: span(),
                }],
                expr: None,
                span: span(),
                ..Default::default()
            }),
            posture: radix_syntax::CallablePosture::SyncFinite,
            is_async: false,
            is_generator: false,
            is_kernel: false,
            nucleum_fragment: false,
            is_vertex: false,
            is_fragment: false,
            annotations: Vec::new(),
            test: None,
        };

        let program = HirModule {
            items: vec![HirItem {
                id: HirId(0),
                def_id: DefId(radix_types::USER_DEF_ID_BASE),
                kind: HirItemKind::Function(Box::new(function)),
                span: span(),
            }],
            entry: None,
            entry_annotations: Vec::new(),
            entry_args_name: None,
            entry_is_async: false,
        };

        let code = generate_faber(&program, &types, &interner);
        assert!(
            code.contains("cape err {") && code.contains("fixum textus c ← \"x\""),
            "cape handler must keep its leading authored local:\n{code}"
        );
    }
}
