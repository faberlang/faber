//! Semantic metadata (`MirValidationContext`) plus enum-representation and
//! integer/pointer/boolean payload analysis used to type-check enum operations.

use super::error::MirFunctionSignature;
use crate::{
    MirClosureEnvironment, MirClosureEnvironmentId, MirExternalFunctionIdentity,
    MirGenericBindings, MirLayoutKind, MirLayoutTable, MirScalarLayout, MirType,
    instantiate_existing_generic_type,
};
use handroll::{FxHashMap, FxHashSet};
use radix_hir::DefId;
use radix_lexer::{Interner, Span, Symbol};
use radix_types::{Primitive, Type, TypeId, TypeTable};

/// One flattened payload field in the selected MIR enum ABI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MirEnumPayloadField {
    pub variant: DefId,
    pub field: Symbol,
    pub ty: MirType,
}

/// Explicit scalar value-domain metadata supplied by MIR producers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirScalarValueDomain {
    IntegerRange { min: i128, max: i128 },
    FloatNonNan,
    FloatFinite,
}

/// MIR-level enum representation selected from validation metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MirEnumRepresentation {
    UnitTags {
        tag_layout: MirScalarLayout,
        variants: Vec<DefId>,
    },
    PointerNiche {
        unit_variant: DefId,
        payload: MirEnumPayloadField,
    },
    BooleanPayload {
        unit_variant: DefId,
        payload: MirEnumPayloadField,
    },
    IntegerPayload {
        unit_variant: DefId,
        payload: MirEnumPayloadField,
        payload_layout: MirScalarLayout,
        carrier_layout: MirScalarLayout,
        sentinel: i128,
    },
    FloatPayload {
        unit_variant: DefId,
        payload: MirEnumPayloadField,
        payload_layout: MirScalarLayout,
    },
    TaggedUnion {
        tag_layout: MirScalarLayout,
        fields: Vec<MirEnumPayloadField>,
        payload_ty: MirType,
    },
    TaggedProduct {
        tag_layout: MirScalarLayout,
        fields: Vec<MirEnumPayloadField>,
    },
}

/// Borrowed-or-owned type-table storage for [`MirValidationContext`].
///
/// Most contexts borrow the analyzed unit's table ([`Self::Borrowed`]).
/// Phases that legitimately extend the arena after analysis — MIR
/// monomorphization's shape finalization and AIR backward shape
/// finalization — clone the table once, intern into the clone, and hand it
/// to the context as [`Self::Owned`], so the validation token carries its
/// own table for as long as it lives instead of borrowing a table some
/// phase would have to leak.
///
/// The variant dereferences to [`TypeTable`], so existing `types.get(…)`
/// style reads are unchanged; reads that need the reference itself use
/// [`Self::as_table`].
#[derive(Clone)]
pub enum MirTypeTable<'a> {
    /// Table borrowed from the phase that owns it (the analyzed unit or a
    /// test fixture).
    Borrowed(&'a TypeTable),
    /// Table owned by the validation context — the post-analysis extension
    /// path (lowering-owned type arena). Shared ownership: cloning the
    /// context shares the table, and the last drop frees it, so the token's
    /// table is bounded without any phase leaking one.
    Owned(std::rc::Rc<TypeTable>),
}

impl MirTypeTable<'_> {
    /// The table reference, regardless of storage.
    #[must_use]
    pub fn as_table(&self) -> &TypeTable {
        match self {
            Self::Borrowed(table) => table,
            Self::Owned(table) => table,
        }
    }
}

impl std::ops::Deref for MirTypeTable<'_> {
    type Target = TypeTable;

    fn deref(&self) -> &TypeTable {
        self.as_table()
    }
}

/// Borrowed, owned, or absent interner storage for [`MirValidationContext`].
///
/// Mirrors [`MirTypeTable`]: most contexts borrow the analyzed unit's
/// interner; the lowering path hands the context the lowering-owned interner
/// clone (which carries the runtime/diagnostic symbols lowering interns) as
/// [`Self::Owned`], so tokens that escape lowering resolve every symbol their
/// MIR references without any phase leaking an interner.
#[derive(Clone)]
pub enum MirInterner<'a> {
    /// No interner carried (contexts built from bare type tables).
    Absent,
    /// Interner borrowed from the phase that owns it.
    Borrowed(&'a Interner),
    /// Interner owned by the validation context.
    Owned(std::sync::Arc<Interner>),
}

impl MirInterner<'_> {
    /// The interner reference, or `None` when absent.
    #[must_use]
    pub fn as_interner(&self) -> Option<&Interner> {
        match self {
            Self::Absent => None,
            Self::Borrowed(interner) => Some(*interner),
            Self::Owned(interner) => Some(interner.as_ref()),
        }
    }
}

/// Metadata needed to validate MIR against the semantic world it was lowered from.
///
/// `types` is authoritative for assignability and primitive IDs. Function and
/// field maps bridge MIR references back to HIR/semantic declarations where MIR
/// only carries `DefId`s and symbols.
#[derive(Clone)]
pub struct MirValidationContext<'a> {
    pub types: MirTypeTable<'a>,
    pub layouts: MirLayoutTable,
    pub interner: MirInterner<'a>,
    pub functions: FxHashMap<DefId, MirFunctionSignature>,
    /// Functions lowered from genus methods: the first MIR parameter of each is
    /// the method's receiver, lowered as an `Owned` parameter that MIR itself
    /// cannot tell from an `own Genus` parameter. Backends that must treat a
    /// receiver differently (the sexp write-back) read it here; it is not part
    /// of any serialized MIR form.
    pub method_defs: FxHashSet<DefId>,
    pub external_function_identities: FxHashMap<DefId, MirExternalFunctionIdentity>,
    /// Current package product identity (package-aware compiles only).
    ///
    /// Set by package-aware MIR lowering from the caller's package identity.
    /// Emitters use it to classify `external_function_identities` whose
    /// `product` is the *current* package: those are cross-module user-code
    /// link identities (D-PA5 distinct symbol class), not library/host
    /// identities. `None` for plain single-file compiles (legacy provider
    /// routing, unchanged behavior).
    pub package_product: Option<String>,
    pub struct_fields: FxHashMap<DefId, FxHashMap<Symbol, MirType>>,
    /// Declaration order of each struct's instance fields (`struct_fields` is
    /// unordered); display paths render fields in this order.
    pub struct_field_order: FxHashMap<DefId, Vec<Symbol>>,
    /// Declaration order of each enum variant's payload fields (common fields
    /// first); `variant_fields` is unordered, display uses this order.
    pub variant_field_order: FxHashMap<DefId, Vec<Symbol>>,
    /// Construction-literal display name of each `genus` (`Pulsus`) and each
    /// `discretio` variant (`Nuntius.Pulsus`, D2.8); `print` renders values
    /// as `<name> { field = value, ... }`. Resolved text, so the map needs no
    /// interner remap when package units link.
    pub display_names: FxHashMap<DefId, String>,
    /// Program-level display-token table and the id each print-bearing
    /// function uses; nothing reads it yet.
    pub display_tokens: crate::display_tokens::MirDisplayTokens,
    pub struct_type_params: FxHashMap<DefId, Vec<Symbol>>,
    /// Struct → the contracts its `implet` clause names. A struct value is
    /// assignable to a contract-typed slot exactly when the contract is listed.
    pub struct_contracts: FxHashMap<DefId, FxHashSet<DefId>>,
    pub enum_type_params: FxHashMap<DefId, Vec<Symbol>>,
    pub optional_struct_fields: FxHashMap<DefId, FxHashSet<Symbol>>,
    pub nullable_struct_fields: FxHashMap<DefId, FxHashSet<Symbol>>,
    pub opaque_structs: FxHashSet<DefId>,
    pub json_struct_field_keys: FxHashMap<DefId, FxHashMap<Symbol, String>>,
    pub variant_parents: FxHashMap<DefId, DefId>,
    pub enum_variants: FxHashMap<DefId, Vec<DefId>>,
    /// Variant `DefId` → position within its parent enum's variant list.
    /// Built by [`Self::rebuild_variant_positions`]; `enum_variant_tag` uses this
    /// for O(1) lookup and falls back to a linear scan when the index is empty.
    pub variant_positions: FxHashMap<DefId, usize>,
    pub variant_fields: FxHashMap<DefId, FxHashMap<Symbol, MirType>>,
    pub scalar_value_domains: FxHashMap<TypeId, MirScalarValueDomain>,
    pub closure_environments: FxHashMap<MirClosureEnvironmentId, MirClosureEnvironment>,
    /// Tier-1 `@ ad` route table (D6.9, F11): exact route → the handler
    /// function's source `DefId`. Lowering records each module's handlers;
    /// package linking merges them. The MIR runner answers these routes
    /// in-process; emitters ignore the map.
    pub ad_routes: FxHashMap<String, DefId>,
}

impl<'a> MirValidationContext<'a> {
    #[must_use]
    pub fn new(types: &'a TypeTable) -> Self {
        Self {
            types: MirTypeTable::Borrowed(types),
            layouts: MirLayoutTable::from_types(types),
            interner: MirInterner::Absent,
            functions: FxHashMap::default(),
            method_defs: FxHashSet::default(),
            external_function_identities: FxHashMap::default(),
            package_product: None,
            struct_fields: FxHashMap::default(),
            struct_field_order: FxHashMap::default(),
            variant_field_order: FxHashMap::default(),
            display_names: FxHashMap::default(),
            display_tokens: crate::display_tokens::MirDisplayTokens::default(),
            struct_type_params: FxHashMap::default(),
            struct_contracts: FxHashMap::default(),
            enum_type_params: FxHashMap::default(),
            optional_struct_fields: FxHashMap::default(),
            nullable_struct_fields: FxHashMap::default(),
            opaque_structs: FxHashSet::default(),
            json_struct_field_keys: FxHashMap::default(),
            variant_parents: FxHashMap::default(),
            enum_variants: FxHashMap::default(),
            variant_positions: FxHashMap::default(),
            variant_fields: FxHashMap::default(),
            scalar_value_domains: FxHashMap::default(),
            closure_environments: FxHashMap::default(),
            ad_routes: FxHashMap::default(),
        }
    }

    /// Rebuild the O(1) variant→tag index from `enum_variants`.
    ///
    /// Call after bulk inserts into `enum_variants` (lowerers populate that map
    /// directly). [`ValidatedMir::new`] runs this automatically.
    pub fn rebuild_variant_positions(&mut self) {
        self.variant_positions.clear();
        for variants in self.enum_variants.values() {
            for (position, variant) in variants.iter().enumerate() {
                self.variant_positions.insert(*variant, position);
            }
        }
    }

    /// Clone this context's metadata onto a NEW type-table reference.
    ///
    /// Callers that must intern additional types (e.g. the D-2b-2 subchain
    /// output-tuple returns) while keeping every other typed fact identical
    /// clone the table, intern into the clone, and rebuild the context on it.
    /// The caller owns `types` and must keep it alive for the returned
    /// context; layouts are rebuilt against the new table, so freshly
    /// interned types resolve exactly like the originals.
    #[must_use]
    pub fn with_types<'b>(&self, types: &'b TypeTable) -> MirValidationContext<'b>
    where
        'a: 'b,
    {
        let mut context = MirValidationContext::<'b>::new(types);
        context.interner = self.interner.clone();
        context.functions.clone_from(&self.functions);
        context.method_defs.clone_from(&self.method_defs);
        context
            .external_function_identities
            .clone_from(&self.external_function_identities);
        context.package_product.clone_from(&self.package_product);
        context.struct_fields.clone_from(&self.struct_fields);
        context
            .struct_field_order
            .clone_from(&self.struct_field_order);
        context
            .variant_field_order
            .clone_from(&self.variant_field_order);
        context.display_names.clone_from(&self.display_names);
        context.display_tokens.clone_from(&self.display_tokens);
        context
            .struct_type_params
            .clone_from(&self.struct_type_params);
        context.struct_contracts.clone_from(&self.struct_contracts);
        context.enum_type_params.clone_from(&self.enum_type_params);
        context
            .optional_struct_fields
            .clone_from(&self.optional_struct_fields);
        context
            .nullable_struct_fields
            .clone_from(&self.nullable_struct_fields);
        context.opaque_structs.clone_from(&self.opaque_structs);
        context
            .json_struct_field_keys
            .clone_from(&self.json_struct_field_keys);
        context.variant_parents.clone_from(&self.variant_parents);
        context.enum_variants.clone_from(&self.enum_variants);
        context
            .variant_positions
            .clone_from(&self.variant_positions);
        context.variant_fields.clone_from(&self.variant_fields);
        context
            .scalar_value_domains
            .clone_from(&self.scalar_value_domains);
        context
            .closure_environments
            .clone_from(&self.closure_environments);
        context.ad_routes.clone_from(&self.ad_routes);
        context
    }

    /// Consume this context and rehome its metadata onto a table it owns.
    ///
    /// This is the owned-table counterpart of [`Self::with_types`] for the
    /// phases that extend the type arena after analysis: they clone the
    /// analyzed table, intern their synthesized shapes into the clone
    /// (monomorphization shape finalization, AIR backward shape
    /// finalization), then hand the clone here. Every other typed fact moves
    /// with the context — no re-clone — and layouts are rebuilt against the
    /// owned table so freshly interned types resolve exactly like the
    /// originals. The returned context carries its own table; no caller
    /// needs to leak one for the compilation lifetime.
    #[must_use]
    pub fn with_owned_types(self, types: TypeTable) -> Self {
        let Self {
            interner,
            functions,
            method_defs,
            external_function_identities,
            package_product,
            struct_fields,
            struct_field_order,
            variant_field_order,
            display_names,
            display_tokens,
            struct_type_params,
            struct_contracts,
            enum_type_params,
            optional_struct_fields,
            nullable_struct_fields,
            opaque_structs,
            json_struct_field_keys,
            variant_parents,
            enum_variants,
            variant_positions,
            variant_fields,
            scalar_value_domains,
            closure_environments,
            ad_routes,
            ..
        } = self;
        let layouts = MirLayoutTable::from_types(&types);
        Self {
            types: MirTypeTable::Owned(std::rc::Rc::new(types)),
            layouts,
            interner,
            functions,
            method_defs,
            external_function_identities,
            package_product,
            struct_fields,
            struct_field_order,
            variant_field_order,
            display_names,
            display_tokens,
            struct_type_params,
            struct_contracts,
            enum_type_params,
            optional_struct_fields,
            nullable_struct_fields,
            opaque_structs,
            json_struct_field_keys,
            variant_parents,
            enum_variants,
            variant_positions,
            variant_fields,
            scalar_value_domains,
            closure_environments,
            ad_routes,
        }
    }

    #[must_use]
    pub fn enum_representation(&self, enum_def: DefId) -> Option<MirEnumRepresentation> {
        let variants = self.enum_variants.get(&enum_def)?;
        if variants.is_empty() {
            return None;
        }
        let tag_layout = compact_enum_tag_layout(variants.len());

        let mut payload_fields = Vec::new();
        for variant in variants {
            if self.variant_parents.get(variant).copied() != Some(enum_def) {
                return None;
            }
            let variant_fields = self.variant_fields.get(variant)?;
            let mut variant_fields = variant_fields
                .iter()
                .map(|(field, ty)| MirEnumPayloadField {
                    variant: *variant,
                    field: *field,
                    ty: *ty,
                })
                .collect::<Vec<_>>();
            variant_fields.sort_by_key(|field| field.field.0);
            payload_fields.extend(variant_fields);
        }

        Some(self.enum_representation_from_fields(variants, tag_layout, payload_fields))
    }

    pub fn enum_representation_for_type(&self, ty: MirType) -> Option<MirEnumRepresentation> {
        let Type::Applied(base, args) = self.types.get(ty.semantic_id()) else {
            return self
                .enum_def_from_type(ty)
                .and_then(|def_id| self.enum_representation(def_id));
        };
        let Type::Enum(enum_def) = self.types.get(*base) else {
            return self
                .enum_def_from_type(ty)
                .and_then(|def_id| self.enum_representation(def_id));
        };
        let params = self.enum_type_params.get(enum_def)?;
        if params.len() != args.len() {
            return None;
        }
        let type_args = args
            .iter()
            .map(|arg| match arg {
                radix_types::AppliedArg::Type(id) => Some(*id),
                radix_types::AppliedArg::Size(_) => None,
            })
            .collect::<Option<Vec<_>>>()?;
        let bindings = params
            .iter()
            .copied()
            .zip(type_args.into_iter().map(MirType::semantic))
            .collect::<MirGenericBindings>();
        let variants = self.enum_variants.get(enum_def)?;
        let tag_layout = compact_enum_tag_layout(variants.len());
        let mut payload_fields = Vec::new();
        for variant in variants {
            let fields = self.variant_fields.get(variant)?;
            let mut fields = fields
                .iter()
                .map(|(field, field_ty)| {
                    instantiate_existing_generic_type(
                        self.types.as_table(),
                        *field_ty,
                        &bindings,
                        Span::default(),
                    )
                    .ok()
                    .map(|ty| MirEnumPayloadField {
                        variant: *variant,
                        field: *field,
                        ty,
                    })
                })
                .collect::<Option<Vec<_>>>()?;
            fields.sort_by_key(|field| field.field.0);
            payload_fields.extend(fields);
        }
        Some(self.enum_representation_from_fields(variants, tag_layout, payload_fields))
    }

    fn enum_representation_from_fields(
        &self,
        variants: &[DefId],
        tag_layout: MirScalarLayout,
        payload_fields: Vec<MirEnumPayloadField>,
    ) -> MirEnumRepresentation {
        if payload_fields.is_empty() {
            MirEnumRepresentation::UnitTags {
                tag_layout,
                variants: variants.to_vec(),
            }
        } else if let Some((unit_variant, payload)) =
            self.pointer_niche_payload(variants, &payload_fields)
        {
            MirEnumRepresentation::PointerNiche {
                unit_variant,
                payload,
            }
        } else if let Some((unit_variant, payload)) =
            self.boolean_payload(variants, &payload_fields)
        {
            MirEnumRepresentation::BooleanPayload {
                unit_variant,
                payload,
            }
        } else if let Some((unit_variant, payload, payload_layout, carrier_layout, sentinel)) =
            self.integer_payload(variants, &payload_fields)
        {
            MirEnumRepresentation::IntegerPayload {
                unit_variant,
                payload,
                payload_layout,
                carrier_layout,
                sentinel,
            }
        } else if let Some((unit_variant, payload, payload_layout)) =
            self.float_payload(variants, &payload_fields)
        {
            MirEnumRepresentation::FloatPayload {
                unit_variant,
                payload,
                payload_layout,
            }
        } else if let Some(payload_ty) = tagged_union_payload_ty(variants, &payload_fields) {
            MirEnumRepresentation::TaggedUnion {
                tag_layout,
                fields: payload_fields,
                payload_ty,
            }
        } else {
            MirEnumRepresentation::TaggedProduct {
                tag_layout,
                fields: payload_fields,
            }
        }
    }

    fn enum_def_from_type(&self, ty: MirType) -> Option<DefId> {
        match self.types.get(ty.semantic_id()) {
            Type::Enum(def_id) => Some(*def_id),
            Type::Applied(base, _) | Type::Alias(_, base) | Type::Ref(_, base) => {
                self.enum_def_from_type(MirType::semantic(*base))
            }
            _ => None,
        }
    }

    #[must_use]
    pub fn enum_variant_tag(&self, enum_def: DefId, variant: DefId) -> Option<usize> {
        if self.variant_parents.get(&variant).copied() != Some(enum_def) {
            return None;
        }
        if let Some(&position) = self.variant_positions.get(&variant) {
            return Some(position);
        }
        // Fallback when callers filled `enum_variants` without rebuilding the index.
        self.enum_variants
            .get(&enum_def)?
            .iter()
            .position(|candidate| *candidate == variant)
    }

    #[must_use]
    pub fn enum_payload_fields(&self, enum_def: DefId) -> Option<Vec<MirEnumPayloadField>> {
        match self.enum_representation(enum_def)? {
            MirEnumRepresentation::PointerNiche { payload, .. }
            | MirEnumRepresentation::BooleanPayload { payload, .. }
            | MirEnumRepresentation::IntegerPayload { payload, .. }
            | MirEnumRepresentation::FloatPayload { payload, .. } => Some(vec![payload]),
            MirEnumRepresentation::TaggedProduct { fields, .. }
            | MirEnumRepresentation::TaggedUnion { fields, .. } => Some(fields),
            MirEnumRepresentation::UnitTags { .. } => Some(Vec::new()),
        }
    }

    #[must_use]
    pub fn enum_tag_layout(&self, enum_def: DefId) -> Option<MirScalarLayout> {
        match self.enum_representation(enum_def)? {
            MirEnumRepresentation::UnitTags { tag_layout, .. }
            | MirEnumRepresentation::TaggedUnion { tag_layout, .. }
            | MirEnumRepresentation::TaggedProduct { tag_layout, .. } => Some(tag_layout),
            MirEnumRepresentation::PointerNiche { .. }
            | MirEnumRepresentation::BooleanPayload { .. }
            | MirEnumRepresentation::IntegerPayload { .. }
            | MirEnumRepresentation::FloatPayload { .. } => None,
        }
    }

    fn pointer_niche_payload(
        &self,
        variants: &[DefId],
        payload_fields: &[MirEnumPayloadField],
    ) -> Option<(DefId, MirEnumPayloadField)> {
        if variants.len() != 2 || payload_fields.len() != 1 {
            return None;
        }
        let payload = payload_fields[0];
        let unit_variant = variants
            .iter()
            .copied()
            .find(|variant| *variant != payload.variant)?;
        if !self.payload_has_pointer_niche(payload.ty) {
            return None;
        }
        Some((unit_variant, payload))
    }

    fn payload_has_pointer_niche(&self, ty: MirType) -> bool {
        if matches!(
            self.types.get(ty.semantic_id()),
            Type::Primitive(Primitive::Nihil)
        ) {
            return false;
        }
        self.layouts
            .layout_for_type(ty)
            .is_ok_and(|layout| matches!(layout.kind, MirLayoutKind::OpaqueHandle))
    }

    fn boolean_payload(
        &self,
        variants: &[DefId],
        payload_fields: &[MirEnumPayloadField],
    ) -> Option<(DefId, MirEnumPayloadField)> {
        if variants.len() != 2 || payload_fields.len() != 1 {
            return None;
        }
        let payload = payload_fields[0];
        let unit_variant = variants
            .iter()
            .copied()
            .find(|variant| *variant != payload.variant)?;
        if !self.payload_has_boolean_domain(payload.ty) {
            return None;
        }
        Some((unit_variant, payload))
    }

    fn payload_has_boolean_domain(&self, ty: MirType) -> bool {
        self.layouts
            .layout_for_type(ty)
            .is_ok_and(|layout| matches!(layout.kind, MirLayoutKind::Scalar(MirScalarLayout::Bool)))
    }

    fn integer_payload(
        &self,
        variants: &[DefId],
        payload_fields: &[MirEnumPayloadField],
    ) -> Option<(
        DefId,
        MirEnumPayloadField,
        MirScalarLayout,
        MirScalarLayout,
        i128,
    )> {
        if variants.len() != 2 || payload_fields.len() != 1 {
            return None;
        }
        let payload = payload_fields[0];
        let unit_variant = variants
            .iter()
            .copied()
            .find(|variant| *variant != payload.variant)?;
        let layout = self.layouts.layout_for_type(payload.ty).ok()?;
        let MirLayoutKind::Scalar(payload_layout) = layout.kind else {
            return None;
        };
        if let Some((carrier_layout, sentinel)) =
            self.range_restricted_integer_payload_carrier(payload.ty, payload_layout)
        {
            return Some((
                unit_variant,
                payload,
                payload_layout,
                carrier_layout,
                sentinel,
            ));
        }
        let (carrier_layout, sentinel) = integer_payload_carrier(payload_layout)?;
        Some((
            unit_variant,
            payload,
            payload_layout,
            carrier_layout,
            sentinel,
        ))
    }

    fn range_restricted_integer_payload_carrier(
        &self,
        payload_ty: MirType,
        payload_layout: MirScalarLayout,
    ) -> Option<(MirScalarLayout, i128)> {
        let domain = self.scalar_value_domains.get(&payload_ty.semantic_id())?;
        let MirScalarValueDomain::IntegerRange { min, max } = *domain else {
            return None;
        };
        let sentinel = range_restricted_integer_sentinel(payload_layout, min, max)?;
        Some((payload_layout, sentinel))
    }

    fn float_payload(
        &self,
        variants: &[DefId],
        payload_fields: &[MirEnumPayloadField],
    ) -> Option<(DefId, MirEnumPayloadField, MirScalarLayout)> {
        if variants.len() != 2 || payload_fields.len() != 1 {
            return None;
        }
        let payload = payload_fields[0];
        let unit_variant = variants
            .iter()
            .copied()
            .find(|variant| *variant != payload.variant)?;
        if !matches!(
            self.scalar_value_domains.get(&payload.ty.semantic_id()),
            Some(MirScalarValueDomain::FloatNonNan | MirScalarValueDomain::FloatFinite)
        ) {
            return None;
        }
        let layout = self.layouts.layout_for_type(payload.ty).ok()?;
        let MirLayoutKind::Scalar(payload_layout @ (MirScalarLayout::F32 | MirScalarLayout::F64)) =
            layout.kind
        else {
            return None;
        };
        Some((unit_variant, payload, payload_layout))
    }
}

pub const INTEGER_PAYLOAD_I8_SENTINEL: i128 = 1_i128 << 7;
pub const INTEGER_PAYLOAD_U8_SENTINEL: i128 = 1_i128 << 8;
pub const INTEGER_PAYLOAD_I16_SENTINEL: i128 = 1_i128 << 15;
pub const INTEGER_PAYLOAD_U16_SENTINEL: i128 = 1_i128 << 16;
pub const INTEGER_PAYLOAD_I32_SENTINEL: i128 = 1_i128 << 31;
pub const INTEGER_PAYLOAD_U32_SENTINEL: i128 = 1_i128 << 32;
pub const INTEGER_PAYLOAD_I64_SENTINEL: i128 = 1_i128 << 63;
pub const INTEGER_PAYLOAD_U64_SENTINEL: i128 = 1_i128 << 64;

fn integer_payload_carrier(payload: MirScalarLayout) -> Option<(MirScalarLayout, i128)> {
    match payload {
        MirScalarLayout::I8 => Some((MirScalarLayout::I16, INTEGER_PAYLOAD_I8_SENTINEL)),
        MirScalarLayout::U8 => Some((MirScalarLayout::U16, INTEGER_PAYLOAD_U8_SENTINEL)),
        MirScalarLayout::I16 => Some((MirScalarLayout::I32, INTEGER_PAYLOAD_I16_SENTINEL)),
        MirScalarLayout::U16 => Some((MirScalarLayout::U32, INTEGER_PAYLOAD_U16_SENTINEL)),
        MirScalarLayout::I32 => Some((MirScalarLayout::I64, INTEGER_PAYLOAD_I32_SENTINEL)),
        MirScalarLayout::U32 => Some((MirScalarLayout::U64, INTEGER_PAYLOAD_U32_SENTINEL)),
        MirScalarLayout::I64 => Some((MirScalarLayout::I128, INTEGER_PAYLOAD_I64_SENTINEL)),
        MirScalarLayout::U64 => Some((MirScalarLayout::I128, INTEGER_PAYLOAD_U64_SENTINEL)),
        // `Magnus` is unbounded: it has no fixed carrier to widen into.
        MirScalarLayout::Bool
        | MirScalarLayout::I128
        | MirScalarLayout::F16
        | MirScalarLayout::Bf16
        | MirScalarLayout::F32
        | MirScalarLayout::F64
        | MirScalarLayout::Magnus
        | MirScalarLayout::D64 => None,
    }
}

fn range_restricted_integer_sentinel(
    payload: MirScalarLayout,
    min: i128,
    max: i128,
) -> Option<i128> {
    let (layout_min, layout_max) = integer_layout_bounds(payload)?;
    if min > max || min < layout_min || max > layout_max {
        return None;
    }
    if min > layout_min {
        return Some(min - 1);
    }
    if max < layout_max {
        return Some(max + 1);
    }
    None
}

fn integer_layout_bounds(layout: MirScalarLayout) -> Option<(i128, i128)> {
    match layout {
        MirScalarLayout::I8 => Some((i128::from(i8::MIN), i128::from(i8::MAX))),
        MirScalarLayout::I16 => Some((i128::from(i16::MIN), i128::from(i16::MAX))),
        MirScalarLayout::I32 => Some((i128::from(i32::MIN), i128::from(i32::MAX))),
        MirScalarLayout::I64 => Some((i128::from(i64::MIN), i128::from(i64::MAX))),
        MirScalarLayout::U8 => Some((0, i128::from(u8::MAX))),
        MirScalarLayout::U16 => Some((0, i128::from(u16::MAX))),
        MirScalarLayout::U32 => Some((0, i128::from(u32::MAX))),
        MirScalarLayout::U64 => Some((0, i128::from(u64::MAX))),
        // `Magnus` has no layout bounds (unbounded).
        MirScalarLayout::Bool
        | MirScalarLayout::I128
        | MirScalarLayout::F16
        | MirScalarLayout::Bf16
        | MirScalarLayout::F32
        | MirScalarLayout::F64
        | MirScalarLayout::Magnus
        | MirScalarLayout::D64 => None,
    }
}

fn tagged_union_payload_ty(
    variants: &[DefId],
    payload_fields: &[MirEnumPayloadField],
) -> Option<MirType> {
    let mut payload_ty = None;
    for variant in variants {
        let mut fields = payload_fields
            .iter()
            .filter(|field| field.variant == *variant);
        let Some(field) = fields.next() else {
            continue;
        };
        if fields.next().is_some() {
            return None;
        }
        match payload_ty {
            Some(existing) if existing != field.ty => return None,
            Some(_) => {}
            None => payload_ty = Some(field.ty),
        }
    }
    payload_ty
}

fn compact_enum_tag_layout(variant_count: usize) -> MirScalarLayout {
    let max_tag = variant_count.saturating_sub(1);
    if u8::try_from(max_tag).is_ok() {
        MirScalarLayout::U8
    } else if u16::try_from(max_tag).is_ok() {
        MirScalarLayout::U16
    } else if u32::try_from(max_tag).is_ok() {
        MirScalarLayout::U32
    } else {
        MirScalarLayout::U64
    }
}
