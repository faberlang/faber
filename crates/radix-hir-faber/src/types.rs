//! Canonical Faber type emission.
//!
//! The Faber backend prints semantic [`Type`] values back into source-level
//! type syntax. This is a grammar boundary: declarations elsewhere in the
//! backend rely on this module to produce type-first Faber spellings such as
//! `textus`, `lista<T>`, references as `de T`/`in T`, and function types with
//! Faber arrows.

use crate::surface::{KeywordSurface, TypeBindingKind};
use handroll::FxHashMap;
use radix_codegen_shared::CodegenError;
use radix_diagnostics::DiagnosticArg;
use radix_hir::{
    DefId, HirRangeArgument, HirRangeBound, HirTypeForm, HirTypeFormArg, HirTypeFormHead,
    HirTypeSite,
};
use radix_lexer::{Interner, Symbol, TokenKind};
use radix_syntax::TypeFormLevel;
use radix_types::{
    BitsWidth, CollectionKind, IndexExpr, IndexId, MarkerFamily, Primitive, SemanticMutability,
    SemanticParamMode, TensorFamilyKind, Type, TypeId,
};

impl super::FaberCodegen<'_> {
    /// Strip nested semantic option wrappers before printing nullable syntax.
    pub(super) fn flatten_option(&self, mut type_id: TypeId) -> TypeId {
        while let Type::Option(inner) = self.types.get(type_id) {
            type_id = *inner;
        }
        type_id
    }

    /// Marker word an exclusive (`own`/`copy`) binding type writes on the
    /// value side of its initializer (`T b ← copy a`).
    pub(super) fn exclusive_binding_marker(&self, ty: TypeId) -> Option<radix_lexer::TokenKind> {
        match self.types.get(ty) {
            Type::Ref(SemanticMutability::Own, _) => Some(radix_lexer::TokenKind::Own),
            Type::Ref(SemanticMutability::Copy, _) => Some(radix_lexer::TokenKind::Copy),
            _ => None,
        }
    }

    pub(super) fn type_to_faber(
        &self,
        type_id: TypeId,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
    ) -> String {
        let ty = self.types.get(type_id);

        match ty {
            // Ruling d477c76d retires the `octeti`/`bytes` word: a raw-bit
            // buffer is a list of `b8`, bounded or not.
            Type::Primitive(Primitive::Octeti) => format!(
                "{}<{}>",
                surface.type_surface(TypeBindingKind::Collection(CollectionKind::Lista)),
                BitsWidth::B8.faber_marker()
            ),
            Type::Primitive(prim) => surface
                .type_surface(TypeBindingKind::Primitive(*prim))
                .to_owned(),
            Type::TensorEdgePolicy(policy) => surface
                .type_surface(TypeBindingKind::TensorEdgePolicy(*policy))
                .to_owned(),

            Type::SizedInstans(precision) => format!(
                "{}<{}>",
                surface.type_surface(TypeBindingKind::Primitive(Primitive::Instans)),
                precision.faber_marker()
            ),

            Type::Array(elem) => self.format_unary_type(
                surface.type_surface(TypeBindingKind::Collection(CollectionKind::Lista)),
                *elem,
                names,
                surface,
            ),

            Type::Queue(elem) => self.format_unary_type(
                surface.type_surface(TypeBindingKind::Collection(CollectionKind::Queue)),
                *elem,
                names,
                surface,
            ),

            Type::Stack(elem) => self.format_unary_type(
                surface.type_surface(TypeBindingKind::Collection(CollectionKind::Stack)),
                *elem,
                names,
                surface,
            ),

            Type::Map(key, value) => self.format_binary_type(
                surface.type_surface(TypeBindingKind::Collection(CollectionKind::Tabula)),
                *key,
                *value,
                names,
                surface,
            ),

            Type::Ledger(key, value) => self.format_binary_type(
                surface.type_surface(TypeBindingKind::Collection(CollectionKind::Tabulae)),
                *key,
                *value,
                names,
                surface,
            ),

            Type::Record(fields) => self.format_record_type(fields, names, surface),

            Type::Set(elem) => self.format_unary_type(
                surface.type_surface(TypeBindingKind::Collection(CollectionKind::Copia)),
                *elem,
                names,
                surface,
            ),

            Type::Promissum(inner) => self.format_unary_type(
                surface.type_surface(TypeBindingKind::Collection(CollectionKind::Promissum)),
                *inner,
                names,
                surface,
            ),

            Type::PromissumFailable(success, alternate) => format!(
                "{}<{} ⇥ {}>",
                surface.type_surface(TypeBindingKind::Collection(CollectionKind::Promissum)),
                self.type_to_faber(*success, names, surface),
                self.type_to_faber(*alternate, names, surface)
            ),

            Type::Cursor(inner) => self.format_unary_type(
                surface.type_surface(TypeBindingKind::Collection(CollectionKind::Cursor)),
                *inner,
                names,
                surface,
            ),

            Type::AsyncCursor(item, alternate) => format!(
                "async cursor<{} ⇥ {}>",
                self.type_to_faber(*item, names, surface),
                self.type_to_faber(*alternate, names, surface)
            ),

            Type::Tensor(elem, shape) => self.format_tensor_family_type(
                TensorFamilyKind::Tensor,
                *elem,
                *shape,
                names,
                surface,
            ),

            Type::Vector(elem, width) => self.format_tensor_family_type(
                TensorFamilyKind::Vector,
                *elem,
                *width,
                names,
                surface,
            ),

            Type::Matrix(elem, shape) => self.format_tensor_family_type(
                TensorFamilyKind::Matrix,
                *elem,
                *shape,
                names,
                surface,
            ),

            Type::Sparsa(elem, shape) => self.format_tensor_family_type(
                TensorFamilyKind::Sparsa,
                *elem,
                *shape,
                names,
                surface,
            ),

            Type::Atomic(elem) => self.format_unary_type(
                surface.type_surface(TypeBindingKind::TensorFamily(TensorFamilyKind::Atomic)),
                *elem,
                names,
                surface,
            ),

            Type::Intervallum(bound, step) => {
                let head = surface
                    .type_surface(TypeBindingKind::TensorFamily(TensorFamilyKind::Intervallum));
                match self.types.get_index(*step) {
                    // Step 1 is the canonical silence: both a bare interval
                    // and an explicit `1` re-emit as `intervallum<T>`.
                    IndexExpr::Literal(value) if *value != 1 => format!(
                        "{}<{}, {}>",
                        head,
                        self.type_to_faber(*bound, names, surface),
                        value
                    ),
                    _ => self.format_unary_type(head, *bound, names, surface),
                }
            }

            // Bare width marker (`u8`, `f32`, `d64`, `inf`): valid in every
            // locale and every type position. The wrapped `numerus<u8>` /
            // `fractus<f32>` spellings are retired (N7c); the policy words
            // (`trapping`, `wrapping`, `saturating`) keep their own heads.
            Type::SizedNumeric(prim, width) => match prim {
                Primitive::Numerus | Primitive::Fractus => width.faber_marker().to_owned(),
                _ => surface
                    .type_surface(TypeBindingKind::Primitive(Primitive::Ignotum))
                    .to_owned(),
            },

            Type::ModularWord(width) => format!(
                "{}<{}>",
                surface.contextual_keyword("modulus"),
                width.faber_marker()
            ),

            // A bare, locale-neutral marker like `u8`.
            Type::Bits(width) => width.faber_marker().to_owned(),

            Type::SaturatingWord(width) => format!(
                "{}<{}>",
                surface.type_surface(TypeBindingKind::SaturatingWord),
                width.faber_marker()
            ),

            Type::RangedInt {
                policy,
                width,
                lo,
                hi,
            } => Self::ranged_int_to_faber(*policy, *width, *lo, *hi, surface),

            Type::PolicyFloat(policy, width) => format!(
                "{}<{}>",
                surface.type_surface(policy_word_binding(*policy)),
                width.faber_marker()
            ),

            Type::Option(inner) => {
                let inner = self.type_to_faber(self.flatten_option(*inner), names, surface);
                // Nullable type: both members are type positions, so the null
                // constituent uses the null *type* surface (D23.2: en `none`,
                // la `nihil`), never the value literal.
                format!(
                    "{inner} ∪ {}",
                    surface.type_surface(TypeBindingKind::Primitive(Primitive::Nihil))
                )
            }

            Type::Ref(mutability, inner) => {
                let prefix = match mutability {
                    SemanticMutability::Immutable => surface.keyword(radix_lexer::TokenKind::De),
                    SemanticMutability::Mutable => surface.keyword(radix_lexer::TokenKind::In),
                    SemanticMutability::Own => surface.keyword(radix_lexer::TokenKind::Own),
                    SemanticMutability::Copy => surface.keyword(radix_lexer::TokenKind::Copy),
                };
                format!("{} {}", prefix, self.type_to_faber(*inner, names, surface))
            }

            Type::Struct(def_id) if let Some(frame) = self.builtin_frame_family(*def_id, names) => {
                surface
                    .type_surface(TypeBindingKind::Frame(frame))
                    .to_owned()
            }
            Type::Struct(def_id) if self.builtin_erratum_carrier(*def_id, names).is_some() => {
                surface.type_surface(TypeBindingKind::Erratum).to_owned()
            }
            Type::Struct(def_id) | Type::Enum(def_id) | Type::Interface(def_id) => self
                .imported_type_spellings
                .get(&type_id)
                .cloned()
                .unwrap_or_else(|| self.name_for_def(*def_id, names)),

            Type::Alias(def_id, resolved) => names.get(def_id).map_or_else(
                || self.type_to_faber(*resolved, names, surface),
                |sym| self.symbol_to_string(*sym),
            ),

            Type::Func(sig) => {
                let params: Vec<String> = sig
                    .params
                    .iter()
                    .map(|p| {
                        let ty = self.type_to_faber(p.ty, names, surface);
                        // A type-position marker already rides the param type
                        // (`de T` / `in T`); synthesize that marker only when
                        // an imported signature carries the mode separately.
                        let type_carries_mode = matches!(self.types.get(p.ty), Type::Ref(..));
                        match p.mode {
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
                let ret = self.type_to_faber(sig.ret, names, surface);
                if let Some(err) = sig.err {
                    let err = self.type_to_faber(err, names, surface);
                    format!("({}) → {} ⇥ {}", params.join(", "), ret, err)
                } else {
                    format!("({}) → {}", params.join(", "), ret)
                }
            }

            Type::Param(sym) => self.symbol_to_string(*sym),

            Type::Applied(base, args) => {
                let base_str = self.type_to_faber(*base, names, surface);
                let args_str: Vec<String> = args
                    .iter()
                    .map(|arg| match arg {
                        radix_types::AppliedArg::Type(id) => {
                            self.type_to_faber(*id, names, surface)
                        }
                        radix_types::AppliedArg::Size(id) => self.format_figura(*id, surface),
                    })
                    .collect();
                format!("{}<{}>", base_str, args_str.join(", "))
            }

            Type::Infer(_) => "_".to_owned(),
            Type::InferUnion(_) => "∪".to_owned(),
            Type::MarkerHole { family, .. } => {
                let head = match family {
                    MarkerFamily::Modulus => surface.contextual_keyword("modulus"),
                    MarkerFamily::Saturatus => {
                        surface.type_surface(TypeBindingKind::SaturatingWord)
                    }
                    MarkerFamily::Instans => {
                        surface.type_surface(TypeBindingKind::Primitive(Primitive::Instans))
                    }
                };
                format!("{head}<_>")
            }
            Type::Tuple(members) => {
                let members = members
                    .iter()
                    .map(|member| self.type_to_faber(*member, names, surface))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{}<{members}>", surface.contextual_keyword("iuncta"))
            }
            Type::Union(members) => self.format_union_type(members, names, surface),
            Type::Intersection(members) => members
                .iter()
                .map(|member| self.type_to_faber(*member, names, surface))
                .collect::<Vec<_>>()
                .join(" ∩ "),
            Type::Error => "<error>".to_owned(),
            Type::BoundedArray { element, capacity } => format!(
                "{}<{}, {}>",
                surface.type_surface(TypeBindingKind::Collection(CollectionKind::Lista)),
                self.type_to_faber(*element, names, surface),
                self.list_count_to_faber(*capacity, surface)
            ),
            Type::RangedArray { element, min, max } => format!(
                "{}<{}, {}>",
                surface.type_surface(TypeBindingKind::Collection(CollectionKind::Lista)),
                self.type_to_faber(*element, names, surface),
                Self::list_range_to_faber(*min, *max, surface)
            ),
            Type::BoundedQueue { element, capacity } => format!(
                "{}<{}, {}>",
                surface.type_surface(TypeBindingKind::Collection(CollectionKind::Queue)),
                self.type_to_faber(*element, names, surface),
                self.list_count_to_faber(*capacity, surface)
            ),
            Type::BoundedStack { element, capacity } => format!(
                "{}<{}, {}>",
                surface.type_surface(TypeBindingKind::Collection(CollectionKind::Stack)),
                self.type_to_faber(*element, names, surface),
                self.list_count_to_faber(*capacity, surface)
            ),
            Type::BoundedTextus { capacity } => {
                self.bounded_primitive_head(Primitive::Textus, *capacity, surface)
            }
            Type::BoundedAscii { capacity } => {
                self.bounded_primitive_head(Primitive::Ascii, *capacity, surface)
            }
            Type::BoundedOcteti { capacity } => format!(
                "{}<{}, {}>",
                surface.type_surface(TypeBindingKind::Collection(CollectionKind::Lista)),
                BitsWidth::B8.faber_marker(),
                self.list_count_to_faber(*capacity, surface)
            ),
        }
    }

    /// Site-aware type emission: the authored form recorded at `site`, or the
    /// canonical spelling when no form exists there (synthesized/hand-built
    /// HIR).
    ///
    /// Policy (goal §8): a present form whose root witness differs from
    /// `type_id`, carries a level illegal for its constructor, or has the
    /// wrong child arity/kind fails closed with [`CodegenError`] — it is
    /// never guessed around or silently re-spelled.
    pub(super) fn type_to_faber_at(
        &self,
        site: &HirTypeSite,
        type_id: TypeId,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
    ) -> Result<String, CodegenError> {
        let Some(form) = self.authored_type_forms.get(site) else {
            return Ok(self.type_to_faber(type_id, names, surface));
        };
        if form.ty != type_id {
            return Err(CodegenError {
                message: format!(
                    "authored type form witness TypeId({}) does not match the site's type TypeId({})",
                    form.ty.0, type_id.0
                ),
                args: vec![DiagnosticArg::new(
                    "issue",
                    "authored_type_form_witness_mismatch",
                )],
            });
        }
        self.type_form_to_faber(form, names, surface)
    }

    /// Render one authored type-form node through the target keyword surface.
    fn type_form_to_faber(
        &self,
        form: &HirTypeForm,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
    ) -> Result<String, CodegenError> {
        match &form.head {
            HirTypeFormHead::Infer => {
                Self::expect_no_form_args(form)?;
                Ok("_".to_owned())
            }
            HirTypeFormHead::UnionHole => {
                Self::expect_no_form_args(form)?;
                Ok("∪".to_owned())
            }
            HirTypeFormHead::Named { def_id, path } => {
                self.named_form_to_faber(form, *def_id, path, names, surface)
            }
            HirTypeFormHead::ImportedAlias { path } => {
                Self::expect_no_form_args(form)?;
                Ok(path
                    .iter()
                    .map(|segment| self.interner.resolve(*segment))
                    .collect::<Vec<_>>()
                    .join("."))
            }
            HirTypeFormHead::Semantic(level) => {
                self.semantic_form_to_faber(form, *level, names, surface)
            }
        }
    }

    /// Render a user-defined alias/nominal head: the retained source path plus
    /// its own authored argument list, which is intentionally independent of
    /// the canonical expansion's child list. Equivalence of the instantiation
    /// beyond def identity is FHIR/revalidation territory (goal §9/§10).
    fn named_form_to_faber(
        &self,
        form: &HirTypeForm,
        def_id: DefId,
        path: &[Symbol],
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
    ) -> Result<String, CodegenError> {
        match self.types.get(form.ty) {
            // A def-carrying witness that resolves elsewhere means the head
            // names an unrelated definition.
            Type::Struct(id) | Type::Enum(id) | Type::Interface(id) | Type::Alias(id, _)
                if *id != def_id =>
            {
                return Err(malformed_type_form(format!(
                    "named type form spells def DefId({}) but its witness resolves DefId({})",
                    def_id.0, id.0
                )));
            }
            Type::Param(symbol) if path.last() != Some(symbol) => {
                return Err(malformed_type_form(
                    "named type form spells a different generic parameter than its witness"
                        .to_owned(),
                ));
            }
            _ => {}
        }
        let head = match self.builtin_frame_family(def_id, names) {
            Some(frame) => surface
                .type_surface(TypeBindingKind::Frame(frame))
                .to_owned(),
            None => path
                .iter()
                .map(|segment| self.interner.resolve(*segment))
                .collect::<Vec<_>>()
                .join("."),
        };
        if form.args.is_empty() {
            return Ok(head);
        }
        let args = self.type_form_args_to_faber(&form.args, names, surface)?;
        Ok(format!("{head}<{}>", args.join(", ")))
    }

    /// Constructor/level spelling registry for `Semantic` heads.
    ///
    /// LONG renders the explicit head through the target surface, recursing
    /// into form-tree children. SHORT renders the bare width marker (sized
    /// numerics) or the fused `l/t/v/m/s` prefix form (lista/tensor/vector/
    /// matrix/sparsa) over the accepted `numeric_width` vocabulary; fused
    /// prefixes and width markers are locale-independent tokens. Anything
    /// else — unknown levels, SHORT on constructors without a live
    /// compression form — fails closed.
    fn semantic_form_to_faber(
        &self,
        form: &HirTypeForm,
        level: TypeFormLevel,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
    ) -> Result<String, CodegenError> {
        match level.0 {
            0 => self.semantic_long_form_to_faber(form, names, surface),
            1 => self.semantic_short_form_to_faber(form, surface),
            2 => self.semantic_policy_form_to_faber(form, surface),
            3 => self.semantic_inf_word_form_to_faber(form, TypeBindingKind::ModularWord, surface),
            4 => {
                self.semantic_inf_word_form_to_faber(form, TypeBindingKind::SaturatingWord, surface)
            }
            _ => Err(malformed_type_form(format!(
                "authored type form carries unsupported level {}",
                level.0
            ))),
        }
    }

    /// POLICY spelling (D11.8): the explicit default-policy head over a
    /// sized numeric witness, `trapping<u8>` / la `exactus<u8>`. The witness
    /// is the bare marker's own type, so only integer and `d64` widths carry it.
    fn semantic_policy_form_to_faber(
        &self,
        form: &HirTypeForm,
        surface: &impl KeywordSurface,
    ) -> Result<String, CodegenError> {
        match self.types.get(form.ty) {
            Type::Error => Ok("<error>".to_owned()),
            Type::SizedNumeric(Primitive::Numerus, width) => {
                let range = self.authored_range(form)?;
                if range.is_none() {
                    Self::expect_no_form_args(form)?;
                }
                Ok(Self::policy_type_to_faber(
                    radix_types::IntPolicy::Trapping,
                    *width,
                    range.as_deref(),
                    surface,
                ))
            }
            Type::PolicyFloat(policy, width) => {
                Self::expect_no_form_args(form)?;
                Ok(format!(
                    "{}<{}>",
                    surface.type_surface(policy_word_binding(*policy)),
                    width.faber_marker()
                ))
            }
            Type::RangedInt {
                policy,
                width,
                lo,
                hi,
            } => match self.authored_range(form)? {
                Some(range) => Ok(Self::policy_type_to_faber(
                    *policy,
                    *width,
                    Some(&range),
                    surface,
                )),
                None => Ok(Self::ranged_int_to_faber(
                    *policy, *width, *lo, *hi, surface,
                )),
            },
            _ => Err(malformed_type_form(
                "authored POLICY level is illegal for this constructor".to_owned(),
            )),
        }
    }

    /// `wrapping<inf>` / `saturating<inf>` spelling (ruling 49): the witness is
    /// bare `inf` (the words have nothing to wrap or clamp on an unbounded
    /// integer), so only the level remembers which policy word was written.
    fn semantic_inf_word_form_to_faber(
        &self,
        form: &HirTypeForm,
        word: TypeBindingKind,
        surface: &impl KeywordSurface,
    ) -> Result<String, CodegenError> {
        match self.types.get(form.ty) {
            Type::Error => Ok("<error>".to_owned()),
            Type::SizedNumeric(Primitive::Numerus, width) if width.is_unbounded() => {
                let range = self.authored_range(form)?;
                if range.is_none() {
                    Self::expect_no_form_args(form)?;
                }
                let policy = match word {
                    TypeBindingKind::ModularWord => radix_types::IntPolicy::Wrapping,
                    TypeBindingKind::SaturatingWord => radix_types::IntPolicy::Saturating,
                    _ => {
                        return Err(malformed_type_form(
                            "invalid unbounded policy word".to_owned(),
                        ));
                    }
                };
                Ok(Self::policy_type_to_faber(
                    policy,
                    *width,
                    range.as_deref(),
                    surface,
                ))
            }
            _ => Err(malformed_type_form(
                "authored wrapping/saturating-inf level is illegal for this constructor".to_owned(),
            )),
        }
    }

    /// SHORT spellings per the registry: bare width markers and fused
    /// `l/t/v/m/s` roots. Width markers keep their language-wide spelling.
    fn semantic_short_form_to_faber(
        &self,
        form: &HirTypeForm,
        surface: &impl KeywordSurface,
    ) -> Result<String, CodegenError> {
        match self.types.get(form.ty) {
            // Lowering's fail-closed bail-out keeps the authored level over an
            // Error witness; render it like the canonical path.
            Type::Error => Ok("<error>".to_owned()),
            Type::SizedNumeric(_, width) => {
                Self::expect_no_form_args(form)?;
                Ok(width.faber_marker().to_owned())
            }
            // Bits markers (`b8`..`b64`) are bare, locale-neutral tokens.
            Type::Bits(width) => {
                Self::expect_no_form_args(form)?;
                Ok(width.faber_marker().to_owned())
            }
            Type::Array(_) => Ok(format!("l{}", self.fused_width_marker(form)?)),
            Type::Tensor(_, shape) => match form.args.as_slice() {
                [HirTypeFormArg::Type(child), HirTypeFormArg::Size(index)]
                    if self.same_figura(*index, *shape, surface) =>
                {
                    let marker = self.fused_child_marker(child)?;
                    Ok(fused_with_figura(
                        "t",
                        &marker,
                        &self.format_figura(*shape, surface),
                    ))
                }
                _ => Err(malformed_type_form(
                    "fused tensor root requires one width child and one size argument matching its witness"
                        .to_owned(),
                )),
            },
            // Vector/matrix/sparsa lowering records the element child only;
            // the figura rides the witness.
            Type::Vector(_, width) => {
                let marker = self.fused_width_marker(form)?;
                Ok(fused_with_figura(
                    "v",
                    &marker,
                    &self.format_figura(*width, surface),
                ))
            }
            Type::Matrix(_, shape) => {
                let marker = self.fused_width_marker(form)?;
                Ok(fused_with_figura(
                    "m",
                    &marker,
                    &self.format_figura(*shape, surface),
                ))
            }
            Type::Sparsa(_, shape) => {
                let marker = self.fused_width_marker(form)?;
                Ok(fused_with_figura(
                    "s",
                    &marker,
                    &self.format_figura(*shape, surface),
                ))
            }
            _ => Err(malformed_type_form(
                "authored SHORT level is illegal for this constructor".to_owned(),
            )),
        }
    }

    /// LONG spellings per the registry: the explicit head for every built-in
    /// constructor, with form-tree children where the constructor has a 1:1
    /// authored-child shape. Witness-only constructors (function types,
    /// unions, records, def/param identities, solved holes) re-emit
    /// canonically from their witness.
    fn semantic_long_form_to_faber(
        &self,
        form: &HirTypeForm,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
    ) -> Result<String, CodegenError> {
        let ty = self.types.get(form.ty);
        match ty {
            Type::Error => Ok("<error>".to_owned()),
            // A list length range keeps its authored spelling:
            // the glyph, the bounds as written and the `size` marker. The
            // resolved witness may be any list shape (`size 0…8` is a bounded
            // list, `size 0…∞` the plain one), so the authored range wins.
            Type::Array(_)
            | Type::BoundedArray { .. }
            | Type::BoundedOcteti { .. }
            | Type::RangedArray { .. }
                if form
                    .args
                    .iter()
                    .any(|arg| matches!(arg, HirTypeFormArg::Range(_))) =>
            {
                let [HirTypeFormArg::Type(child), HirTypeFormArg::Range(range)] =
                    form.args.as_slice()
                else {
                    return Err(malformed_type_form(
                        "a list size range form requires an element and one range".to_owned(),
                    ));
                };
                Ok(format!(
                    "{}<{}, {} {}>",
                    surface.type_surface(TypeBindingKind::Collection(CollectionKind::Lista)),
                    self.type_form_to_faber(child, names, surface)?,
                    surface.keyword(TokenKind::Magnitudo),
                    self.range_argument_to_faber(range)
                ))
            }
            Type::Primitive(Primitive::Octeti) => {
                Self::expect_no_form_args(form)?;
                Ok(format!(
                    "{}<{}>",
                    surface.type_surface(TypeBindingKind::Collection(CollectionKind::Lista)),
                    BitsWidth::B8.faber_marker()
                ))
            }
            Type::Primitive(prim) => {
                Self::expect_no_form_args(form)?;
                Ok(surface
                    .type_surface(TypeBindingKind::Primitive(*prim))
                    .to_owned())
            }
            Type::TensorEdgePolicy(policy) => {
                Self::expect_no_form_args(form)?;
                Ok(surface
                    .type_surface(TypeBindingKind::TensorEdgePolicy(*policy))
                    .to_owned())
            }
            Type::SizedNumeric(prim, width) => {
                Self::expect_no_form_args(form)?;
                match prim {
                    Primitive::Numerus | Primitive::Fractus => Ok(width.faber_marker().to_owned()),
                    _ => Err(malformed_type_form(
                        "sized numeric witness carries an unsupported primitive family".to_owned(),
                    )),
                }
            }
            Type::SizedInstans(precision) => {
                Self::expect_no_form_args(form)?;
                Ok(format!(
                    "{}<{}>",
                    surface.type_surface(TypeBindingKind::Primitive(Primitive::Instans)),
                    precision.faber_marker()
                ))
            }
            Type::Bits(width) => {
                Self::expect_no_form_args(form)?;
                Ok(width.faber_marker().to_owned())
            }
            Type::ModularWord(width) => {
                let range = self.authored_range(form)?;
                if range.is_none() {
                    Self::expect_no_form_args(form)?;
                }
                Ok(Self::policy_type_to_faber(
                    radix_types::IntPolicy::Wrapping,
                    *width,
                    range.as_deref(),
                    surface,
                ))
            }
            Type::SaturatingWord(width) => {
                let range = self.authored_range(form)?;
                if range.is_none() {
                    Self::expect_no_form_args(form)?;
                }
                Ok(Self::policy_type_to_faber(
                    radix_types::IntPolicy::Saturating,
                    *width,
                    range.as_deref(),
                    surface,
                ))
            }
            Type::PolicyFloat(policy, width) => {
                Self::expect_no_form_args(form)?;
                Ok(format!(
                    "{}<{}>",
                    surface.type_surface(policy_word_binding(*policy)),
                    width.faber_marker()
                ))
            }
            Type::RangedInt {
                policy,
                width,
                lo,
                hi,
            } => match self.authored_range(form)? {
                Some(range) => Ok(Self::policy_type_to_faber(
                    *policy,
                    *width,
                    Some(&range),
                    surface,
                )),
                None => Ok(Self::ranged_int_to_faber(
                    *policy, *width, *lo, *hi, surface,
                )),
            },
            Type::BoundedTextus { capacity } => {
                Self::expect_no_form_args(form)?;
                Ok(self.bounded_primitive_head(Primitive::Textus, *capacity, surface))
            }
            Type::BoundedAscii { capacity } => {
                Self::expect_no_form_args(form)?;
                Ok(self.bounded_primitive_head(Primitive::Ascii, *capacity, surface))
            }
            Type::BoundedOcteti { capacity } => {
                // Ruling d477c76d spells the bounded byte buffer `lista<b8, magnitudo N>`,
                // so the live authored form carries the `b8` type child; the
                // retired `octeti<N>`/`bytes<N>` word carried none. Both
                // re-emit as the list form.
                let element = if form.args.is_empty() {
                    BitsWidth::B8.faber_marker().to_owned()
                } else {
                    self.single_type_form_child(form, names, surface)?
                };
                Ok(format!(
                    "{}<{}, {}>",
                    surface.type_surface(TypeBindingKind::Collection(CollectionKind::Lista)),
                    element,
                    self.list_count_to_faber(*capacity, surface)
                ))
            }
            // An authored empty-arg array form witnesses the retired
            // `octeti`/`bytes` word (ruling d477c76d); it re-emits as the
            // list form.
            Type::Array(element)
                if form.args.is_empty()
                    && matches!(self.types.get(*element), Type::Bits(BitsWidth::B8)) =>
            {
                Ok(format!(
                    "{}<{}>",
                    surface.type_surface(TypeBindingKind::Collection(CollectionKind::Lista)),
                    BitsWidth::B8.faber_marker()
                ))
            }
            Type::Array(_) => Ok(format!(
                "{}<{}>",
                surface.type_surface(TypeBindingKind::Collection(CollectionKind::Lista)),
                self.single_type_form_child(form, names, surface)?
            )),
            Type::Queue(_) => Ok(format!(
                "{}<{}>",
                surface.type_surface(TypeBindingKind::Collection(CollectionKind::Queue)),
                self.single_type_form_child(form, names, surface)?
            )),
            Type::Stack(_) => Ok(format!(
                "{}<{}>",
                surface.type_surface(TypeBindingKind::Collection(CollectionKind::Stack)),
                self.single_type_form_child(form, names, surface)?
            )),
            Type::Set(_) => Ok(format!(
                "{}<{}>",
                surface.type_surface(TypeBindingKind::Collection(CollectionKind::Copia)),
                self.single_type_form_child(form, names, surface)?
            )),
            Type::Cursor(_) => Ok(format!(
                "{}<{}>",
                surface.type_surface(TypeBindingKind::Collection(CollectionKind::Cursor)),
                self.single_type_form_child(form, names, surface)?
            )),
            Type::Promissum(_) => Ok(format!(
                "{}<{}>",
                surface.type_surface(TypeBindingKind::Collection(CollectionKind::Promissum)),
                self.single_type_form_child(form, names, surface)?
            )),
            Type::PromissumFailable(_, _) => {
                let members = self.exact_type_form_children(form, 2, names, surface)?;
                Ok(format!(
                    "{}<{} ⇥ {}>",
                    surface.type_surface(TypeBindingKind::Collection(CollectionKind::Promissum)),
                    members[0],
                    members[1]
                ))
            }
            Type::AsyncCursor(_, _) => {
                let members = self.exact_type_form_children(form, 2, names, surface)?;
                // Canonical arm keeps this spelling outside the keyword
                // surface; mirror it exactly.
                Ok(format!("async cursor<{} ⇥ {}>", members[0], members[1]))
            }
            Type::Map(_, _) => {
                // `series` (the `tabula<textus, valor>` synonym) lowers with
                // no form children; the synonym itself is not captured, so it
                // re-emits through the canonical path.
                if form.args.is_empty() {
                    return Ok(self.type_to_faber(form.ty, names, surface));
                }
                let members = self.exact_type_form_children(form, 2, names, surface)?;
                Ok(format!(
                    "{}<{}, {}>",
                    surface.type_surface(TypeBindingKind::Collection(CollectionKind::Tabula)),
                    members[0],
                    members[1]
                ))
            }
            Type::Ledger(_, _) => {
                let members = self.exact_type_form_children(form, 2, names, surface)?;
                Ok(format!(
                    "{}<{}, {}>",
                    surface.type_surface(TypeBindingKind::Collection(CollectionKind::Tabulae)),
                    members[0],
                    members[1]
                ))
            }
            Type::Record(fields) => {
                // Authored children carry the label and the authored field
                // order, so a hole (`ratio<g: _, w: int>`) and an authored
                // nested spelling survive. A solved or hand-built record has
                // no complete labeled child list and re-emits canonically.
                let labeled = form.args.len() == fields.len()
                    && form.args.iter().all(
                        |arg| matches!(arg, HirTypeFormArg::Type(child) if child.label.is_some()),
                    );
                if !labeled {
                    return Ok(self.type_to_faber(form.ty, names, surface));
                }
                let rendered = self.labeled_iuncta_children(form, fields.len(), names, surface)?;
                Ok(format!(
                    "{}<{}>",
                    surface.type_surface(TypeBindingKind::Ratio),
                    rendered.join(", ")
                ))
            }
            Type::Tensor(_, shape) => {
                let elem = self.tensor_element_child(form, *shape, names, surface)?;
                Ok(self.figura_tensor_head(TensorFamilyKind::Tensor, &elem, *shape, surface))
            }
            Type::Vector(_, width) => {
                let elem = self.single_type_form_child(form, names, surface)?;
                Ok(self.figura_tensor_head(TensorFamilyKind::Vector, &elem, *width, surface))
            }
            Type::Matrix(_, shape) => {
                let elem = self.single_type_form_child(form, names, surface)?;
                Ok(self.figura_tensor_head(TensorFamilyKind::Matrix, &elem, *shape, surface))
            }
            Type::Sparsa(_, shape) => {
                let elem = self.single_type_form_child(form, names, surface)?;
                Ok(self.figura_tensor_head(TensorFamilyKind::Sparsa, &elem, *shape, surface))
            }
            Type::Atomic(_) => Ok(format!(
                "{}<{}>",
                surface.type_surface(TypeBindingKind::TensorFamily(TensorFamilyKind::Atomic)),
                self.single_type_form_child(form, names, surface)?
            )),
            Type::Intervallum(..) => match form.args.as_slice() {
                [HirTypeFormArg::Type(child)] => Ok(format!(
                    "{}<{}>",
                    surface
                        .type_surface(TypeBindingKind::TensorFamily(TensorFamilyKind::Intervallum)),
                    self.type_form_to_faber(child, names, surface)?
                )),
                [HirTypeFormArg::Type(child), HirTypeFormArg::Size(step)] => Ok(format!(
                    "{}<{}, {}>",
                    surface
                        .type_surface(TypeBindingKind::TensorFamily(TensorFamilyKind::Intervallum)),
                    self.type_form_to_faber(child, names, surface)?,
                    self.format_figura(*step, surface)
                )),
                // Never drop an authored step silently: unusual shapes take the
                // canonical spelling, which carries the witness step.
                _ => Ok(self.type_to_faber(form.ty, names, surface)),
            },
            Type::BoundedArray { capacity, .. } => {
                let elem = self.single_type_form_child(form, names, surface)?;
                Ok(format!(
                    "{}<{elem}, {}>",
                    surface.type_surface(TypeBindingKind::Collection(CollectionKind::Lista)),
                    self.list_count_to_faber(*capacity, surface)
                ))
            }
            Type::RangedArray { min, max, .. } => {
                let elem = self.single_type_form_child(form, names, surface)?;
                Ok(format!(
                    "{}<{elem}, {}>",
                    surface.type_surface(TypeBindingKind::Collection(CollectionKind::Lista)),
                    Self::list_range_to_faber(*min, *max, surface)
                ))
            }
            Type::BoundedQueue { capacity, .. } => {
                let elem = self.single_type_form_child(form, names, surface)?;
                Ok(format!(
                    "{}<{elem}, {}>",
                    surface.type_surface(TypeBindingKind::Collection(CollectionKind::Queue)),
                    self.list_count_to_faber(*capacity, surface)
                ))
            }
            Type::BoundedStack { capacity, .. } => {
                let elem = self.single_type_form_child(form, names, surface)?;
                Ok(format!(
                    "{}<{elem}, {}>",
                    surface.type_surface(TypeBindingKind::Collection(CollectionKind::Stack)),
                    self.list_count_to_faber(*capacity, surface)
                ))
            }
            Type::Option(_) => match form.args.as_slice() {
                // Union-normalized or solved option witnesses carry no
                // authored child and delegate canonically.
                [] => Ok(self.type_to_faber(form.ty, names, surface)),
                [HirTypeFormArg::Type(child)] => {
                    // Witness reconciliation can wrap an authored alias in a
                    // semantic form when the checker flattens `Alias ∪ none`
                    // to its payload. In this option position the nested named
                    // form is the source spelling we must retain; rendering the
                    // semantic function/constructor witness would expand it.
                    let payload = match child.args.as_slice() {
                        [HirTypeFormArg::Type(authored_alias)]
                            if matches!(&child.head, HirTypeFormHead::Semantic(_))
                                && matches!(
                                    &authored_alias.head,
                                    HirTypeFormHead::Named { .. }
                                )
                                && matches!(
                                    self.types.get(authored_alias.ty),
                                    Type::Alias(_, _)
                                ) =>
                        {
                            authored_alias.as_ref()
                        }
                        _ => child.as_ref(),
                    };
                    let inner = self.type_form_to_faber(payload, names, surface)?;
                    Ok(format!(
                        "{inner} ∪ {}",
                        surface.type_surface(TypeBindingKind::Primitive(Primitive::Nihil))
                    ))
                }
                _ => Err(malformed_type_form(
                    "option form carries unexpected children".to_owned(),
                )),
            },
            Type::Ref(mutability, _) => {
                let [HirTypeFormArg::Type(child)] = form.args.as_slice() else {
                    return Err(malformed_type_form(
                        "reference form requires exactly one type child".to_owned(),
                    ));
                };
                let prefix = match mutability {
                    SemanticMutability::Immutable => surface.keyword(radix_lexer::TokenKind::De),
                    SemanticMutability::Mutable => surface.keyword(radix_lexer::TokenKind::In),
                    SemanticMutability::Own => surface.keyword(radix_lexer::TokenKind::Own),
                    SemanticMutability::Copy => surface.keyword(radix_lexer::TokenKind::Copy),
                };
                let inner = self.type_form_to_faber(child, names, surface)?;
                Ok(format!("{prefix} {inner}"))
            }
            Type::Tuple(members) => {
                let rendered = self.labeled_iuncta_children(form, members.len(), names, surface)?;
                Ok(format!(
                    "{}<{}>",
                    surface.contextual_keyword("iuncta"),
                    rendered.join(", ")
                ))
            }
            // A builtin frame carrier (`sermo`) keeps no type arguments in its
            // erased witness, but the author's `sermo<O, R>` rides the form:
            // print the head over the authored arguments, or the re-emit
            // would drop them.
            Type::Struct(def_id)
                if !form.args.is_empty() && self.builtin_frame_family(*def_id, names).is_some() =>
            {
                let head = self.type_to_faber(form.ty, names, surface);
                let args = self.type_form_args_to_faber(&form.args, names, surface)?;
                Ok(format!("{head}<{}>", args.join(", ")))
            }
            Type::Func(_)
            | Type::Union(_)
            | Type::Intersection(_)
            | Type::Param(_)
            | Type::Applied(_, _)
            | Type::Alias(_, _)
            | Type::Struct(_)
            | Type::Enum(_)
            | Type::Interface(_)
            | Type::Infer(_)
            | Type::InferUnion(_)
            | Type::MarkerHole { .. } => {
                // Witness-only witnesses: lowering keeps no 1:1 authored
                // child shape here (modes, dedup, solved holes, def
                // identities), so the node re-emits canonically.
                Ok(self.type_to_faber(form.ty, names, surface))
            }
        }
    }

    /// Render the authored argument list of a `Named` node.
    fn type_form_args_to_faber(
        &self,
        args: &[HirTypeFormArg],
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
    ) -> Result<Vec<String>, CodegenError> {
        args.iter()
            .map(|arg| match arg {
                HirTypeFormArg::Type(child) => self.type_form_to_faber(child, names, surface),
                HirTypeFormArg::Size(index) => Ok(self.format_figura(*index, surface)),
                HirTypeFormArg::Name(symbol) => Ok(self.symbol_to_string(*symbol)),
                HirTypeFormArg::Range(range) => Ok(self.range_argument_to_faber(range)),
            })
            .collect()
    }

    /// The exactly-one `Type` child of a unary constructor node, rendered.
    fn single_type_form_child(
        &self,
        form: &HirTypeForm,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
    ) -> Result<String, CodegenError> {
        // The count is checked above; the slice is exactly one element.
        let children = self.exact_type_form_children(form, 1, names, surface)?;
        let [child] = children.as_slice() else {
            return Err(malformed_type_form(
                "form requires exactly one type child".to_owned(),
            ));
        };
        Ok(child.clone())
    }

    /// Exactly `count` `Type` children, rendered in authored order.
    fn exact_type_form_children(
        &self,
        form: &HirTypeForm,
        count: usize,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
    ) -> Result<Vec<String>, CodegenError> {
        let children = form
            .args
            .iter()
            .map(|arg| match arg {
                HirTypeFormArg::Type(child) => Some(child.as_ref()),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()
            .filter(|children| children.len() == count);
        let Some(children) = children else {
            return Err(malformed_type_form(format!(
                "form requires exactly {count} type children, got {} arguments",
                form.args.len()
            )));
        };
        children
            .iter()
            .map(|child| self.type_form_to_faber(child, names, surface))
            .collect()
    }

    /// Authored element label recorded at one `iuncta` element site, if the
    /// source spelled one there. Construction type arguments record each
    /// element as its own root form, so the label sits on that root.
    pub(super) fn authored_element_label(&self, site: &HirTypeSite) -> Option<Symbol> {
        self.authored_type_forms.get(site)?.label
    }

    /// Render the `iuncta` element list, restoring each child's authored
    /// element label (`iuncta<gx: int, gw: int>`). The label rides the child
    /// node; unlabeled slots render bare.
    fn labeled_iuncta_children(
        &self,
        form: &HirTypeForm,
        count: usize,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
    ) -> Result<Vec<String>, CodegenError> {
        let rendered = self.exact_type_form_children(form, count, names, surface)?;
        Ok(form
            .args
            .iter()
            .zip(rendered)
            .map(|(arg, spelling)| match arg {
                HirTypeFormArg::Type(child) => match child.label {
                    Some(label) => format!("{}: {spelling}", self.symbol_to_string(label)),
                    None => spelling,
                },
                HirTypeFormArg::Size(_) | HirTypeFormArg::Name(_) | HirTypeFormArg::Range(_) => {
                    spelling
                }
            })
            .collect())
    }

    /// The element child of a tensor-form node, whose lowering records the
    /// `[Type, Size]` pair; the size argument must match the witness shape.
    fn tensor_element_child(
        &self,
        form: &HirTypeForm,
        shape: IndexId,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
    ) -> Result<String, CodegenError> {
        match form.args.as_slice() {
            [HirTypeFormArg::Type(child), HirTypeFormArg::Size(index)]
                if self.same_figura(*index, shape, surface) =>
            {
                self.type_form_to_faber(child, names, surface)
            }
            _ => Err(malformed_type_form(
                "tensor form requires one type child and one size argument matching its witness"
                    .to_owned(),
            )),
        }
    }

    /// Width marker of a fused short root (`lu32`): exactly one argument, a
    /// type child that is itself a SHORT authored width over a sized-numeric
    /// witness.
    fn fused_width_marker(&self, form: &HirTypeForm) -> Result<String, CodegenError> {
        let [HirTypeFormArg::Type(child)] = form.args.as_slice() else {
            return Err(malformed_type_form(format!(
                "fused short root requires exactly one type child, got {} arguments",
                form.args.len()
            )));
        };
        self.fused_child_marker(child)
    }

    fn fused_child_marker(&self, child: &HirTypeForm) -> Result<String, CodegenError> {
        if child.head != HirTypeFormHead::Semantic(TypeFormLevel::SHORT) {
            return Err(malformed_type_form(
                "fused short root requires a SHORT width child".to_owned(),
            ));
        }
        match self.types.get(child.ty) {
            Type::SizedNumeric(_, width) => Ok(width.faber_marker().to_owned()),
            _ => Err(malformed_type_form(
                "fused short root child must be a sized numeric width".to_owned(),
            )),
        }
    }

    fn same_figura(&self, left: IndexId, right: IndexId, surface: &impl KeywordSurface) -> bool {
        self.format_figura(left, surface) == self.format_figura(right, surface)
    }

    fn figura_tensor_head(
        &self,
        kind: TensorFamilyKind,
        child: &str,
        figura: IndexId,
        surface: &impl KeywordSurface,
    ) -> String {
        // The vector long form writes its lane count after the `size` marker
        //; the other heads take a bracketed extent.
        let extent = if matches!(kind, TensorFamilyKind::Vector) {
            self.vector_width_to_faber(figura, surface)
        } else {
            self.format_figura(figura, surface)
        };
        format!(
            "{}<{child}, {extent}>",
            surface.type_surface(TypeBindingKind::TensorFamily(kind)),
        )
    }

    fn bounded_primitive_head(
        &self,
        prim: Primitive,
        capacity: IndexId,
        surface: &impl KeywordSurface,
    ) -> String {
        format!(
            "{}<{}>",
            surface.type_surface(TypeBindingKind::Primitive(prim)),
            self.list_count_to_faber(capacity, surface)
        )
    }

    /// Recover an authored range argument from the form tree, rendering its
    /// source glyph and endpoints instead of reconstructing resolved bounds.
    fn authored_range(&self, form: &HirTypeForm) -> Result<Option<String>, CodegenError> {
        match form.args.as_slice() {
            [] => Ok(None),
            [HirTypeFormArg::Range(range)] => Ok(Some(self.range_argument_to_faber(range))),
            _ => Err(malformed_type_form(
                "a ranged policy form requires exactly one range argument".to_owned(),
            )),
        }
    }

    fn range_argument_to_faber(&self, range: &HirRangeArgument) -> String {
        let bound_to_string = |bound: &HirRangeBound| match bound {
            HirRangeBound::Literal(value) => value.clone(),
            HirRangeBound::Unbounded { negative: true } => "-∞".to_owned(),
            HirRangeBound::Unbounded { negative: false } => "∞".to_owned(),
            HirRangeBound::Name { name, lifted } => {
                let name = self.symbol_to_string(*name);
                if *lifted {
                    format!("size {name}")
                } else {
                    name
                }
            }
        };
        let glyph = if range.inclusive { "…" } else { "‥" };
        format!(
            "{}{}{}",
            bound_to_string(&range.lo),
            glyph,
            bound_to_string(&range.hi)
        )
    }

    fn policy_type_to_faber(
        policy: radix_types::IntPolicy,
        width: radix_types::NumericWidth,
        range: Option<&str>,
        surface: &impl KeywordSurface,
    ) -> String {
        let head = match policy {
            radix_types::IntPolicy::Trapping => surface.type_surface(TypeBindingKind::TrappingWord),
            radix_types::IntPolicy::Wrapping => surface.contextual_keyword("modulus"),
            radix_types::IntPolicy::Saturating => {
                surface.type_surface(TypeBindingKind::SaturatingWord)
            }
        };
        match range {
            Some(range) => format!("{head}<{}, {range}>", width.faber_marker()),
            None => format!("{head}<{}>", width.faber_marker()),
        }
    }

    /// Canonical fallback for a ranged cell with no authored range form. A
    /// source-backed form uses its retained glyph and endpoints instead.
    fn ranged_int_to_faber(
        policy: radix_types::IntPolicy,
        width: radix_types::NumericWidth,
        lo: i128,
        hi: i128,
        surface: &impl KeywordSurface,
    ) -> String {
        let word = match policy {
            radix_types::IntPolicy::Trapping => surface.type_surface(TypeBindingKind::TrappingWord),
            radix_types::IntPolicy::Wrapping => surface.contextual_keyword("modulus"),
            radix_types::IntPolicy::Saturating => {
                surface.type_surface(TypeBindingKind::SaturatingWord)
            }
        };
        let range = if hi < i128::from(u64::MAX) {
            format!("{lo}‥{}", hi + 1)
        } else {
            format!("{lo}…{hi}")
        };
        format!("{word}<{}, {range}>", width.faber_marker())
    }

    /// A list length range spelled after the `size` marker, inclusive:
    /// `size 1…8`, or `size 1…∞` for no upper end.
    fn list_range_to_faber(min: u64, max: Option<u64>, surface: &impl KeywordSurface) -> String {
        let size = surface.keyword(TokenKind::Magnitudo);
        match max {
            None => format!("{size} {min}…∞"),
            Some(max) => format!("{size} {min}…{max}"),
        }
    }

    fn expect_no_form_args(form: &HirTypeForm) -> Result<(), CodegenError> {
        if form.args.is_empty() {
            Ok(())
        } else {
            Err(malformed_type_form(format!(
                "form head carries {} unexpected child arguments",
                form.args.len()
            )))
        }
    }

    fn format_unary_type(
        &self,
        head: &str,
        inner: TypeId,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
    ) -> String {
        format!("{head}<{}>", self.type_to_faber(inner, names, surface))
    }

    fn format_binary_type(
        &self,
        head: &str,
        left: TypeId,
        right: TypeId,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
    ) -> String {
        format!(
            "{head}<{}, {}>",
            self.type_to_faber(left, names, surface),
            self.type_to_faber(right, names, surface)
        )
    }

    fn format_union_type(
        &self,
        members: &[TypeId],
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
    ) -> String {
        if members.is_empty() {
            return surface
                .type_surface(TypeBindingKind::Primitive(Primitive::Ignotum))
                .to_owned();
        }
        members
            .iter()
            .map(|member| self.type_to_faber(*member, names, surface))
            .collect::<Vec<_>>()
            .join(" ∪ ")
    }

    fn format_record_type(
        &self,
        fields: &handroll::FxHashMap<Symbol, TypeId>,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
    ) -> String {
        let entries: Vec<String> = sorted_record_fields(fields, self.interner)
            .into_iter()
            .map(|(name, ty)| {
                format!(
                    "{}: {}",
                    self.symbol_to_string(name),
                    self.type_to_faber(ty, names, surface)
                )
            })
            .collect();
        format!(
            "{}<{}>",
            surface.type_surface(TypeBindingKind::Ratio),
            entries.join(", ")
        )
    }

    /// Canonical fallback for presentation-absent sites: the long
    /// `tensor<f32, [2, 2]>` surface re-parses in every locale. Authored
    /// `tf32[2, 2]` roots render through `type_to_faber_at` from their
    /// recorded form instead; this helper is never that path.
    fn format_tensor_family_type(
        &self,
        kind: TensorFamilyKind,
        elem: TypeId,
        figura: IndexId,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
    ) -> String {
        let child = self.type_to_faber(elem, names, surface);
        self.figura_tensor_head(kind, &child, figura, surface)
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

/// Fused short spelling: the figura string is already bracketed by
/// `format_figura` (`[2, 3]`); an inference hole (authored without brackets)
/// emits the bare fused token. A single-literal figura prints unbracketed
/// from `format_figura` (lowering interns `vf32[4]`'s width as a bare
/// `Literal`), so bracket it here — the sugar grammar has no `vf324` form.
fn fused_with_figura(prefix: &str, marker: &str, figura: &str) -> String {
    if figura == "_" {
        format!("{prefix}{marker}")
    } else if figura.starts_with('[') {
        format!("{prefix}{marker}{figura}")
    } else {
        format!("{prefix}{marker}[{figura}]")
    }
}

/// Fail-closed error for a present-but-malformed authored type form.
fn malformed_type_form(message: String) -> CodegenError {
    CodegenError {
        message,
        args: vec![DiagnosticArg::new("issue", "malformed_authored_type_form")],
    }
}

#[cfg(test)]
#[path = "types_test.rs"]
mod tests;

#[cfg(test)]
#[path = "types_form_test.rs"]
mod form_test;

/// Type-surface binding of a policy-float cell's policy word (D5): the same
/// `TrappingWord` / `SaturatingWord` bindings the integer cells use.
fn policy_word_binding(policy: radix_types::FloatPolicy) -> TypeBindingKind {
    match policy {
        radix_types::FloatPolicy::Trapping => TypeBindingKind::TrappingWord,
        radix_types::FloatPolicy::Saturating => TypeBindingKind::SaturatingWord,
    }
}
