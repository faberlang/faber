//! MIR validation before inspection and backend experiments.
//!
//! Validation checks that MIR is well-formed relative to the semantic type
//! table and the auxiliary signature/field metadata supplied by the lowerer or
//! caller. It catches duplicate IDs, missing local/temp/block/value references,
//! invalid CFG targets, unresolved/error recovery types, call arity/type
//! mismatches, destination typing, aggregate shape mismatches, projection
//! metadata gaps, option-operation contracts, and runtime intrinsic policies.
//!
//! ERROR STRATEGY
//! ==============
//! Validation accumulates all errors it can find and returns them as one vector.
//! It never repairs MIR and it does not guess missing type metadata; a missing
//! field table or failable-call signature is a producer bug that should remain
//! visible to the caller.
//!
//! LIMITS
//! ======
//! This pass verifies MIR shape below typed HIR. It is not a full semantic
//! analyzer or borrow checker, and it currently tracks statement-defined
//! `MirValueId`s within each block rather than globally. CFG reachability and
//! termination are enforced by `validate_cfg`.

use crate::lista_tensor_conversio::validate_lista_to_tensor_source;
use crate::visit::{terminator_successors, MirVisitor};
use crate::{
    instantiate_existing_generic_type, MirAggregate, MirAggregateCarrier, MirAggregateFamily,
    MirAggregateFields, MirAggregateItem, MirAggregateKind, MirAtomicOp, MirBinOp, MirBlock,
    MirBlockId, MirCallee, MirClosureCallee, MirClosureEnvironment, MirClosureEnvironmentId,
    MirClosureValue, MirCollectionOp, MirConstant, MirConversion, MirExternalFunctionIdentity,
    MirFunction, MirFunctionId, MirGenericBindings, MirGradientOp, MirIntrinsic, MirKernelBuiltin,
    MirLayoutKind, MirLayoutTable, MirLocal, MirLocalId, MirNamedOperand, MirOperand,
    MirOptionChainLink, MirOptionOp, MirParam, MirPlace, MirPlaceBase, MirProgram, MirProjection,
    MirRuntimeCall, MirScalarLayout, MirSermoOp, MirStatement, MirStatementKind, MirTemp,
    MirTempId, MirTerminator, MirTerminatorKind, MirType, MirUnOp, MirValue, MirValueId,
    MirValueKind,
};
use radix_hir::DefId;
use radix_lexer::Interner;
use radix_lexer::{Span, Symbol};
use radix_types::{FuncSig, IndexExpr, NumericWidth, Primitive, Type, TypeId, TypeTable};
use rustc_hash::{FxHashMap, FxHashSet};

/// Validation diagnostic for malformed MIR.
///
/// Spans are carried from the offending MIR node so callers can tie structural
/// failures back to the source region that produced the bad representation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirValidationError {
    pub message: String,
    pub issue: String,
    pub span: Span,
}

impl MirValidationError {
    fn new(span: Span, message: impl Into<String>) -> Self {
        let message = message.into();
        let issue = validation_issue(&message);
        Self {
            message,
            issue,
            span,
        }
    }
}

fn validation_issue(message: &str) -> String {
    let mut issue = String::from("mir_validation");
    let mut separated = false;
    for ch in message.chars() {
        if ch.is_ascii_alphanumeric() {
            if !separated {
                issue.push('_');
            }
            issue.push(ch.to_ascii_lowercase());
            separated = true;
        } else if !issue.ends_with('_') {
            issue.push('_');
            separated = true;
        }
    }
    issue.trim_end_matches('_').to_string()
}

/// One callable parameter slot in a MIR validation signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirParamSignature {
    pub ty: MirType,
    pub optional: bool,
}

/// Known callable signature used by MIR validation.
///
/// Direct MIR functions can derive this from their parameter locals and return
/// type. External or not-yet-lowered callees are supplied through
/// `MirValidationContext::functions`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirFunctionSignature {
    pub params: Vec<MirParamSignature>,
    pub return_ty: MirType,
    pub error_ty: Option<MirType>,
}

type GenericTypeBindings = MirGenericBindings;

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

/// Metadata needed to validate MIR against the semantic world it was lowered from.
///
/// `types` is authoritative for assignability and primitive IDs. Function and
/// field maps bridge MIR references back to HIR/semantic declarations where MIR
/// only carries `DefId`s and symbols.
#[derive(Clone)]
pub struct MirValidationContext<'a> {
    pub types: &'a TypeTable,
    pub layouts: MirLayoutTable,
    pub interner: Option<&'a Interner>,
    pub functions: FxHashMap<DefId, MirFunctionSignature>,
    pub external_function_identities: FxHashMap<DefId, MirExternalFunctionIdentity>,
    pub struct_fields: FxHashMap<DefId, FxHashMap<Symbol, MirType>>,
    pub struct_type_params: FxHashMap<DefId, Vec<Symbol>>,
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
}

impl<'a> MirValidationContext<'a> {
    #[must_use]
    pub fn new(types: &'a TypeTable) -> Self {
        Self {
            types,
            layouts: MirLayoutTable::from_types(types),
            interner: None,
            functions: FxHashMap::default(),
            external_function_identities: FxHashMap::default(),
            struct_fields: FxHashMap::default(),
            struct_type_params: FxHashMap::default(),
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
        context.interner = self.interner;
        context.functions = self.functions.clone();
        context.external_function_identities = self.external_function_identities.clone();
        context.struct_fields = self.struct_fields.clone();
        context.struct_type_params = self.struct_type_params.clone();
        context.enum_type_params = self.enum_type_params.clone();
        context.optional_struct_fields = self.optional_struct_fields.clone();
        context.nullable_struct_fields = self.nullable_struct_fields.clone();
        context.opaque_structs = self.opaque_structs.clone();
        context.json_struct_field_keys = self.json_struct_field_keys.clone();
        context.variant_parents = self.variant_parents.clone();
        context.enum_variants = self.enum_variants.clone();
        context.variant_positions = self.variant_positions.clone();
        context.variant_fields = self.variant_fields.clone();
        context.scalar_value_domains = self.scalar_value_domains.clone();
        context.closure_environments = self.closure_environments.clone();
        context
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
        let bindings = params
            .iter()
            .copied()
            .zip(args.iter().copied().map(MirType::semantic))
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
                        self.types,
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
        MirScalarLayout::Bool
        | MirScalarLayout::I128
        | MirScalarLayout::F16
        | MirScalarLayout::F32
        | MirScalarLayout::F64 => None,
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
        MirScalarLayout::Bool
        | MirScalarLayout::I128
        | MirScalarLayout::F16
        | MirScalarLayout::F32
        | MirScalarLayout::F64 => None,
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

/// Validated MIR program proof — the type-level gate for all public MIR entry points.
///
/// `ValidatedMir` wraps a [`MirProgram`] together with the
/// [`MirValidationContext`] that proved it well-formed. It can only be
/// constructed by passing the program and context through
/// [`validate_program`]; there is no escape hatch.
///
/// Callers that already hold a program and a known-good context can call
/// [`ValidatedMir::new`], which re-runs validation to produce the token.
///
/// Empty-context substitution is impossible: the constructor consumes the
/// context, so a bare `MirValidationContext::new(types)` can never produce
/// a `ValidatedMir` unless the program passes validation against it.
///
/// The token is immutable after construction. Callers that need to augment
/// the context (e.g. merging closure environments during package linking)
/// must clone the context, modify the clone, and construct a fresh token.
pub struct ValidatedMir<'a> {
    program: MirProgram,
    validation: MirValidationContext<'a>,
}

impl std::fmt::Debug for ValidatedMir<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ValidatedMir")
            .field("program", &self.program)
            .finish_non_exhaustive()
    }
}

impl Clone for ValidatedMir<'_> {
    fn clone(&self) -> Self {
        Self {
            program: self.program.clone(),
            validation: self.validation.clone(),
        }
    }
}

impl<'a> ValidatedMir<'a> {
    /// Build a validated-MIR token by running MIR validation.
    ///
    /// # Errors
    /// Returns [`Vec<MirValidationError>`] when the program does not satisfy
    /// all MIR well-formedness checks.
    pub fn new(
        program: MirProgram,
        mut context: MirValidationContext<'a>,
    ) -> Result<Self, Vec<MirValidationError>> {
        context.rebuild_variant_positions();
        validate_program(&program, &context)?;
        Ok(Self {
            program,
            validation: context,
        })
    }

    /// The validated inner program.
    #[must_use]
    pub fn program(&self) -> &MirProgram {
        &self.program
    }

    /// The proving validation context consumed during construction.
    #[must_use]
    pub fn validation(&self) -> &MirValidationContext<'a> {
        &self.validation
    }
}

/// Validate one MIR program and collect all structural/type-contract errors.
///
/// Validation is deliberately caller-supplied-context driven: MIR carries stable
/// references, while the context provides the semantic type table and signature
/// metadata needed to prove those references are coherent.
/// # Errors
/// Returns an error when validation or type resolution fails. See [`Vec<MirValidationError`].
pub fn validate_program(
    program: &MirProgram,
    context: &MirValidationContext<'_>,
) -> Result<(), Vec<MirValidationError>> {
    let mut validator = Validator {
        program,
        context,
        errors: Vec::new(),
    };
    validator.validate_program();
    if validator.errors.is_empty() {
        Ok(())
    } else {
        Err(validator.errors)
    }
}

struct Validator<'a, 'ctx> {
    program: &'a MirProgram,
    context: &'ctx MirValidationContext<'ctx>,
    errors: Vec<MirValidationError>,
}

struct FunctionScope<'prog, 'map> {
    function: &'prog MirFunction,
    /// Shared program-wide index — one map for all functions (PRP-053).
    functions_by_id: &'map FxHashMap<MirFunctionId, &'prog MirFunction>,
    locals: FxHashMap<MirLocalId, MirType>,
    temps: FxHashMap<MirTempId, MirType>,
    blocks: FxHashSet<MirBlockId>,
    /// Values are statement-local products in block order; clearing them per
    /// block prevents accidental cross-block SSA assumptions in this early MIR.
    values: FxHashMap<MirValueId, MirType>,
}

struct DuplicateFunctionFinder {
    ids: FxHashSet<MirFunctionId>,
    duplicates: Vec<(MirFunctionId, Span)>,
}

impl DuplicateFunctionFinder {
    fn find(program: &MirProgram) -> Vec<(MirFunctionId, Span)> {
        let mut finder = Self {
            ids: FxHashSet::default(),
            duplicates: Vec::new(),
        };
        finder.visit_program(program);
        finder.duplicates
    }
}

impl MirVisitor for DuplicateFunctionFinder {
    fn visit_function(&mut self, function: &MirFunction) {
        if !self.ids.insert(function.id) {
            self.duplicates.push((function.id, function.span));
        }
    }
}

struct FunctionShape {
    locals: FxHashMap<MirLocalId, MirType>,
    temps: FxHashMap<MirTempId, MirType>,
    blocks: FxHashSet<MirBlockId>,
    param_locals: FxHashSet<MirLocalId>,
    declared_locals: FxHashSet<MirLocalId>,
    duplicate_param_locals: Vec<(MirLocalId, Span)>,
    duplicate_locals: Vec<(MirLocalId, Span)>,
    conflicting_locals: Vec<(MirLocalId, Span)>,
    duplicate_temps: Vec<(MirTempId, Span)>,
    duplicate_blocks: Vec<(MirBlockId, Span)>,
}

impl FunctionShape {
    fn collect(function: &MirFunction) -> Self {
        let mut shape = Self {
            locals: FxHashMap::default(),
            temps: FxHashMap::default(),
            blocks: FxHashSet::default(),
            param_locals: FxHashSet::default(),
            declared_locals: FxHashSet::default(),
            duplicate_param_locals: Vec::new(),
            duplicate_locals: Vec::new(),
            conflicting_locals: Vec::new(),
            duplicate_temps: Vec::new(),
            duplicate_blocks: Vec::new(),
        };
        shape.visit_function(function);
        shape
    }
}

impl MirVisitor for FunctionShape {
    fn visit_param(&mut self, param: &MirParam) {
        if !self.param_locals.insert(param.local) {
            self.duplicate_param_locals.push((param.local, param.span));
            return;
        }
        self.locals.insert(param.local, param.ty);
    }

    fn visit_local(&mut self, local: &MirLocal) {
        if !self.declared_locals.insert(local.id) {
            self.duplicate_locals.push((local.id, local.span));
        }
        match self.locals.insert(local.id, local.ty) {
            Some(existing) if existing.semantic_id() != local.ty.semantic_id() => {
                self.conflicting_locals.push((local.id, local.span));
            }
            _ => {}
        }
    }

    fn visit_temp(&mut self, temp: &MirTemp) {
        if self.temps.insert(temp.id, temp.ty).is_some() {
            self.duplicate_temps.push((temp.id, temp.span));
        }
    }

    fn visit_block(&mut self, block: &MirBlock) {
        if !self.blocks.insert(block.id) {
            self.duplicate_blocks.push((block.id, block.span));
        }
    }
}

fn signature_from_function(function: &MirFunction) -> MirFunctionSignature {
    MirFunctionSignature {
        params: function
            .params
            .iter()
            .map(|param| MirParamSignature {
                ty: param.ty,
                optional: false,
            })
            .collect(),
        return_ty: function.return_ty,
        error_ty: function.error_ty,
    }
}

fn signature_from_func_sig(sig: &FuncSig) -> MirFunctionSignature {
    MirFunctionSignature {
        params: sig
            .params
            .iter()
            .map(|param| MirParamSignature {
                ty: MirType::semantic(param.ty),
                optional: param.optional,
            })
            .collect(),
        return_ty: MirType::semantic(sig.ret),
        error_ty: sig.err.map(MirType::semantic),
    }
}

impl Validator<'_, '_> {
    fn validate_program(&mut self) {
        for (id, span) in DuplicateFunctionFinder::find(self.program) {
            self.error(span, format!("duplicate MIR function id f{}", id.0));
        }
        // Build the function index once; every FunctionScope borrows it (PRP-053).
        // Prior path cloned the full map per function (O(N²) hash-map copies).
        let functions_by_id = self
            .program
            .functions
            .iter()
            .map(|function| (function.id, function))
            .collect::<FxHashMap<_, _>>();

        for function in &self.program.functions {
            self.validate_function(function, &functions_by_id);
        }
    }

    fn validate_function(
        &mut self,
        function: &MirFunction,
        functions_by_id: &FxHashMap<MirFunctionId, &MirFunction>,
    ) {
        // First prove declaration shape, then validate executable block bodies
        // against maps built from that shape. This keeps later diagnostics about
        // bad uses from being polluted by partially collected declarations.
        self.validate_mir_type(function.return_ty, function.span);
        if let Some(error_ty) = function.error_ty {
            self.validate_mir_type(error_ty, function.span);
        }
        for param in &function.params {
            self.validate_mir_type(param.ty, param.span);
        }
        for local in &function.locals {
            self.validate_mir_type(local.ty, local.span);
        }
        for temp in &function.temps {
            self.validate_mir_type(temp.ty, temp.span);
        }
        let shape = FunctionShape::collect(function);
        self.report_duplicate_local_ids(&shape.duplicate_param_locals);
        self.report_duplicate_local_ids(&shape.duplicate_locals);
        for (id, span) in &shape.conflicting_locals {
            self.error(
                *span,
                format!("duplicate MIR local id _{} has conflicting type", id.0),
            );
        }
        for (id, span) in &shape.duplicate_temps {
            self.error(*span, format!("duplicate MIR temp id %{}", id.0));
        }
        for (id, span) in &shape.duplicate_blocks {
            self.error(*span, format!("duplicate MIR block id bb{}", id.0));
        }
        let mut scope = FunctionScope {
            function,
            functions_by_id,
            locals: shape.locals,
            temps: shape.temps,
            blocks: shape.blocks,
            values: FxHashMap::default(),
        };

        for block in &function.blocks {
            self.validate_block(&mut scope, block);
        }

        self.validate_cfg(function, &scope);
    }

    fn validate_block(&mut self, scope: &mut FunctionScope<'_, '_>, block: &MirBlock) {
        scope.values.clear();
        for stmt in &block.statements {
            self.validate_stmt(scope, stmt);
        }
        self.validate_terminator(scope, &block.terminator);
    }

    fn validate_stmt(&mut self, scope: &mut FunctionScope<'_, '_>, stmt: &MirStatement) {
        match &stmt.kind {
            MirStatementKind::Assign { place, value } => {
                let place_ty = self.validate_place(scope, place, stmt.span);
                self.validate_value(scope, value);
                if let Some(place_ty) = place_ty {
                    self.require_assignable(
                        value.ty,
                        place_ty,
                        value.span,
                        "assigned value type does not match destination",
                    );
                }
                scope.values.insert(value.id, value.ty);
            }
            MirStatementKind::Call {
                destination,
                callee,
                args,
            } => {
                let sig = self.validate_callee(scope, callee, stmt.span);
                if let Some(sig) = &sig {
                    if sig.error_ty.is_some() {
                        self.error(stmt.span, "ordinary call callee is failable; use try_call");
                    }
                }
                let mut generic_bindings = GenericTypeBindings::default();
                self.validate_call_args(
                    scope,
                    sig.as_ref(),
                    args,
                    stmt.span,
                    &mut generic_bindings,
                );
                self.validate_optional_destination(
                    scope,
                    destination.as_ref(),
                    sig.as_ref().map(|sig| sig.return_ty),
                    stmt.span,
                    &mut generic_bindings,
                );
            }
            MirStatementKind::RuntimeCall { destination, call } => {
                self.validate_runtime_call(scope, destination.as_ref(), call, stmt.span);
            }
            MirStatementKind::Construct {
                destination,
                aggregate,
            } => {
                let destination_ty = self.validate_place(scope, destination, stmt.span);
                self.validate_aggregate(scope, aggregate, stmt.span);
                if let Some(destination_ty) = destination_ty {
                    self.require_assignable(
                        aggregate.ty,
                        destination_ty,
                        stmt.span,
                        "construct destination type does not match aggregate type",
                    );
                }
            }
        }
    }

    fn validate_terminator(
        &mut self,
        scope: &mut FunctionScope<'_, '_>,
        terminator: &MirTerminator,
    ) {
        for target in terminator_successors(&terminator.kind) {
            self.require_block(scope, target, terminator.span);
        }

        match &terminator.kind {
            MirTerminatorKind::Return(None) => {
                if !self.is_unit_return(scope.function.return_ty)
                    && !scope.function.is_generator
                    && !scope.function.is_async
                {
                    self.error(
                        terminator.span,
                        "no-value return in value-returning function",
                    );
                }
            }
            MirTerminatorKind::Return(Some(value)) => {
                if let Some(value_ty) = self.validate_operand(scope, value, terminator.span) {
                    self.require_assignable(
                        value_ty,
                        scope.function.return_ty,
                        terminator.span,
                        "return type mismatch",
                    );
                }
            }
            MirTerminatorKind::ReturnError(value) => {
                let Some(error_ty) = scope.function.error_ty else {
                    self.error(
                        terminator.span,
                        "return_error in function without alternate-exit type",
                    );
                    self.validate_operand(scope, value, terminator.span);
                    return;
                };
                if let Some(value_ty) = self.validate_operand(scope, value, terminator.span) {
                    self.require_assignable(
                        value_ty,
                        error_ty,
                        terminator.span,
                        "return_error type mismatch",
                    );
                }
            }
            MirTerminatorKind::TryCall {
                destination,
                callee,
                args,
                error_place,
                ..
            } => {
                let sig = self.validate_callee(scope, callee, terminator.span);
                let mut generic_bindings = GenericTypeBindings::default();
                self.validate_call_args(
                    scope,
                    sig.as_ref(),
                    args,
                    terminator.span,
                    &mut generic_bindings,
                );
                self.validate_optional_destination(
                    scope,
                    destination.as_ref(),
                    sig.as_ref().map(|sig| sig.return_ty),
                    terminator.span,
                    &mut generic_bindings,
                );
                let error_place_ty = self.validate_place(scope, error_place, terminator.span);
                if sig.is_none() {
                    self.error(
                        terminator.span,
                        "try_call callee does not have a known failable signature",
                    );
                }
                match (sig.as_ref().and_then(|sig| sig.error_ty), error_place_ty) {
                    (Some(error_ty), Some(place_ty)) => {
                        self.bind_generic_type_witness(
                            error_ty,
                            place_ty,
                            &mut generic_bindings,
                            terminator.span,
                            "try_call error place type mismatch",
                        );
                        if let Some(error_ty) = self.instantiate_generic_type(
                            error_ty,
                            &generic_bindings,
                            terminator.span,
                        ) {
                            self.require_assignable(
                                error_ty,
                                place_ty,
                                terminator.span,
                                "try_call error place type mismatch",
                            );
                        }
                    }
                    (None, _) if sig.is_some() => {
                        self.error(terminator.span, "try_call callee is not known failable");
                    }
                    _ => {}
                }
            }
            MirTerminatorKind::Goto(_) => {}
            MirTerminatorKind::Branch { condition, .. } => {
                if let Some(condition_ty) = self.validate_operand(scope, condition, terminator.span)
                {
                    self.require_exact(
                        condition_ty,
                        self.primitive(Primitive::Bivalens),
                        terminator.span,
                        "branch condition is not bivalens",
                    );
                }
            }
            MirTerminatorKind::Switch { value, cases, .. } => {
                self.validate_operand(scope, value, terminator.span);
                if cases.is_empty() {
                    self.error(terminator.span, "switch has no cases");
                }
                for (i, case) in cases.iter().enumerate() {
                    if cases[..i].iter().any(|prev| prev.value == case.value) {
                        self.error(terminator.span, "duplicate switch case value");
                    }
                }
            }
            MirTerminatorKind::Unreachable => {}
        }
    }

    /// Validate CFG invariants: entry block existence, reachability, and
    /// termination. Called after per-block validation so successor targets
    /// have already been checked for existence.
    fn validate_cfg(&mut self, function: &MirFunction, scope: &FunctionScope<'_, '_>) {
        let entry = MirBlockId(0);

        // 1. Entry block must exist.
        if !scope.blocks.contains(&entry) {
            self.error(function.span, "function has no entry block bb0");
            return;
        }

        // 2. Reachability — BFS from entry block.
        let block_map: FxHashMap<MirBlockId, &MirBlock> =
            function.blocks.iter().map(|b| (b.id, b)).collect();

        // Collect the set of all blocks that are referenced as successors
        // by at least one terminator. Blocks not referenced by any terminator
        // (e.g. join points sealed as Unreachable) are dead code created by
        // the lowerer — they are not orphan bugs.
        let mut successor_targets = FxHashSet::default();
        for block in &function.blocks {
            for succ in terminator_successors(&block.terminator.kind) {
                successor_targets.insert(succ);
            }
        }

        let mut reachable = FxHashSet::default();
        let mut stack = vec![entry];
        reachable.insert(entry);
        while let Some(current) = stack.pop() {
            if let Some(block) = block_map.get(&current) {
                for succ in terminator_successors(&block.terminator.kind) {
                    if reachable.insert(succ) {
                        stack.push(succ);
                    }
                }
            }
        }

        for block in &function.blocks {
            // Only flag blocks that are referenced as successor targets but are
            // not reachable from entry. These are genuine orphans: some
            // terminator tries to reach them but no path from entry exists.
            //
            // Blocks that no terminator references are dead code (unused join
            // points, eliminated paths) — the lowerer creates them and later
            // passes clean them up. They are not reachability bugs.
            if successor_targets.contains(&block.id) && !reachable.contains(&block.id) {
                self.error(
                    block.span,
                    format!("block bb{} is not reachable from entry", block.id.0),
                );
            }
        }

        // 3. Termination — every reachable block must have at least one path
        //    to Return / ReturnError / Unreachable.
        //
        //    Reverse-BFS from seed terminators: mark all blocks that can
        //    reach a seed by following predecessor edges backward. This
        //    accepts loops (the exit reaches Return) while rejecting pure
        //    Goto cycles (no path to any seed).

        // Build predecessor map: for each block, which blocks target it.
        let mut predecessors: FxHashMap<MirBlockId, Vec<MirBlockId>> = FxHashMap::default();
        for block in &function.blocks {
            for succ in terminator_successors(&block.terminator.kind) {
                predecessors.entry(succ).or_default().push(block.id);
            }
        }

        // Identify seeds: blocks whose terminator is Return/ReturnError/Unreachable.
        let mut reaches_terminator = FxHashSet::default();
        let mut stack: Vec<MirBlockId> = Vec::new();
        for block in &function.blocks {
            match &block.terminator.kind {
                MirTerminatorKind::Return(_)
                | MirTerminatorKind::ReturnError(_)
                | MirTerminatorKind::Unreachable => {
                    reaches_terminator.insert(block.id);
                    stack.push(block.id);
                }
                _ => {}
            }
        }

        // Reverse-BFS: follow predecessor edges backward from seeds.
        while let Some(current) = stack.pop() {
            if let Some(preds) = predecessors.get(&current) {
                for pred in preds {
                    if reaches_terminator.insert(*pred) {
                        stack.push(*pred);
                    }
                }
            }
        }

        for block in &function.blocks {
            if reachable.contains(&block.id) && !reaches_terminator.contains(&block.id) {
                self.error(
                    block.span,
                    format!(
                        "block bb{} does not reach a terminator \
                         (Return/ReturnError/Unreachable) on every path",
                        block.id.0,
                    ),
                );
            }
        }
    }

    fn validate_value(&mut self, scope: &FunctionScope<'_, '_>, value: &MirValue) {
        self.validate_mir_type(value.ty, value.span);
        match &value.kind {
            MirValueKind::Operand(operand) => {
                self.validate_operand(scope, operand, value.span);
            }
            MirValueKind::Closure(closure) => {
                self.validate_closure_value(scope, closure, value.ty, value.span);
            }
            MirValueKind::Unary { op, operand } => {
                let operand_ty = self.validate_operand(scope, operand, value.span);
                match op {
                    MirUnOp::Not => {
                        if let Some(operand_ty) = operand_ty {
                            self.require_exact(
                                operand_ty,
                                self.primitive(Primitive::Bivalens),
                                value.span,
                                "not operand is not bivalens",
                            );
                        }
                        self.require_exact(
                            value.ty,
                            self.primitive(Primitive::Bivalens),
                            value.span,
                            "not result is not bivalens",
                        );
                    }
                    MirUnOp::IsNil | MirUnOp::IsNotNil => {
                        self.require_exact(
                            value.ty,
                            self.primitive(Primitive::Bivalens),
                            value.span,
                            "nil-test result is not bivalens",
                        );
                    }
                    MirUnOp::Neg => {
                        if let Some(operand_ty) = operand_ty {
                            if !self.is_numeric_type(operand_ty) {
                                self.error(value.span, "neg operand is not numeric");
                            }
                        }
                        if !self.is_numeric_type(value.ty) {
                            self.error(value.span, "neg result is not numeric");
                        }
                    }
                    MirUnOp::BitNot => {
                        if let Some(operand_ty) = operand_ty {
                            if !self.is_numeric_integral_type(operand_ty) {
                                self.error(
                                    value.span,
                                    "bitwise not operand is not numeric integral",
                                );
                            }
                        }
                        if !self.is_numeric_integral_type(value.ty) {
                            self.error(value.span, "bitwise not result is not numeric integral");
                        }
                    }
                    MirUnOp::Sqrt
                    | MirUnOp::Relu
                    | MirUnOp::Gelu
                    | MirUnOp::Tanh
                    | MirUnOp::Exp
                    | MirUnOp::Log => {
                        // R-W1: accept tensor operands whose ELEMENT type is float
                        // (AIR lowers AirTensorOp::Gelu/Tanh/Exp/Log/Relu/Sqrt to
                        // MirUnOp on tensor<f32,…>), while still rejecting genuinely
                        // non-float operands — scalar or tensor.
                        if let Some(operand_ty) = operand_ty {
                            if !self.is_float_operand_type(operand_ty) {
                                self.error(value.span, "float unary operand is not fractus");
                            }
                        }
                        if !self.is_float_operand_type(value.ty) {
                            self.error(value.span, "float unary result is not fractus");
                        }
                    }
                }
            }
            MirValueKind::Binary { op, lhs, rhs } => {
                let lhs_ty = self.validate_operand(scope, lhs, value.span);
                let rhs_ty = self.validate_operand(scope, rhs, value.span);
                match op {
                    MirBinOp::Add | MirBinOp::Sub | MirBinOp::Mul | MirBinOp::Div => {
                        if let Some(ty) = lhs_ty {
                            if !self.is_arithmetic_operand_type(*op, ty) {
                                self.error(value.span, "binary arithmetic lhs is not numeric");
                            }
                        }
                        if let Some(ty) = rhs_ty {
                            if !self.is_arithmetic_operand_type(*op, ty) {
                                self.error(value.span, "binary arithmetic rhs is not numeric");
                            }
                        }
                    }
                    MirBinOp::Mod => {
                        if let Some(ty) = lhs_ty {
                            if !self.is_numeric_integral_type(ty) && !self.is_float_type(ty) {
                                self.error(value.span, "mod lhs is not numeric integral");
                            }
                        }
                        if let Some(ty) = rhs_ty {
                            if !self.is_numeric_integral_type(ty) && !self.is_float_type(ty) {
                                self.error(value.span, "mod rhs is not numeric integral");
                            }
                        }
                    }
                    MirBinOp::Eq
                    | MirBinOp::NotEq
                    | MirBinOp::Lt
                    | MirBinOp::Gt
                    | MirBinOp::LtEq
                    | MirBinOp::GtEq => {
                        self.require_exact(
                            value.ty,
                            self.primitive(Primitive::Bivalens),
                            value.span,
                            "comparison result is not bivalens",
                        );
                    }
                    MirBinOp::And | MirBinOp::Or => {
                        if let Some(ty) = lhs_ty {
                            self.require_exact(
                                ty,
                                self.primitive(Primitive::Bivalens),
                                value.span,
                                "logical and/or lhs is not bivalens",
                            );
                        }
                        if let Some(ty) = rhs_ty {
                            self.require_exact(
                                ty,
                                self.primitive(Primitive::Bivalens),
                                value.span,
                                "logical and/or rhs is not bivalens",
                            );
                        }
                        self.require_exact(
                            value.ty,
                            self.primitive(Primitive::Bivalens),
                            value.span,
                            "logical and/or result is not bivalens",
                        );
                    }
                    MirBinOp::Coalesce => {
                        // Coalesce: operands are individually validated; result type
                        // is checked by the caller via require_assignable.
                    }
                    MirBinOp::BitAnd
                    | MirBinOp::BitOr
                    | MirBinOp::BitXor
                    | MirBinOp::Shl
                    | MirBinOp::Shr => {
                        if let Some(ty) = lhs_ty {
                            if !self.is_numeric_integral_type(ty) {
                                self.error(value.span, "bitwise lhs is not numeric integral");
                            }
                        }
                        if let Some(ty) = rhs_ty {
                            if !self.is_numeric_integral_type(ty) {
                                self.error(value.span, "bitwise rhs is not numeric integral");
                            }
                        }
                    }
                }
            }
            MirValueKind::Option(op) => self.validate_option_op(scope, op, value.ty, value.span),
        }
    }

    fn validate_option_op(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        op: &MirOptionOp,
        result_ty: MirType,
        span: Span,
    ) {
        match op {
            MirOptionOp::None => {
                if !matches!(self.type_kind(result_ty), Type::Option(_)) {
                    self.error(span, "option none result is not nullable");
                }
            }
            MirOptionOp::Some(value) => {
                let value_ty = self.validate_operand(scope, value, span);
                match self.type_kind(result_ty) {
                    Type::Option(inner) => {
                        if let Some(value_ty) = value_ty {
                            self.require_assignable(
                                value_ty,
                                MirType::semantic(*inner),
                                span,
                                "option some payload type mismatch",
                            );
                        }
                    }
                    _ => self.error(span, "option some result is not nullable"),
                }
            }
            MirOptionOp::IsNil(value) | MirOptionOp::IsNotNil(value) => {
                self.validate_operand(scope, value, span);
                self.require_exact(
                    result_ty,
                    self.primitive(Primitive::Bivalens),
                    span,
                    "option test result is not bivalens",
                );
            }
            MirOptionOp::Unwrap { value, .. } => {
                if let Some(value_ty) = self.validate_operand(scope, value, span) {
                    if let Some(inner_ty) = self.nullable_inner_ty(value_ty) {
                        self.require_assignable(
                            inner_ty,
                            result_ty,
                            span,
                            "option unwrap result type mismatch",
                        );
                    } else {
                        self.error(span, "option unwrap operand is not nullable");
                    }
                }
            }
            MirOptionOp::Coalesce { value, fallback } => {
                let value_ty = self.validate_operand(scope, value, span);
                let fallback_ty = self.validate_operand(scope, fallback, span);
                if let Some(value_ty) = value_ty {
                    match self.type_kind(value_ty) {
                        Type::Option(inner) => {
                            self.require_assignable(
                                MirType::semantic(*inner),
                                result_ty,
                                span,
                                "option coalesce value type mismatch",
                            );
                        }
                        Type::Primitive(Primitive::Nihil) => {}
                        _ => {
                            self.require_assignable(
                                value_ty,
                                result_ty,
                                span,
                                "option coalesce value type mismatch",
                            );
                        }
                    }
                }
                if let Some(fallback_ty) = fallback_ty {
                    self.require_assignable(
                        fallback_ty,
                        result_ty,
                        span,
                        "option coalesce fallback type mismatch",
                    );
                }
            }
            MirOptionOp::Chain { base, link } => {
                self.validate_operand(scope, base, span);
                self.validate_option_chain_link(scope, link, span);
                if !matches!(self.type_kind(result_ty), Type::Option(_)) {
                    self.error(span, "optional chain result is not nullable");
                }
            }
        }
    }

    fn validate_option_chain_link(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        link: &MirOptionChainLink,
        span: Span,
    ) {
        match link {
            MirOptionChainLink::Field(_) | MirOptionChainLink::VariantField { .. } => {}
            MirOptionChainLink::Index(index) => {
                self.validate_operand(scope, index, span);
            }
            MirOptionChainLink::Call { callee, args } => {
                let sig = self.validate_callee(scope, callee, span);
                let mut generic_bindings = GenericTypeBindings::default();
                self.validate_call_args(scope, sig.as_ref(), args, span, &mut generic_bindings);
            }
        }
    }

    fn validate_runtime_call(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        destination: Option<&MirPlace>,
        call: &MirRuntimeCall,
        span: Span,
    ) {
        self.validate_mir_type(call.return_ty, span);
        for arg in &call.args {
            self.validate_operand(scope, arg, span);
        }
        self.validate_intrinsic(scope, call, span);
        let mut generic_bindings = GenericTypeBindings::default();
        self.validate_optional_destination(
            scope,
            destination,
            Some(call.return_ty),
            span,
            &mut generic_bindings,
        );
    }

    fn validate_intrinsic(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        call: &MirRuntimeCall,
        span: Span,
    ) {
        match &call.intrinsic {
            MirIntrinsic::Diagnostic(_) => {
                self.require_arg_count(call.args.len(), 1, span, "diagnostic runtime call");
                self.require_exact(
                    call.return_ty,
                    self.primitive(Primitive::Vacuum),
                    span,
                    "diagnostic runtime call does not return vacuum",
                );
            }
            MirIntrinsic::Assert => {
                if !(1..=2).contains(&call.args.len()) {
                    self.error(span, "assert runtime call expects 1 or 2 MIR arguments");
                }
                if let Some(cond_ty) = call
                    .args
                    .first()
                    .and_then(|arg| self.validate_operand(scope, arg, span))
                {
                    self.require_exact(
                        cond_ty,
                        self.primitive(Primitive::Bivalens),
                        span,
                        "assert runtime condition is not bivalens",
                    );
                }
                self.require_exact(
                    call.return_ty,
                    self.primitive(Primitive::Vacuum),
                    span,
                    "assert runtime call does not return vacuum",
                );
            }
            MirIntrinsic::FormatString { .. } => {
                self.require_exact(
                    call.return_ty,
                    self.primitive(Primitive::Textus),
                    span,
                    "format_string runtime call does not return textus",
                );
            }
            MirIntrinsic::Convert(conversion) => {
                self.require_arg_count(call.args.len(), 1, span, "conversion runtime call");
                self.validate_mir_type(conversion.source_ty, span);
                self.validate_mir_type(conversion.target_ty, span);
                self.require_assignable(
                    conversion.target_ty,
                    call.return_ty,
                    span,
                    "conversion return type mismatch",
                );
                if let Some(source_ty) = call
                    .args
                    .first()
                    .and_then(|arg| self.validate_operand(scope, arg, span))
                {
                    self.require_assignable(
                        source_ty,
                        conversion.source_ty,
                        span,
                        "conversion source type mismatch",
                    );
                    self.validate_runtime_conversion_route(source_ty, conversion.target_ty, span);
                }
                if let Some(recovery) = &conversion.recovery {
                    if let Some(recovery_ty) = self.validate_operand(scope, recovery, span) {
                        self.require_assignable(
                            recovery_ty,
                            conversion.target_ty,
                            span,
                            "conversion recovery type mismatch",
                        );
                    }
                }
                self.validate_conversion_struct_defaults(scope, conversion, span);
            }
            MirIntrinsic::Collection(op) => self.validate_collection_call(scope, *op, call, span),
            MirIntrinsic::Atomic(op) => self.validate_atomic_call(scope, *op, call, span),
            MirIntrinsic::Panic => {
                self.require_arg_count(call.args.len(), 1, span, "panic runtime call");
                self.require_exact(
                    call.return_ty,
                    self.primitive(Primitive::Numquam),
                    span,
                    "panic runtime call does not return numquam",
                );
            }
            MirIntrinsic::Provider(_) => {}
            MirIntrinsic::SermoOpen | MirIntrinsic::SermoSetOpener => {}
            MirIntrinsic::Sermo(op) => self.validate_sermo_call(scope, op, call, span),
            MirIntrinsic::Cede => {
                self.require_arg_count(call.args.len(), 1, span, "cede runtime call");
                if let Some(arg_ty) = call
                    .args
                    .first()
                    .and_then(|arg| self.validate_operand(scope, arg, span))
                {
                    let awaited = if scope.function.is_generator {
                        arg_ty
                    } else if let Type::Promissum(inner) = self.type_kind(arg_ty) {
                        MirType::semantic(*inner)
                    } else {
                        self.error(span, "cede argument is not promissum");
                        return;
                    };
                    self.require_assignable(
                        awaited,
                        call.return_ty,
                        span,
                        "cede return type mismatch",
                    );
                }
            }
            MirIntrinsic::GpuBuiltin(builtin) => {
                self.require_arg_count(call.args.len(), 0, span, "GPU builtin call");
                if builtin.returns_vector() {
                    if !self.is_u32_vector3(call.return_ty) {
                        self.error(
                            span,
                            "GPU vector builtin call does not return vector<u32, 3>",
                        );
                    }
                } else if !self.is_u32_scalar(call.return_ty) {
                    self.error(span, "GPU builtin call does not return u32");
                }
            }
            MirIntrinsic::CursorStream(_) => {
                if !matches!(self.type_kind(call.return_ty), Type::Array(_)) {
                    self.error(
                        span,
                        "cursor_stream runtime call does not return an array type",
                    );
                }
            }
            MirIntrinsic::ReadLine => {
                self.require_arg_count(call.args.len(), 0, span, "read_line runtime call");
                match self.type_kind(call.return_ty) {
                    Type::Option(inner) => self.require_exact(
                        MirType::semantic(*inner),
                        self.primitive(Primitive::Textus),
                        span,
                        "read_line runtime call does not return textus option",
                    ),
                    _ => self.error(span, "read_line runtime call does not return textus option"),
                }
            }
            MirIntrinsic::Gradient(op) => {
                self.validate_gradient_call(scope, *op, call, span);
            }
        }
    }

    fn validate_gradient_call(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        op: MirGradientOp,
        call: &MirRuntimeCall,
        span: Span,
    ) {
        // Gradient operations take 1 (read/zero) or 5 (create/accumulate) args.
        // The handle is always the first runtime argument.
        let expected = match op {
            MirGradientOp::Read | MirGradientOp::Zero => 1,
            MirGradientOp::Create | MirGradientOp::Accumulate => 5,
        };
        self.require_arg_count(call.args.len(), expected, span, "gradient runtime call");

        // Minimal type checks beyond arity.
        match op {
            MirGradientOp::Read | MirGradientOp::Zero => {
                // Reject obviously-wrong scalar handle types.
                if let Some(handle_ty) = call
                    .args
                    .first()
                    .and_then(|arg| self.validate_operand(scope, arg, span))
                {
                    if self.is_gradient_handle_type_invalid(handle_ty) {
                        self.error(span, "gradient handle type is not an opaque handle");
                    }
                }
            }
            MirGradientOp::Create => {
                // args[0]=shape, args[1]=rank, args[2]=kind — rank/kind must be numerus
                if let Some(rank_ty) = call
                    .args
                    .get(1)
                    .and_then(|arg| self.validate_operand(scope, arg, span))
                {
                    self.require_assignable(
                        rank_ty,
                        self.primitive(Primitive::Numerus),
                        span,
                        "gradient create rank is not numerus",
                    );
                }
                if let Some(kind_ty) = call
                    .args
                    .get(2)
                    .and_then(|arg| self.validate_operand(scope, arg, span))
                {
                    self.require_assignable(
                        kind_ty,
                        self.primitive(Primitive::Numerus),
                        span,
                        "gradient create kind is not numerus",
                    );
                }
            }
            MirGradientOp::Accumulate => {
                // args[0]=handle, args[1]=data, args[2]=shape, args[3]=rank, args[4]=scale
                if let Some(rank_ty) = call
                    .args
                    .get(3)
                    .and_then(|arg| self.validate_operand(scope, arg, span))
                {
                    self.require_assignable(
                        rank_ty,
                        self.primitive(Primitive::Numerus),
                        span,
                        "gradient accumulate rank is not numerus",
                    );
                }
                if let Some(scale_ty) = call
                    .args
                    .get(4)
                    .and_then(|arg| self.validate_operand(scope, arg, span))
                {
                    self.require_assignable(
                        scale_ty,
                        self.primitive(Primitive::Numerus),
                        span,
                        "gradient accumulate scale is not numerus",
                    );
                }
            }
        }
    }

    fn is_gradient_handle_type_invalid(&self, ty: MirType) -> bool {
        matches!(
            self.type_kind(ty),
            Type::Primitive(
                Primitive::Numerus
                    | Primitive::Fractus
                    | Primitive::Bivalens
                    | Primitive::Nihil
                    | Primitive::Vacuum
                    | Primitive::Textus
            )
        )
    }

    fn validate_conversion_struct_defaults(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        conversion: &MirConversion,
        span: Span,
    ) {
        for defaults in &conversion.struct_defaults {
            let Some(fields) = self.context.struct_fields.get(&defaults.def_id).cloned() else {
                self.error(
                    span,
                    "conversion struct defaults are missing field metadata",
                );
                continue;
            };
            for field in &defaults.fields {
                let Some(expected_ty) = fields.get(&field.name).copied() else {
                    self.error(span, "conversion struct default references unknown field");
                    continue;
                };
                if matches!(field.value, MirOperand::Constant(MirConstant::Nil))
                    && self
                        .context
                        .optional_struct_fields
                        .get(&defaults.def_id)
                        .is_some_and(|optional| optional.contains(&field.name))
                {
                    continue;
                }
                if let Some(value_ty) = self.validate_operand(scope, &field.value, span) {
                    self.require_assignable(
                        value_ty,
                        expected_ty,
                        span,
                        "conversion struct default type mismatch",
                    );
                }
            }
        }
    }

    fn validate_sermo_call(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        op: &MirSermoOp,
        call: &MirRuntimeCall,
        span: Span,
    ) {
        let expected_args = match op {
            MirSermoOp::Send => 2,
            MirSermoOp::ViewMeus
            | MirSermoOp::ViewTuus
            | MirSermoOp::Finish
            | MirSermoOp::Receive { .. }
            | MirSermoOp::Cursor { .. }
            | MirSermoOp::DrainCursor => 1,
        };
        self.require_arg_count(call.args.len(), expected_args, span, "sermo runtime call");
        let receiver_ty = call
            .args
            .first()
            .and_then(|receiver| self.validate_operand(scope, receiver, span));
        match op {
            MirSermoOp::ViewMeus | MirSermoOp::ViewTuus => {
                if !call.args.is_empty()
                    && !matches!(self.type_kind(call.return_ty), Type::Applied(_, _))
                {
                    self.error(
                        span,
                        "sermo directional view result is not applied view type",
                    );
                }
            }
            MirSermoOp::Send => {
                self.require_exact(
                    call.return_ty,
                    self.primitive(Primitive::Vacuum),
                    span,
                    "sermo send result is not vacuum",
                );
            }
            MirSermoOp::Finish => {
                if !matches!(self.type_kind(call.return_ty), Type::Enum(_)) {
                    self.error(span, "sermo finish result is not status enum");
                }
            }
            MirSermoOp::Receive { frame, data_field } => {
                if !matches!(
                    self.type_kind(call.return_ty),
                    Type::Option(_) | Type::Union(_)
                ) {
                    self.error(span, "sermo receive result is not optional frame");
                }
                if !self
                    .context
                    .struct_fields
                    .get(frame)
                    .is_some_and(|fields| fields.contains_key(data_field))
                {
                    self.error(span, "sermo receive frame metadata is missing data field");
                }
            }
            MirSermoOp::Cursor { frame, data_field } => {
                if !matches!(self.type_kind(call.return_ty), Type::Cursor(_)) {
                    self.error(span, "sermo cursor result is not cursor");
                }
                if !self
                    .context
                    .struct_fields
                    .get(frame)
                    .is_some_and(|fields| fields.contains_key(data_field))
                {
                    self.error(span, "sermo cursor frame metadata is missing data field");
                }
            }
            MirSermoOp::DrainCursor => {
                let Some(receiver_ty) = receiver_ty else {
                    return;
                };
                let Type::Cursor(element) = self.type_kind(receiver_ty) else {
                    self.error(span, "sermo cursor drain receiver is not cursor");
                    return;
                };
                let Type::Array(result_element) = self.type_kind(call.return_ty) else {
                    self.error(span, "sermo cursor drain result is not lista");
                    return;
                };
                if !self.context.types.assignable(*element, *result_element) {
                    self.error(span, "sermo cursor drain element type mismatch");
                }
            }
        }
    }

    fn validate_collection_call(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        op: MirCollectionOp,
        call: &MirRuntimeCall,
        span: Span,
    ) {
        let expected_args = match op {
            MirCollectionOp::Length
            | MirCollectionOp::IsEmpty
            | MirCollectionOp::First
            | MirCollectionOp::Last
            | MirCollectionOp::RemoveFirst
            | MirCollectionOp::RemoveLast
            | MirCollectionOp::Reverse
            | MirCollectionOp::ReverseInPlace
            | MirCollectionOp::Sort
            | MirCollectionOp::Uppercase
            | MirCollectionOp::Lowercase
            | MirCollectionOp::Trim
            | MirCollectionOp::Absolute
            | MirCollectionOp::Signum
            | MirCollectionOp::Round
            | MirCollectionOp::Floor
            | MirCollectionOp::Ceiling
            | MirCollectionOp::Truncate
            | MirCollectionOp::Sum
            | MirCollectionOp::TensorShape
            | MirCollectionOp::TensorFlatten
            | MirCollectionOp::TensorMaterialize
            | MirCollectionOp::TensorMean
            | MirCollectionOp::TensorNeg
            | MirCollectionOp::TensorGelu
            | MirCollectionOp::TensorRelu
            | MirCollectionOp::TensorSoftmax
            | MirCollectionOp::TensorTranspose
            | MirCollectionOp::SparseNonzeroCount
            | MirCollectionOp::SparseDensify
            | MirCollectionOp::IntervalLength
            | MirCollectionOp::IntervalMaterialize
            | MirCollectionOp::Keys
            | MirCollectionOp::Values => 1,
            MirCollectionOp::Append
            | MirCollectionOp::AppendImmutable
            | MirCollectionOp::Delete
            | MirCollectionOp::Index
            | MirCollectionOp::Contains
            | MirCollectionOp::Union
            | MirCollectionOp::Intersection
            | MirCollectionOp::Difference
            | MirCollectionOp::SymmetricDifference
            | MirCollectionOp::IsSubset
            | MirCollectionOp::IsSuperset
            | MirCollectionOp::Take
            | MirCollectionOp::TakeLast
            | MirCollectionOp::DropFirst
            | MirCollectionOp::StartsWith
            | MirCollectionOp::EndsWith
            | MirCollectionOp::Split
            | MirCollectionOp::Minimum
            | MirCollectionOp::Maximum
            | MirCollectionOp::SumAxis
            | MirCollectionOp::TensorReshape
            | MirCollectionOp::TensorGet
            | MirCollectionOp::TensorFill
            | MirCollectionOp::TensorAdd
            | MirCollectionOp::TensorSub
            | MirCollectionOp::TensorMul
            | MirCollectionOp::TensorMatMul
            | MirCollectionOp::TensorCruxEntropia
            | MirCollectionOp::TensorDiv => 2,
            MirCollectionOp::SparseGet
            | MirCollectionOp::IntervalIntersect
            | MirCollectionOp::IntervalUnion
            | MirCollectionOp::IntervalContains
            | MirCollectionOp::IntervalClamp => 2,
            MirCollectionOp::Put
            | MirCollectionOp::Approximate
            | MirCollectionOp::Slice
            | MirCollectionOp::Replace
            | MirCollectionOp::TensorSet
            | MirCollectionOp::TensorCreate
            | MirCollectionOp::TensorFromFlat
            | MirCollectionOp::TensorSlice => 3,
            MirCollectionOp::SparseSet => 3,
            // TensorLayerNorm: 3-5 args (input + optional gamma/beta + axis + epsilon).
            // Range check is handled below — this arm prevents a non-exhaustive match.
            MirCollectionOp::TensorLayerNorm => 3,
        };
        // TensorLayerNorm accepts 3, 4, or 5 arguments; all other ops are exact.
        let is_layernorm = matches!(op, MirCollectionOp::TensorLayerNorm);
        if is_layernorm {
            if call.args.len() < 3 || call.args.len() > 5 {
                self.error(span, "collection TensorLayerNorm expects 3-5 MIR arguments");
            }
        } else if call.args.len() != expected_args {
            self.error(
                span,
                format!("collection {op:?} expects {expected_args} MIR arguments"),
            );
        }
        let first_arg_ty = call
            .args
            .first()
            .and_then(|arg| self.validate_operand(scope, arg, span));
        match op {
            MirCollectionOp::Length => {
                self.require_exact(
                    call.return_ty,
                    self.primitive(Primitive::Numerus),
                    span,
                    "collection length result is not numerus",
                );
            }
            MirCollectionOp::IsEmpty => {
                self.require_exact(
                    call.return_ty,
                    self.primitive(Primitive::Bivalens),
                    span,
                    "collection is-empty result is not bivalens",
                );
            }
            MirCollectionOp::Contains => {
                self.require_exact(
                    call.return_ty,
                    self.primitive(Primitive::Bivalens),
                    span,
                    "collection contains result is not bivalens",
                );
            }
            MirCollectionOp::Append | MirCollectionOp::AppendImmutable => {
                if let (Some(collection_ty), Some(value)) = (first_arg_ty, call.args.get(1)) {
                    let value_ty = self.validate_operand(scope, value, span);
                    if let (Some(element_ty), Some(value_ty)) =
                        (self.collection_element_ty(collection_ty), value_ty)
                    {
                        self.require_assignable_or_fitting_constant(
                            value,
                            value_ty,
                            element_ty,
                            span,
                            "collection append value type mismatch",
                        );
                    }
                }
            }
            MirCollectionOp::Put => {
                self.require_exact(
                    call.return_ty,
                    self.primitive(Primitive::Vacuum),
                    span,
                    "collection put does not return vacuum",
                );
                let Some(collection_ty) = first_arg_ty else {
                    return;
                };
                let Type::Map(key_ty, value_ty) = self.type_kind(collection_ty) else {
                    self.error(span, "collection put receiver is not a map");
                    return;
                };
                let key_ty = MirType::semantic(*key_ty);
                let value_ty = MirType::semantic(*value_ty);
                if let Some(key) = call.args.get(1) {
                    if let Some(arg_ty) = self.validate_operand(scope, key, span) {
                        self.require_assignable_or_fitting_constant(
                            key,
                            arg_ty,
                            key_ty,
                            span,
                            "collection put key type mismatch",
                        );
                    }
                }
                if let Some(value) = call.args.get(2) {
                    if let Some(arg_ty) = self.validate_operand(scope, value, span) {
                        self.require_assignable_or_fitting_constant(
                            value,
                            arg_ty,
                            value_ty,
                            span,
                            "collection put value type mismatch",
                        );
                    }
                }
            }
            MirCollectionOp::Delete => {
                self.require_exact(
                    call.return_ty,
                    self.primitive(Primitive::Bivalens),
                    span,
                    "collection delete result is not bivalens",
                );
                if let (Some(collection_ty), Some(value)) = (first_arg_ty, call.args.get(1)) {
                    let value_ty = self.validate_operand(scope, value, span);
                    if let Some(value_ty) = value_ty {
                        let expected = match self.type_kind(collection_ty) {
                            Type::Map(key, _) => Some((
                                MirType::semantic(*key),
                                "collection delete key type mismatch",
                            )),
                            Type::Set(inner) => Some((
                                MirType::semantic(*inner),
                                "collection delete element type mismatch",
                            )),
                            _ => None,
                        };
                        if let Some((expected_ty, message)) = expected {
                            self.require_assignable_or_fitting_constant(
                                value,
                                value_ty,
                                expected_ty,
                                span,
                                message,
                            );
                        } else {
                            self.error(span, "collection delete receiver is not map or set");
                        }
                    }
                }
            }
            MirCollectionOp::Index => {
                if let (Some(collection_ty), Some(index)) = (first_arg_ty, call.args.get(1)) {
                    let index_ty = self.validate_operand(scope, index, span);
                    let result_ty =
                        self.project_index(collection_ty, index_ty.map(|ty| (index, ty)), span);
                    if let Some(result_ty) = result_ty {
                        self.require_assignable(
                            result_ty,
                            call.return_ty,
                            span,
                            "collection index result type mismatch",
                        );
                    }
                }
            }
            MirCollectionOp::First
            | MirCollectionOp::Last
            | MirCollectionOp::RemoveFirst
            | MirCollectionOp::RemoveLast => {
                if let Some(collection_ty) = first_arg_ty {
                    let Some(element_ty) = self.collection_element_ty(collection_ty) else {
                        return;
                    };
                    match self.type_kind(call.return_ty) {
                        Type::Option(inner) => {
                            self.require_assignable(
                                MirType::semantic(*inner),
                                element_ty,
                                span,
                                "collection first/last/remove result is not an option of the element type",
                            );
                        }
                        _ => self.require_assignable(
                            call.return_ty,
                            element_ty,
                            span,
                            "collection first/last/remove result type mismatch",
                        ),
                    }
                }
            }
            MirCollectionOp::Reverse | MirCollectionOp::Sort => {
                if let Some(collection_ty) = first_arg_ty {
                    self.require_assignable(
                        call.return_ty,
                        collection_ty,
                        span,
                        "collection reverse/sort result type mismatch",
                    );
                }
            }
            MirCollectionOp::Take | MirCollectionOp::TakeLast | MirCollectionOp::DropFirst => {
                if let Some(collection_ty) = first_arg_ty {
                    let Type::Array(_) = self.type_kind(collection_ty) else {
                        self.error(span, "list copy/view receiver is not a lista");
                        return;
                    };
                    self.require_assignable(
                        call.return_ty,
                        collection_ty,
                        span,
                        "list copy/view result type mismatch",
                    );
                }
                if let Some(count) = call.args.get(1) {
                    if let Some(count_ty) = self.validate_operand(scope, count, span) {
                        self.require_exact(
                            count_ty,
                            self.primitive(Primitive::Numerus),
                            span,
                            "list copy/view count is not numerus",
                        );
                    }
                }
            }
            MirCollectionOp::Sum => {
                let Some(collection_ty) = first_arg_ty else {
                    return;
                };
                let inner = match self.type_kind(collection_ty) {
                    Type::Array(inner) | Type::Tensor(inner, _) => *inner,
                    _ => {
                        self.error(span, "collection sum receiver is not a lista or tensor");
                        return;
                    }
                };
                let element_ty = MirType::semantic(inner);
                if !self.is_numeric_type(element_ty) {
                    self.error(span, "collection sum element is not numeric");
                    return;
                }
                self.require_assignable(
                    element_ty,
                    call.return_ty,
                    span,
                    "collection sum result type mismatch",
                );
            }
            MirCollectionOp::SumAxis => {
                let Some(receiver_ty) = first_arg_ty else {
                    return;
                };
                if !matches!(self.type_kind(receiver_ty), Type::Tensor(_, _)) {
                    self.error(span, "SumAxis receiver is not a tensor");
                    return;
                }
                if !matches!(self.type_kind(call.return_ty), Type::Tensor(_, _)) {
                    self.error(span, "SumAxis result is not a tensor");
                }
            }
            MirCollectionOp::ReverseInPlace => {
                self.require_exact(
                    call.return_ty,
                    self.primitive(Primitive::Vacuum),
                    span,
                    "collection reverse-in-place does not return vacuum",
                );
            }
            MirCollectionOp::Keys | MirCollectionOp::Values => {
                self.validate_map_snapshot_call(first_arg_ty, call.return_ty, span, op);
            }
            MirCollectionOp::Union
            | MirCollectionOp::Intersection
            | MirCollectionOp::Difference
            | MirCollectionOp::SymmetricDifference => {
                if let (Some(collection_ty), Some(other)) = (first_arg_ty, call.args.get(1)) {
                    let other_ty = self.validate_operand(scope, other, span);
                    if !matches!(self.type_kind(collection_ty), Type::Set(_)) {
                        self.error(span, "set algebra receiver is not a set");
                    }
                    if let Some(other_ty) = other_ty {
                        self.require_assignable(
                            other_ty,
                            collection_ty,
                            span,
                            "set algebra argument type mismatch",
                        );
                    }
                    self.require_assignable(
                        collection_ty,
                        call.return_ty,
                        span,
                        "set algebra result type mismatch",
                    );
                }
            }
            MirCollectionOp::IsSubset | MirCollectionOp::IsSuperset => {
                self.require_exact(
                    call.return_ty,
                    self.primitive(Primitive::Bivalens),
                    span,
                    "set predicate result is not bivalens",
                );
                if let (Some(collection_ty), Some(other)) = (first_arg_ty, call.args.get(1)) {
                    let other_ty = self.validate_operand(scope, other, span);
                    if !matches!(self.type_kind(collection_ty), Type::Set(_)) {
                        self.error(span, "set predicate receiver is not a set");
                    }
                    if let Some(other_ty) = other_ty {
                        self.require_assignable(
                            other_ty,
                            collection_ty,
                            span,
                            "set predicate argument type mismatch",
                        );
                    }
                }
            }
            MirCollectionOp::Slice => {
                if let Some(receiver_ty) = first_arg_ty {
                    match self.type_kind(receiver_ty) {
                        Type::Primitive(Primitive::Textus) => {
                            self.require_exact(
                                call.return_ty,
                                self.primitive(Primitive::Textus),
                                span,
                                "text slice result is not textus",
                            );
                        }
                        Type::Array(_) => {
                            self.require_assignable(
                                call.return_ty,
                                receiver_ty,
                                span,
                                "list slice result type mismatch",
                            );
                        }
                        _ => self.error(span, "slice receiver is not textus or lista"),
                    }
                }
                for arg in call.args.iter().skip(1).take(2) {
                    if let Some(arg_ty) = self.validate_operand(scope, arg, span) {
                        self.require_exact(
                            arg_ty,
                            self.primitive(Primitive::Numerus),
                            span,
                            "slice index is not numerus",
                        );
                    }
                }
            }
            MirCollectionOp::StartsWith | MirCollectionOp::EndsWith => {
                self.require_exact(
                    call.return_ty,
                    self.primitive(Primitive::Bivalens),
                    span,
                    "text predicate result is not bivalens",
                );
                if let Some(receiver_ty) = first_arg_ty {
                    self.require_exact(
                        receiver_ty,
                        self.primitive(Primitive::Textus),
                        span,
                        "text predicate receiver is not textus",
                    );
                }
                if let Some(arg) = call.args.get(1) {
                    if let Some(arg_ty) = self.validate_operand(scope, arg, span) {
                        self.require_exact(
                            arg_ty,
                            self.primitive(Primitive::Textus),
                            span,
                            "text predicate argument is not textus",
                        );
                    }
                }
            }
            MirCollectionOp::Uppercase | MirCollectionOp::Lowercase | MirCollectionOp::Trim => {
                self.require_exact(
                    call.return_ty,
                    self.primitive(Primitive::Textus),
                    span,
                    "text transform result is not textus",
                );
                if let Some(receiver_ty) = first_arg_ty {
                    self.require_exact(
                        receiver_ty,
                        self.primitive(Primitive::Textus),
                        span,
                        "text transform receiver is not textus",
                    );
                }
            }
            MirCollectionOp::Split => {
                if let Some(receiver_ty) = first_arg_ty {
                    self.require_exact(
                        receiver_ty,
                        self.primitive(Primitive::Textus),
                        span,
                        "text split receiver is not textus",
                    );
                }
                if let Some(separator) = call.args.get(1) {
                    if let Some(separator_ty) = self.validate_operand(scope, separator, span) {
                        self.require_exact(
                            separator_ty,
                            self.primitive(Primitive::Textus),
                            span,
                            "text split separator is not textus",
                        );
                    }
                }
                match self.type_kind(call.return_ty) {
                    Type::Array(inner)
                        if *inner == self.primitive(Primitive::Textus).semantic_id() => {}
                    _ => self.error(span, "text split result is not lista<textus>"),
                }
            }
            MirCollectionOp::Replace => {
                self.require_exact(
                    call.return_ty,
                    self.primitive(Primitive::Textus),
                    span,
                    "text replace result is not textus",
                );
                if let Some(receiver_ty) = first_arg_ty {
                    self.require_exact(
                        receiver_ty,
                        self.primitive(Primitive::Textus),
                        span,
                        "text replace receiver is not textus",
                    );
                }
                for arg in call.args.iter().skip(1).take(2) {
                    if let Some(arg_ty) = self.validate_operand(scope, arg, span) {
                        self.require_exact(
                            arg_ty,
                            self.primitive(Primitive::Textus),
                            span,
                            "text replace argument is not textus",
                        );
                    }
                }
            }
            MirCollectionOp::Absolute | MirCollectionOp::Signum => {
                let Some(receiver_ty) = first_arg_ty else {
                    return;
                };
                if !matches!(
                    self.type_kind(receiver_ty),
                    Type::Primitive(Primitive::Numerus | Primitive::Fractus)
                ) {
                    self.error(span, "numeric intrinsic receiver is not numerus or fractus");
                    return;
                }
                self.require_exact(
                    call.return_ty,
                    receiver_ty,
                    span,
                    "numeric intrinsic result type mismatch",
                );
            }
            MirCollectionOp::Minimum | MirCollectionOp::Maximum => {
                let Some(receiver_ty) = first_arg_ty else {
                    return;
                };
                if !matches!(
                    self.type_kind(receiver_ty),
                    Type::Primitive(Primitive::Numerus | Primitive::Fractus)
                        | Type::SizedNumeric(Primitive::Numerus | Primitive::Fractus, _)
                ) {
                    self.error(
                        span,
                        "numeric comparison receiver is not numerus or fractus",
                    );
                    return;
                }
                self.require_exact(
                    call.return_ty,
                    receiver_ty,
                    span,
                    "numeric comparison result type mismatch",
                );
                if let Some(arg) = call.args.get(1) {
                    if let Some(arg_ty) = self.validate_operand(scope, arg, span) {
                        self.require_assignable(
                            arg_ty,
                            receiver_ty,
                            span,
                            "numeric comparison argument type mismatch",
                        );
                    }
                }
            }
            MirCollectionOp::Approximate => {
                self.require_exact(
                    call.return_ty,
                    self.primitive(Primitive::Bivalens),
                    span,
                    "approximate comparison result is not bivalens",
                );
                let Some(receiver_ty) = first_arg_ty else {
                    return;
                };
                self.require_exact(
                    receiver_ty,
                    self.primitive(Primitive::Fractus),
                    span,
                    "approximate comparison receiver is not fractus",
                );
                for arg in call.args.iter().skip(1).take(2) {
                    if let Some(arg_ty) = self.validate_operand(scope, arg, span) {
                        self.require_assignable(
                            arg_ty,
                            receiver_ty,
                            span,
                            "approximate comparison argument type mismatch",
                        );
                    }
                }
            }
            MirCollectionOp::Round
            | MirCollectionOp::Floor
            | MirCollectionOp::Ceiling
            | MirCollectionOp::Truncate => {
                self.require_exact(
                    call.return_ty,
                    self.primitive(Primitive::Numerus),
                    span,
                    "numeric rounding result is not numerus",
                );
                if let Some(receiver_ty) = first_arg_ty {
                    self.require_exact(
                        receiver_ty,
                        self.primitive(Primitive::Fractus),
                        span,
                        "numeric rounding receiver is not fractus",
                    );
                }
            }
            MirCollectionOp::TensorShape => {
                let Some(receiver_ty) = first_arg_ty else {
                    return;
                };
                if !matches!(
                    self.type_kind(receiver_ty),
                    Type::Tensor(_, _) | Type::Sparsa(_, _)
                ) {
                    self.error(span, "tensor shape receiver is not tensor or sparsa");
                }
                if !matches!(self.type_kind(call.return_ty), Type::Array(_)) {
                    self.error(span, "tensor collection result is not lista");
                }
            }
            MirCollectionOp::TensorFlatten => {
                let Some(receiver_ty) = first_arg_ty else {
                    return;
                };
                if !matches!(self.type_kind(receiver_ty), Type::Tensor(_, _)) {
                    self.error(span, "tensor flatten receiver is not tensor");
                }
                if !matches!(self.type_kind(call.return_ty), Type::Array(_)) {
                    self.error(span, "tensor collection result is not lista");
                }
            }
            MirCollectionOp::TensorMaterialize => {
                let Some(receiver_ty) = first_arg_ty else {
                    return;
                };
                if !matches!(self.type_kind(receiver_ty), Type::Tensor(_, _)) {
                    self.error(span, "tensor materialize receiver is not tensor");
                }
                if !matches!(self.type_kind(call.return_ty), Type::Tensor(_, _)) {
                    self.error(span, "tensor materialize result is not tensor");
                }
            }
            MirCollectionOp::TensorReshape
            | MirCollectionOp::TensorCreate
            | MirCollectionOp::TensorFromFlat
            | MirCollectionOp::TensorSlice
            | MirCollectionOp::TensorAdd
            | MirCollectionOp::TensorSub
            | MirCollectionOp::TensorMul
            | MirCollectionOp::TensorMatMul
            | MirCollectionOp::TensorDiv
            | MirCollectionOp::TensorNeg
            | MirCollectionOp::TensorGelu
            | MirCollectionOp::TensorRelu
            | MirCollectionOp::TensorSoftmax
            | MirCollectionOp::TensorTranspose => {
                let Some(receiver_ty) = first_arg_ty else {
                    return;
                };
                if !matches!(self.type_kind(receiver_ty), Type::Tensor(_, _)) {
                    self.error(span, "tensor intrinsic receiver is not tensor");
                }
                if !matches!(self.type_kind(call.return_ty), Type::Tensor(_, _)) {
                    self.error(span, "tensor intrinsic result is not tensor");
                }
            }
            MirCollectionOp::TensorGet => {
                let Some(receiver_ty) = first_arg_ty else {
                    return;
                };
                if !matches!(self.type_kind(receiver_ty), Type::Tensor(_, _)) {
                    self.error(span, "tensor get receiver is not tensor");
                }
                if !matches!(self.type_kind(call.return_ty), Type::Option(_)) {
                    self.error(span, "tensor get result is not optional");
                }
            }
            MirCollectionOp::TensorSet => {
                self.require_exact(
                    call.return_ty,
                    self.primitive(Primitive::Vacuum),
                    span,
                    "tensor mutation result is not vacuum",
                );
                if let Some(receiver_ty) = first_arg_ty {
                    if !matches!(self.type_kind(receiver_ty), Type::Tensor(_, _)) {
                        self.error(span, "tensor mutation receiver type mismatch");
                    }
                }
            }
            MirCollectionOp::TensorFill => {
                if let Some(receiver_ty) = first_arg_ty {
                    if !matches!(self.type_kind(receiver_ty), Type::Tensor(_, _)) {
                        self.error(span, "tensor fill receiver is not tensor");
                    }
                }
                if !matches!(self.type_kind(call.return_ty), Type::Tensor(_, _)) {
                    self.error(span, "tensor fill result is not tensor");
                }
            }
            MirCollectionOp::TensorMean => {
                let Some(receiver_ty) = first_arg_ty else {
                    return;
                };
                if !matches!(self.type_kind(receiver_ty), Type::Tensor(_, _)) {
                    self.error(span, "tensor mean receiver is not tensor");
                }
                if !self.is_numeric_type(call.return_ty) {
                    self.error(span, "tensor mean result is not numeric");
                }
            }
            MirCollectionOp::TensorCruxEntropia => {
                let Some(receiver_ty) = first_arg_ty else {
                    return;
                };
                if !matches!(self.type_kind(receiver_ty), Type::Tensor(_, _)) {
                    self.error(span, "tensor crux_entropia receiver is not tensor");
                }
                if !self.is_numeric_type(call.return_ty) {
                    self.error(span, "tensor crux_entropia result is not numeric");
                }
            }
            MirCollectionOp::TensorLayerNorm => {
                // Validate input (first arg) is a tensor and extract shape.
                let Some(input_ty) = first_arg_ty else {
                    return;
                };
                let input_shape = if let Type::Tensor(_, shape) = self.type_kind(input_ty) {
                    *shape
                } else {
                    self.error(span, "tensor layernorm input is not tensor");
                    return;
                };

                // Check input rank is 1 or 2.
                let rank = match self.context.types.get_index(input_shape) {
                    IndexExpr::Literal(_) | IndexExpr::Param(_) => 1,
                    IndexExpr::Tuple(dims) => dims.len(),
                    // Unresolved shape: skip rank checks.
                    IndexExpr::Infer(_) | IndexExpr::Unspecified => return,
                };
                if rank != 1 && rank != 2 {
                    self.error(span, "tensor layernorm input must have rank 1 or 2");
                    return;
                }

                // Validate axis (args[1]) and epsilon (args[2]) — matches method call
                // order `input.laminatio(axis, eps, gamma?, beta?)`.
                // HIGH: guard index computation BEFORE accessing — arity check
                // above only records an error and does not return early.
                let n = call.args.len();
                if n < 3 {
                    return;
                }
                let axis_idx = 1;
                let epsilon_idx = 2;

                if let Some(arg) = call.args.get(axis_idx) {
                    let _ = self.validate_operand(scope, arg, span);
                    match arg {
                        MirOperand::Constant(MirConstant::Int(axis_val)) => {
                            if usize::try_from(*axis_val).map_or(true, |axis| axis >= rank) {
                                self.error(
                                    span,
                                    format!(
                                        "tensor layernorm axis {axis_val} is out of range for rank {rank}"
                                    ),
                                );
                            }
                        }
                        MirOperand::Value(_) | MirOperand::Temp(_) | MirOperand::Place(_) => {
                            // Literals lowered through sized-fractus temps arrive as
                            // MirOperand::Value; the MIR is valid regardless.
                        }
                        MirOperand::Constant(_) => {
                            self.error(
                                span,
                                "tensor layernorm axis must be an int constant or temp",
                            );
                        }
                    }
                }
                if let Some(arg) = call.args.get(epsilon_idx) {
                    let _ = self.validate_operand(scope, arg, span);
                    match arg {
                        MirOperand::Constant(MirConstant::Float(f)) => {
                            if *f <= 0.0 || !f.is_finite() {
                                self.error(
                                    span,
                                    "tensor layernorm epsilon must be positive and finite",
                                );
                            }
                        }
                        MirOperand::Value(_) | MirOperand::Temp(_) | MirOperand::Place(_) => {
                            // Literals lowered through sized-fractus temps are valid.
                        }
                        MirOperand::Constant(_) => {
                            self.error(
                                span,
                                "tensor layernorm epsilon must be a float constant or temp",
                            );
                        }
                    }
                }

                // Validate gamma (optional, args[3] when 4+ args).
                if n >= 4 {
                    if let Some(gamma_ty) = call
                        .args
                        .get(3)
                        .and_then(|arg| self.validate_operand(scope, arg, span))
                    {
                        if !matches!(self.type_kind(gamma_ty), Type::Tensor(_, _)) {
                            self.error(span, "tensor layernorm gamma is not tensor");
                        }
                    }
                }

                // Validate beta (optional, args[4] when 5 args).
                if n >= 5 {
                    if let Some(beta_ty) = call
                        .args
                        .get(4)
                        .and_then(|arg| self.validate_operand(scope, arg, span))
                    {
                        if !matches!(self.type_kind(beta_ty), Type::Tensor(_, _)) {
                            self.error(span, "tensor layernorm beta is not tensor");
                        }
                    }
                }

                // Validate return type is tensor.
                if !matches!(self.type_kind(call.return_ty), Type::Tensor(_, _)) {
                    self.error(span, "tensor layernorm result is not tensor");
                }
            }
            MirCollectionOp::SparseNonzeroCount => {
                let Some(receiver_ty) = first_arg_ty else {
                    return;
                };
                if !matches!(self.type_kind(receiver_ty), Type::Sparsa(_, _)) {
                    self.error(span, "sparse nonzero-count receiver is not sparsa");
                }
                self.require_exact(
                    call.return_ty,
                    self.primitive(Primitive::Numerus),
                    span,
                    "sparse nonzero-count result is not numerus",
                );
            }
            MirCollectionOp::SparseDensify => {
                let Some(receiver_ty) = first_arg_ty else {
                    return;
                };
                let (
                    Type::Sparsa(source_element, source_shape),
                    Type::Tensor(target_element, target_shape),
                ) = (self.type_kind(receiver_ty), self.type_kind(call.return_ty))
                else {
                    self.error(
                        span,
                        "sparse densify requires sparsa receiver and tensor result",
                    );
                    return;
                };
                let elements_match = self
                    .context
                    .types
                    .assignable(*source_element, *target_element)
                    && self
                        .context
                        .types
                        .assignable(*target_element, *source_element);
                let shapes_match = self.context.types.get_index(*source_shape)
                    == self.context.types.get_index(*target_shape);
                if !elements_match || !shapes_match {
                    self.error(span, "sparse densify element or shape mismatch");
                }
            }
            MirCollectionOp::SparseGet => {
                let Some(receiver_ty) = first_arg_ty else {
                    return;
                };
                let Type::Sparsa(element, _) = self.type_kind(receiver_ty) else {
                    self.error(span, "sparse get receiver is not sparsa");
                    return;
                };
                self.require_assignable(
                    call.return_ty,
                    MirType::semantic(*element),
                    span,
                    "sparse get result type mismatch",
                );
            }
            MirCollectionOp::SparseSet => {
                self.require_exact(
                    call.return_ty,
                    self.primitive(Primitive::Vacuum),
                    span,
                    "sparse set result is not vacuum",
                );
                let Some(receiver_ty) = first_arg_ty else {
                    return;
                };
                let Type::Sparsa(element, _) = self.type_kind(receiver_ty) else {
                    self.error(span, "sparse set receiver is not sparsa");
                    return;
                };
                let element = *element;
                if let Some(value) = call
                    .args
                    .get(2)
                    .and_then(|value| self.validate_operand(scope, value, span))
                {
                    self.require_assignable(
                        value,
                        MirType::semantic(element),
                        span,
                        "sparse set value type mismatch",
                    );
                }
            }
            MirCollectionOp::IntervalLength => {
                if !first_arg_ty
                    .is_some_and(|ty| matches!(self.type_kind(ty), Type::Intervallum(_)))
                {
                    self.error(span, "interval length receiver is not intervallum");
                }
                self.require_exact(
                    call.return_ty,
                    self.primitive(Primitive::Numerus),
                    span,
                    "interval length result is not numerus",
                );
            }
            MirCollectionOp::IntervalContains => {
                let Some(receiver_ty) = first_arg_ty else {
                    return;
                };
                let Type::Intervallum(element) = self.type_kind(receiver_ty) else {
                    self.error(span, "interval contains receiver is not intervallum");
                    return;
                };
                let element = *element;
                if let Some(value) = call
                    .args
                    .get(1)
                    .and_then(|arg| self.validate_operand(scope, arg, span))
                {
                    self.require_assignable(
                        value,
                        MirType::semantic(element),
                        span,
                        "interval contains value mismatch",
                    );
                }
                self.require_exact(
                    call.return_ty,
                    self.primitive(Primitive::Bivalens),
                    span,
                    "interval contains result is not bivalens",
                );
            }
            MirCollectionOp::IntervalIntersect | MirCollectionOp::IntervalUnion => {
                let Some(receiver_ty) = first_arg_ty else {
                    return;
                };
                if !matches!(self.type_kind(receiver_ty), Type::Intervallum(_)) {
                    self.error(span, "interval algebra receiver is not intervallum");
                }
                if let Some(other_ty) = call
                    .args
                    .get(1)
                    .and_then(|arg| self.validate_operand(scope, arg, span))
                {
                    self.require_assignable(
                        other_ty,
                        receiver_ty,
                        span,
                        "interval algebra argument mismatch",
                    );
                }
                if !matches!(
                    self.type_kind(call.return_ty),
                    Type::Option(_) | Type::Union(_)
                ) {
                    self.error(span, "interval algebra result is not nullable interval");
                }
            }
            MirCollectionOp::IntervalClamp => {
                let Some(target_ty) = call
                    .args
                    .get(1)
                    .and_then(|arg| self.validate_operand(scope, arg, span))
                else {
                    return;
                };
                if !matches!(self.type_kind(target_ty), Type::Intervallum(_)) {
                    self.error(span, "interval clamp target is not intervallum");
                }
            }
            MirCollectionOp::IntervalMaterialize => {
                if !first_arg_ty
                    .is_some_and(|ty| matches!(self.type_kind(ty), Type::Intervallum(_)))
                {
                    self.error(span, "interval materialize receiver is not intervallum");
                }
                if !matches!(
                    self.type_kind(call.return_ty),
                    Type::Array(_) | Type::Tensor(_, _)
                ) {
                    self.error(span, "interval materialize result is not lista or tensor");
                }
            }
        }
    }

    fn validate_map_snapshot_call(
        &mut self,
        collection_ty: Option<MirType>,
        result_ty: MirType,
        span: Span,
        op: MirCollectionOp,
    ) {
        let (receiver_msg, array_msg, element_msg) = match op {
            MirCollectionOp::Keys => (
                "collection keys receiver is not a map",
                "collection keys result is not an array",
                "collection keys result is not an array of the map key type",
            ),
            MirCollectionOp::Values => (
                "collection values receiver is not a map",
                "collection values result is not an array",
                "collection values result is not an array of the map value type",
            ),
            _ => return,
        };
        let Some(collection_ty) = collection_ty else {
            return;
        };
        let element_ty = match self.type_kind(collection_ty) {
            Type::Map(key, _) if matches!(op, MirCollectionOp::Keys) => *key,
            Type::Map(_, value) if matches!(op, MirCollectionOp::Values) => *value,
            _ => {
                self.error(span, receiver_msg);
                return;
            }
        };
        match self.type_kind(result_ty) {
            Type::Array(inner) => self.require_assignable(
                MirType::semantic(*inner),
                MirType::semantic(element_ty),
                span,
                element_msg,
            ),
            _ => self.error(span, array_msg),
        }
    }

    fn validate_atomic_call(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        op: MirAtomicOp,
        call: &MirRuntimeCall,
        span: Span,
    ) {
        let expected_args = match op {
            MirAtomicOp::Load => 1,
            MirAtomicOp::Store | MirAtomicOp::Exchange => 2,
            MirAtomicOp::CompareExchange => 3,
        };
        self.require_arg_count(call.args.len(), expected_args, span, "atomic runtime call");

        let receiver_ty = call
            .args
            .first()
            .and_then(|receiver| self.validate_operand(scope, receiver, span));
        let Some(receiver_ty) = receiver_ty else {
            return;
        };
        let Type::Atomic(element_id) = self.type_kind(receiver_ty) else {
            self.error(span, "atomic runtime receiver is not atomic");
            return;
        };
        let element_ty = MirType::semantic(*element_id);
        match self.context.layouts.layout_for_type(receiver_ty) {
            Ok(layout) if matches!(layout.kind, MirLayoutKind::Atomic(_)) => {}
            Ok(_) => self.error(span, "atomic runtime receiver layout is not atomic"),
            Err(_) => self.error(span, "atomic runtime receiver has no atomic layout"),
        }

        match op {
            MirAtomicOp::Load => {
                self.require_assignable(
                    element_ty,
                    call.return_ty,
                    span,
                    "atomic load result type mismatch",
                );
            }
            MirAtomicOp::Store => {
                self.require_exact(
                    call.return_ty,
                    self.primitive(Primitive::Vacuum),
                    span,
                    "atomic store does not return vacuum",
                );
                self.validate_atomic_value_arg(
                    scope,
                    call,
                    1,
                    element_ty,
                    span,
                    "atomic store value type mismatch",
                );
            }
            MirAtomicOp::Exchange => {
                self.require_assignable(
                    element_ty,
                    call.return_ty,
                    span,
                    "atomic exchange result type mismatch",
                );
                self.validate_atomic_value_arg(
                    scope,
                    call,
                    1,
                    element_ty,
                    span,
                    "atomic exchange value type mismatch",
                );
            }
            MirAtomicOp::CompareExchange => {
                self.require_exact(
                    call.return_ty,
                    self.primitive(Primitive::Bivalens),
                    span,
                    "atomic compare_exchange result is not bivalens",
                );
                self.validate_atomic_value_arg(
                    scope,
                    call,
                    1,
                    element_ty,
                    span,
                    "atomic compare_exchange expected type mismatch",
                );
                self.validate_atomic_value_arg(
                    scope,
                    call,
                    2,
                    element_ty,
                    span,
                    "atomic compare_exchange replacement type mismatch",
                );
            }
        }
    }

    fn validate_atomic_value_arg(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        call: &MirRuntimeCall,
        index: usize,
        element_ty: MirType,
        span: Span,
        message: &'static str,
    ) {
        if let Some(arg) = call.args.get(index) {
            if let Some(arg_ty) = self.validate_operand(scope, arg, span) {
                self.require_assignable(arg_ty, element_ty, span, message);
            }
        }
    }

    fn validate_runtime_conversion_route(
        &mut self,
        source_ty: MirType,
        target_ty: MirType,
        span: Span,
    ) {
        let source = source_ty.semantic_id();
        let target = target_ty.semantic_id();
        if validate_lista_to_tensor_source(source, target, self.context.types).is_ok() {
            self.error(
                span,
                "eligible lista-to-tensor conversio must lower to tensor_from_flat, not runtime convert",
            );
        }
    }

    fn require_arg_count(
        &mut self,
        actual: usize,
        expected: usize,
        span: Span,
        label: &'static str,
    ) {
        if actual != expected {
            self.error(span, format!("{label} expects {expected} MIR arguments"));
        }
    }

    fn collection_element_ty(&mut self, collection_ty: MirType) -> Option<MirType> {
        match self.type_kind(collection_ty) {
            Type::Array(inner) | Type::Set(inner) => Some(MirType::semantic(*inner)),
            _ => None,
        }
    }

    fn is_numeric_type(&self, ty: MirType) -> bool {
        matches!(
            self.type_kind(ty),
            Type::Primitive(Primitive::Numerus | Primitive::Fractus)
                | Type::SizedNumeric(Primitive::Numerus | Primitive::Fractus, _)
                | Type::ModularWord(_)
        )
    }

    fn is_numeric_integral_type(&self, ty: MirType) -> bool {
        matches!(
            self.type_kind(ty),
            Type::Primitive(Primitive::Numerus)
                | Type::SizedNumeric(Primitive::Numerus, _)
                | Type::ModularWord(_)
        )
    }

    fn is_float_type(&self, ty: MirType) -> bool {
        matches!(
            self.type_kind(ty),
            Type::Primitive(Primitive::Fractus) | Type::SizedNumeric(Primitive::Fractus, _)
        )
    }

    /// Float check for the float-unary ops (Gelu/Tanh/Sqrt/Relu/Exp/Log).
    ///
    /// Accepts scalar float types and tensor types whose ELEMENT type is float;
    /// scalar behavior is identical to [`Self::is_float_type`]. Non-float
    /// element types (scalar or tensor) are still rejected, preserving the
    /// F-035/F-036 hardening intent.
    fn is_float_operand_type(&self, ty: MirType) -> bool {
        match self.type_kind(ty) {
            Type::Primitive(Primitive::Fractus) | Type::SizedNumeric(Primitive::Fractus, _) => true,
            Type::Tensor(inner, _) => self.is_float_type(MirType::semantic(*inner)),
            _ => false,
        }
    }

    fn is_arithmetic_operand_type(&self, op: MirBinOp, ty: MirType) -> bool {
        self.is_numeric_type(ty) || (matches!(op, MirBinOp::Add) && self.is_text_type(ty))
    }

    fn is_text_type(&self, ty: MirType) -> bool {
        matches!(self.type_kind(ty), Type::Primitive(Primitive::Textus))
    }

    fn validate_aggregate(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        aggregate: &MirAggregate,
        span: Span,
    ) {
        self.validate_mir_type(aggregate.ty, span);
        match (&aggregate.kind, &aggregate.fields) {
            (MirAggregateKind::Tuple, MirAggregateFields::Ordered(items)) => {
                let Type::Tuple(elements) = self.type_kind(aggregate.ty) else {
                    self.error(span, "tuple aggregate type is not tuple");
                    return;
                };
                let elem_tys = elements.clone();
                if items.len() != elem_tys.len() {
                    self.error(
                        span,
                        format!(
                            "tuple aggregate element count {} does not match tuple arity {}",
                            items.len(),
                            elem_tys.len()
                        ),
                    );
                }
                for (i, item) in items.iter().enumerate() {
                    match item {
                        MirAggregateItem::Operand(value) => {
                            let value_ty = self.validate_operand(scope, value, span);
                            if let (Some(value_ty), Some(expected_ty)) = (value_ty, elem_tys.get(i))
                            {
                                self.require_assignable(
                                    value_ty,
                                    MirType::semantic(*expected_ty),
                                    span,
                                    "tuple aggregate element type mismatch",
                                );
                            }
                        }
                        MirAggregateItem::Spread(value) => {
                            self.validate_operand(scope, value, span);
                            self.error(span, "tuple aggregate spread is unsupported");
                        }
                    }
                }
            }
            (MirAggregateKind::Tensor, MirAggregateFields::Ordered(items)) => {
                if !items.is_empty() {
                    self.error(
                        span,
                        "tensor aggregate construction supports only vacua in MIR",
                    );
                }
                if !matches!(self.type_kind(aggregate.ty), Type::Tensor(_, _)) {
                    self.error(span, "tensor aggregate type is not tensor");
                }
            }
            (MirAggregateKind::SparseTensor, MirAggregateFields::Ordered(items)) => {
                if !items.is_empty() {
                    self.error(
                        span,
                        "sparse tensor aggregate construction supports only vacua in MIR",
                    );
                }
                if !matches!(self.type_kind(aggregate.ty), Type::Sparsa(_, _)) {
                    self.error(span, "sparse tensor aggregate type is not sparsa");
                }
            }
            (MirAggregateKind::Interval { .. }, MirAggregateFields::Ordered(items)) => {
                let Type::Intervallum(element) = self.type_kind(aggregate.ty) else {
                    self.error(span, "interval aggregate type is not intervallum");
                    return;
                };
                let element = MirType::semantic(*element);
                if items.len() != 2 {
                    self.error(span, "interval aggregate requires two bounds");
                }
                for item in items {
                    let MirAggregateItem::Operand(bound) = item else {
                        self.error(span, "interval aggregate bound cannot spread");
                        continue;
                    };
                    if let Some(bound_ty) = self.validate_operand(scope, bound, span) {
                        self.require_assignable(
                            bound_ty,
                            element,
                            span,
                            "interval aggregate bound type mismatch",
                        );
                    }
                }
            }
            (MirAggregateKind::Vector, MirAggregateFields::Ordered(items)) => {
                let (element_ty, width) =
                    if let Type::Vector(inner, width) = self.type_kind(aggregate.ty) {
                        (Some(MirType::semantic(*inner)), Some(*width))
                    } else {
                        self.error(span, "vector aggregate type is not vector");
                        (None, None)
                    };
                let expected_len = width.and_then(|width| {
                    if let IndexExpr::Literal(value) = self.context.types.get_index(width) {
                        if let Ok(value) = usize::try_from(*value) {
                            Some(value)
                        } else {
                            self.error(span, "vector aggregate width does not fit this target");
                            None
                        }
                    } else {
                        self.error(span, "vector aggregate width is not a literal");
                        None
                    }
                });
                if let Some(expected_len) = expected_len {
                    if items.len() != expected_len {
                        self.error(span, "vector aggregate element count does not match width");
                    }
                }
                for item in items {
                    match item {
                        MirAggregateItem::Operand(value) => {
                            let value_ty = self.validate_operand(scope, value, span);
                            if let (Some(value_ty), Some(element_ty)) = (value_ty, element_ty) {
                                self.require_assignable_or_fitting_constant(
                                    value,
                                    value_ty,
                                    element_ty,
                                    span,
                                    "vector aggregate element type mismatch",
                                );
                            }
                        }
                        MirAggregateItem::Spread(value) => {
                            self.validate_operand(scope, value, span);
                            self.error(span, "vector aggregate spread is unsupported");
                        }
                    }
                }
            }
            (MirAggregateKind::Matrix, MirAggregateFields::Ordered(items)) => {
                let (element_ty, expected_len) =
                    if let Type::Matrix(inner, shape) = self.type_kind(aggregate.ty).clone() {
                        let expected_len = if let Some((rows, columns)) =
                            crate::matrix_shape::literal_matrix_shape(shape, self.context.types)
                        {
                            Some(usize::from(rows) * usize::from(columns))
                        } else {
                            self.error(span, "matrix aggregate shape is not literal");
                            None
                        };
                        (Some(MirType::semantic(inner)), expected_len)
                    } else {
                        self.error(span, "matrix aggregate type is not matrix");
                        (None, None)
                    };
                if let Some(expected_len) = expected_len {
                    if items.len() != expected_len {
                        self.error(span, "matrix aggregate element count does not match shape");
                    }
                }
                for item in items {
                    match item {
                        MirAggregateItem::Operand(value) => {
                            let value_ty = self.validate_operand(scope, value, span);
                            if let (Some(value_ty), Some(element_ty)) = (value_ty, element_ty) {
                                self.require_assignable_or_fitting_constant(
                                    value,
                                    value_ty,
                                    element_ty,
                                    span,
                                    "matrix aggregate element type mismatch",
                                );
                            }
                        }
                        MirAggregateItem::Spread(value) => {
                            self.validate_operand(scope, value, span);
                            self.error(span, "matrix aggregate spread is unsupported");
                        }
                    }
                }
            }
            (
                MirAggregateKind::Array | MirAggregateKind::Set,
                MirAggregateFields::Ordered(items),
            ) => {
                let element_ty = match self.type_kind(aggregate.ty) {
                    Type::Array(inner) | Type::Set(inner) => Some(MirType::semantic(*inner)),
                    Type::Primitive(Primitive::Valor)
                        if matches!(aggregate.kind, MirAggregateKind::Array) =>
                    {
                        None
                    }
                    _ => {
                        self.error(
                            span,
                            "ordered collection aggregate type is not array or set",
                        );
                        None
                    }
                };
                for item in items {
                    match item {
                        MirAggregateItem::Operand(value) => {
                            let value_ty = self.validate_operand(scope, value, span);
                            if let (Some(value_ty), Some(element_ty)) = (value_ty, element_ty) {
                                self.require_assignable_or_fitting_constant(
                                    value,
                                    value_ty,
                                    element_ty,
                                    span,
                                    "aggregate element type mismatch",
                                );
                            }
                        }
                        MirAggregateItem::Spread(value) => {
                            let value_ty = self.validate_operand(scope, value, span);
                            if let Some(value_ty) = value_ty {
                                self.require_assignable(
                                    value_ty,
                                    aggregate.ty,
                                    span,
                                    "aggregate spread type mismatch",
                                );
                            }
                        }
                    }
                }
            }
            (MirAggregateKind::Map, MirAggregateFields::Keyed(items)) => {
                let (key_ty, value_ty) = match self.type_kind(aggregate.ty) {
                    Type::Map(key, value) => (
                        Some(MirType::semantic(*key)),
                        Some(MirType::semantic(*value)),
                    ),
                    Type::Primitive(Primitive::Valor | Primitive::Json) => (None, None),
                    _ => {
                        self.error(span, "map aggregate type is not map");
                        (None, None)
                    }
                };
                for item in items {
                    let actual_key_ty = self.validate_operand(scope, &item.key, span);
                    let actual_value_ty = self.validate_operand(scope, &item.value, span);
                    if let (Some(actual_key_ty), Some(key_ty)) = (actual_key_ty, key_ty) {
                        self.require_assignable_or_fitting_constant(
                            &item.key,
                            actual_key_ty,
                            key_ty,
                            span,
                            "map aggregate key type mismatch",
                        );
                    }
                    if let (Some(actual_value_ty), Some(value_ty)) = (actual_value_ty, value_ty) {
                        self.require_assignable_or_fitting_constant(
                            &item.value,
                            actual_value_ty,
                            value_ty,
                            span,
                            "map aggregate value type mismatch",
                        );
                    }
                }
            }
            (MirAggregateKind::Record, MirAggregateFields::Named(items)) => {
                let Type::Record(fields) = self.type_kind(aggregate.ty).clone() else {
                    self.error(span, "record aggregate type is not record");
                    return;
                };
                self.validate_record_aggregate_fields(scope, &fields, items, span);
            }
            (
                MirAggregateKind::ClosureEnvironment(environment_id),
                MirAggregateFields::Named(items),
            ) => {
                self.validate_closure_environment_aggregate(scope, *environment_id, items, span);
            }
            (MirAggregateKind::Struct(def_id), MirAggregateFields::Named(items)) => {
                match self.type_kind(aggregate.ty) {
                    Type::Struct(type_def) if type_def == def_id => {}
                    Type::Struct(_) => {
                        self.error(span, "struct aggregate DefId does not match aggregate type");
                    }
                    _ => self.error(span, "struct aggregate type is not struct"),
                }
                self.validate_named_aggregate_fields(scope, *def_id, items, true, span);
            }
            (MirAggregateKind::EnumVariant(def_id), MirAggregateFields::Named(items)) => {
                if let Some(enum_def) = self.enum_def_from_type(aggregate.ty) {
                    self.validate_variant_parent(*def_id, enum_def, span);
                } else {
                    self.error(span, "enum variant aggregate type is not enum");
                }
                self.validate_named_aggregate_fields(scope, *def_id, items, false, span);
            }
            (MirAggregateKind::EnumVariant(def_id), MirAggregateFields::Ordered(items)) => {
                if let Some(enum_def) = self.enum_def_from_type(aggregate.ty) {
                    self.validate_variant_parent(*def_id, enum_def, span);
                } else {
                    self.error(span, "enum variant aggregate type is not enum");
                }
                for item in items {
                    match item {
                        MirAggregateItem::Operand(value) | MirAggregateItem::Spread(value) => {
                            self.validate_operand(scope, value, span);
                        }
                    }
                }
            }
            _ => self.error(
                span,
                "aggregate payload shape does not match aggregate kind",
            ),
        }
    }

    fn validate_record_aggregate_fields(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        fields: &FxHashMap<Symbol, radix_types::TypeId>,
        items: &[MirNamedOperand],
        span: Span,
    ) {
        let mut seen = FxHashSet::default();
        for item in items {
            let value_ty = self.validate_operand(scope, &item.value, span);
            if !seen.insert(item.name) {
                self.error(span, "record aggregate field is duplicated");
            }
            let Some(expected_ty) = fields.get(&item.name).copied() else {
                self.error(span, "record aggregate references unknown field");
                continue;
            };
            if let Some(value_ty) = value_ty {
                self.require_assignable(
                    value_ty,
                    MirType::semantic(expected_ty),
                    span,
                    "record aggregate field type mismatch",
                );
            }
        }
        for field in fields.keys() {
            if !seen.contains(field) {
                self.error(span, "record aggregate is missing required field");
            }
        }
    }

    fn validate_closure_environment_aggregate(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        environment_id: MirClosureEnvironmentId,
        items: &[MirNamedOperand],
        span: Span,
    ) {
        let Some(environment) = self
            .context
            .closure_environments
            .get(&environment_id)
            .cloned()
        else {
            self.error(span, "closure environment aggregate is missing metadata");
            return;
        };
        let mut expected = environment
            .captures
            .iter()
            .map(|capture| (capture.name, (capture.source, capture.ty)))
            .collect::<FxHashMap<_, _>>();
        let mut seen = FxHashSet::default();
        for item in items {
            let value_ty = self.validate_operand(scope, &item.value, span);
            if !seen.insert(item.name) {
                self.error(span, "closure environment field is duplicated");
            }
            let Some((_, expected_ty)) = expected.remove(&item.name) else {
                self.error(span, "closure environment references unknown capture");
                continue;
            };
            if let Some(value_ty) = value_ty {
                self.require_assignable(
                    value_ty,
                    expected_ty,
                    span,
                    "closure environment capture type mismatch",
                );
            }
        }
        if !expected.is_empty() {
            self.error(span, "closure environment is missing required capture");
        }
    }

    fn validate_closure_value(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        closure: &MirClosureValue,
        value_ty: MirType,
        span: Span,
    ) {
        let Some(environment) = self
            .context
            .closure_environments
            .get(&closure.environment_id)
            .cloned()
        else {
            self.error(span, "closure value is missing environment metadata");
            return;
        };
        if environment.function != closure.function {
            self.error(
                span,
                "closure value function does not match environment metadata",
            );
        }
        if environment.value_ty != value_ty {
            self.error(
                span,
                "closure value type does not match environment metadata",
            );
        }
        if !scope.functions_by_id.contains_key(&closure.function) {
            self.error(
                span,
                format!("closure function f{} does not exist", closure.function.0),
            );
        }
        self.validate_operand(scope, &closure.environment, span);
        if !matches!(self.type_kind(value_ty), Type::Func(_)) {
            self.error(span, "closure value result type is not a function");
        }
    }

    fn validate_named_aggregate_fields(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        def_id: DefId,
        items: &[MirNamedOperand],
        is_struct: bool,
        span: Span,
    ) {
        let fields = if is_struct {
            self.context.struct_fields.get(&def_id)
        } else {
            self.context.variant_fields.get(&def_id)
        };

        let mut seen = FxHashSet::default();
        for item in items {
            let value_ty = self.validate_operand(scope, &item.value, span);
            if !seen.insert(item.name) {
                self.error(span, "named aggregate field is duplicated");
            }

            let Some(fields) = fields else {
                continue;
            };
            let Some(expected_ty) = fields.get(&item.name).copied() else {
                self.error(span, "named aggregate references unknown field");
                continue;
            };
            if is_struct
                && matches!(item.value, MirOperand::Constant(MirConstant::Nil))
                && self
                    .context
                    .optional_struct_fields
                    .get(&def_id)
                    .is_some_and(|optional| optional.contains(&item.name))
            {
                continue;
            }
            if let Some(value_ty) = value_ty {
                self.require_assignable(
                    value_ty,
                    expected_ty,
                    span,
                    "named aggregate field type mismatch",
                );
            }
        }

        let Some(fields) = fields else {
            self.error(span, "named aggregate is missing field metadata");
            return;
        };
        for field in fields.keys() {
            if !seen.contains(field) {
                if is_struct
                    && self
                        .context
                        .optional_struct_fields
                        .get(&def_id)
                        .is_some_and(|optional| optional.contains(field))
                {
                    continue;
                }
                self.error(span, "named aggregate is missing required field");
            }
        }
    }

    fn validate_optional_destination(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        destination: Option<&MirPlace>,
        expected_ty: Option<MirType>,
        span: Span,
        generic_bindings: &mut GenericTypeBindings,
    ) {
        match (destination, expected_ty) {
            (Some(destination), Some(expected_ty)) => {
                if let Some(destination_ty) = self.validate_place(scope, destination, span) {
                    self.bind_generic_type_witness(
                        expected_ty,
                        destination_ty,
                        generic_bindings,
                        span,
                        "destination type mismatch",
                    );
                    if let Some(expected_ty) =
                        self.instantiate_generic_type(expected_ty, generic_bindings, span)
                    {
                        self.require_assignable(
                            expected_ty,
                            destination_ty,
                            span,
                            "destination type mismatch",
                        );
                    }
                }
            }
            (Some(destination), None) => {
                self.validate_place(scope, destination, span);
            }
            (None, Some(expected_ty)) => {
                if !self.is_vacuum(expected_ty) && !self.is_numquam(expected_ty) {
                    self.error(span, "non-vacuum result has no destination");
                }
            }
            (None, None) => {}
        }
    }

    fn validate_callee(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        callee: &MirCallee,
        span: Span,
    ) -> Option<MirFunctionSignature> {
        match callee {
            MirCallee::Function(id) => {
                let Some(function) = scope.functions_by_id.get(id).copied() else {
                    self.error(span, format!("callee function f{} does not exist", id.0));
                    return None;
                };
                Some(signature_from_function(function))
            }
            MirCallee::Definition { source: def_id, .. } => self
                .context
                .functions
                .get(def_id)
                .cloned()
                .or_else(|| Self::signature_from_program_def(scope, *def_id)),
            MirCallee::Closure(closure) => self.validate_closure_callee(scope, closure, span),
            MirCallee::Value(value) => {
                let operand_ty = self.validate_operand(scope, value, span);
                let sig = operand_ty
                    .and_then(|ty| self.signature_from_semantic_callable_type(ty.semantic_id()));
                if operand_ty.is_some() && sig.is_none() {
                    self.error(span, "value callee is not callable");
                }
                sig
            }
        }
    }

    fn validate_closure_callee(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        closure: &MirClosureCallee,
        span: Span,
    ) -> Option<MirFunctionSignature> {
        let Some(environment) = self
            .context
            .closure_environments
            .get(&closure.environment_id)
            .cloned()
        else {
            self.error(span, "closure callee is missing environment metadata");
            self.validate_operand(scope, &closure.environment, span);
            return None;
        };
        if environment.function != closure.function {
            self.error(
                span,
                "closure callee function does not match environment metadata",
            );
        }
        self.validate_operand(scope, &closure.environment, span);
        let Some(function) = scope.functions_by_id.get(&closure.function).copied() else {
            self.error(
                span,
                format!(
                    "closure callee function f{} does not exist",
                    closure.function.0
                ),
            );
            return None;
        };
        Some(signature_from_function(function))
    }

    fn signature_from_program_def(
        scope: &FunctionScope<'_, '_>,
        def_id: DefId,
    ) -> Option<MirFunctionSignature> {
        scope
            .functions_by_id
            .values()
            .find(|function| function.source == Some(def_id))
            .map(|function| signature_from_function(function))
    }

    fn signature_from_semantic_callable_type(&self, ty: TypeId) -> Option<MirFunctionSignature> {
        match self.context.types.get(ty) {
            Type::Func(sig) => Some(signature_from_func_sig(sig)),
            Type::Alias(_, inner) => self.signature_from_semantic_callable_type(*inner),
            _ => None,
        }
    }

    fn validate_call_args(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        sig: Option<&MirFunctionSignature>,
        args: &[MirOperand],
        span: Span,
        generic_bindings: &mut GenericTypeBindings,
    ) {
        let Some(sig) = sig else {
            for arg in args {
                self.validate_operand(scope, arg, span);
            }
            return;
        };

        let required = sig.params.iter().filter(|param| !param.optional).count();
        if args.len() < required || args.len() > sig.params.len() {
            self.error(
                span,
                format!(
                    "call argument count mismatch: expected {}..={}, got {}",
                    required,
                    sig.params.len(),
                    args.len()
                ),
            );
        }

        for (index, arg) in args.iter().enumerate() {
            let arg_ty = self.validate_operand(scope, arg, span);
            let Some(expected) = sig.params.get(index) else {
                continue;
            };
            if let Some(arg_ty) = arg_ty {
                if expected.optional && matches!(arg, MirOperand::Constant(MirConstant::Nil)) {
                    continue;
                }
                self.bind_generic_type_witness(
                    expected.ty,
                    arg_ty,
                    generic_bindings,
                    span,
                    "call argument type mismatch",
                );
                if let Some(expected_ty) =
                    self.instantiate_generic_type(expected.ty, generic_bindings, span)
                {
                    self.require_assignable(
                        arg_ty,
                        expected_ty,
                        span,
                        "call argument type mismatch",
                    );
                }
            }
        }
    }

    fn bind_generic_type_witness(
        &mut self,
        expected: MirType,
        actual: MirType,
        generic_bindings: &mut GenericTypeBindings,
        span: Span,
        message: &'static str,
    ) {
        if crate::bind_generic_type_witness(self.context.types, expected, actual, generic_bindings)
        {
            return;
        }
        self.error(span, message);
    }

    fn instantiate_generic_type(
        &mut self,
        ty: MirType,
        generic_bindings: &GenericTypeBindings,
        span: Span,
    ) -> Option<MirType> {
        match crate::instantiate_existing_generic_type(
            self.context.types,
            ty,
            generic_bindings,
            span,
        ) {
            Ok(ty) => Some(ty),
            Err(error) => {
                self.error(error.span, error.message);
                None
            }
        }
    }

    fn validate_operand(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        operand: &MirOperand,
        span: Span,
    ) -> Option<MirType> {
        match operand {
            MirOperand::Place(place) => self.validate_place(scope, place, span),
            MirOperand::Temp(id) => {
                if let Some(ty) = scope.temps.get(id).copied() {
                    Some(ty)
                } else {
                    self.error(span, format!("temp %{} does not exist", id.0));
                    None
                }
            }
            MirOperand::Value(id) => {
                if let Some(ty) = scope.values.get(id).copied() {
                    Some(ty)
                } else {
                    self.error(
                        span,
                        format!("value v{} is not defined earlier in MIR", id.0),
                    );
                    None
                }
            }
            MirOperand::Constant(value) => Some(self.constant_ty(value)),
        }
    }

    fn validate_place(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        place: &MirPlace,
        span: Span,
    ) -> Option<MirType> {
        let mut ty = match place.base {
            MirPlaceBase::Local(id) => {
                if let Some(ty) = scope.locals.get(&id).copied() {
                    ty
                } else {
                    self.error(span, format!("local _{} does not exist", id.0));
                    return None;
                }
            }
            MirPlaceBase::Temp(id) => {
                if let Some(ty) = scope.temps.get(&id).copied() {
                    ty
                } else {
                    self.error(span, format!("temp %{} does not exist", id.0));
                    return None;
                }
            }
        };

        for projection in &place.projections {
            let base_ty = ty;
            ty = match projection {
                MirProjection::Field(field) => self.project_field(ty, *field, span)?,
                MirProjection::VariantField { variant, field } => {
                    self.project_variant_field(ty, *variant, *field, span)?
                }
                MirProjection::ClosureCapture {
                    environment,
                    source,
                } => self.project_closure_capture(*environment, *source, span)?,
                MirProjection::Index(index) => {
                    let index_ty = self.validate_operand(scope, index, span);
                    self.project_index(ty, index_ty.map(|ty| (index, ty)), span)?
                }
                MirProjection::VectorLane(index) => self.project_vector_lane(ty, *index, span)?,
                MirProjection::MatrixCell { row, column } => {
                    self.project_matrix_cell(ty, *row, *column, span)?
                }
            };
            if !matches!(projection, MirProjection::ClosureCapture { .. }) {
                self.validate_projection_representation(base_ty, ty, projection, span);
            }
        }

        Some(ty)
    }

    fn project_closure_capture(
        &mut self,
        environment_id: MirClosureEnvironmentId,
        source: DefId,
        span: Span,
    ) -> Option<MirType> {
        self.context
            .closure_environments
            .get(&environment_id)
            .and_then(|environment| {
                environment
                    .captures
                    .iter()
                    .find(|capture| capture.source == source)
                    .map(|capture| capture.ty)
            })
            .or_else(|| {
                self.error(span, "closure capture projection is missing metadata");
                None
            })
    }

    fn project_field(&mut self, base_ty: MirType, field: Symbol, span: Span) -> Option<MirType> {
        if self.is_handler_error_nuntius_projection(base_ty, field) {
            return Some(self.primitive(Primitive::Textus));
        }
        if matches!(self.type_kind(base_ty), Type::Primitive(Primitive::Ignotum)) {
            return Some(self.primitive(Primitive::Ignotum));
        }
        match self.type_kind(base_ty).clone() {
            Type::Struct(def_id) => self
                .context
                .struct_fields
                .get(&def_id)
                .and_then(|fields| fields.get(&field))
                .copied()
                .or_else(|| {
                    self.error(span, "struct field projection is missing field metadata");
                    None
                }),
            Type::Record(fields) => {
                fields
                    .get(&field)
                    .copied()
                    .map(MirType::semantic)
                    .or_else(|| {
                        self.error(span, "record field projection references unknown field");
                        None
                    })
            }
            Type::Applied(base, args) => {
                let Type::Struct(def_id) = self.context.types.get(base) else {
                    self.error(span, "applied field projection base is not a struct");
                    return None;
                };
                let Some(raw_field_ty) = self
                    .context
                    .struct_fields
                    .get(def_id)
                    .and_then(|fields| fields.get(&field))
                    .copied()
                else {
                    self.error(
                        span,
                        "applied struct field projection is missing field metadata",
                    );
                    return None;
                };
                let Some(params) = self.context.struct_type_params.get(def_id) else {
                    self.error(
                        span,
                        "applied struct field projection is missing type parameters",
                    );
                    return None;
                };
                if params.len() != args.len() {
                    self.error(
                        span,
                        "applied struct field projection type argument count mismatch",
                    );
                    return None;
                }
                let bindings = params
                    .iter()
                    .copied()
                    .zip(args.into_iter().map(MirType::semantic))
                    .collect::<MirGenericBindings>();
                instantiate_existing_generic_type(self.context.types, raw_field_ty, &bindings, span)
                    .map_err(|error| self.error(error.span, error.message))
                    .ok()
            }
            Type::Map(key, value) => {
                self.require_assignable(
                    self.primitive(Primitive::Textus),
                    MirType::semantic(key),
                    span,
                    "map field projection key type mismatch",
                );
                Some(MirType::semantic(value))
            }
            _ => {
                self.error(span, "field projection base is not a struct or record");
                None
            }
        }
    }

    fn project_vector_lane(&mut self, base_ty: MirType, index: u8, span: Span) -> Option<MirType> {
        let Type::Vector(element, width) = self.type_kind(base_ty).clone() else {
            self.error(span, "vector lane projection base is not vector");
            return None;
        };
        match self.context.types.get_index(width) {
            IndexExpr::Literal(width) if u64::from(index) < *width => {
                Some(MirType::semantic(element))
            }
            IndexExpr::Literal(_) => {
                self.error(span, "vector lane projection index is out of bounds");
                None
            }
            _ => {
                self.error(span, "vector lane projection width is not a literal");
                None
            }
        }
    }

    fn project_matrix_cell(
        &mut self,
        base_ty: MirType,
        row: u8,
        column: u8,
        span: Span,
    ) -> Option<MirType> {
        let Type::Matrix(element, shape) = self.type_kind(base_ty).clone() else {
            self.error(span, "matrix cell projection base is not matrix");
            return None;
        };
        match crate::matrix_shape::literal_matrix_shape(shape, self.context.types) {
            Some((rows, columns)) if row < rows && column < columns => {
                Some(MirType::semantic(element))
            }
            Some(_) => {
                self.error(span, "matrix cell projection index is out of bounds");
                None
            }
            None => {
                self.error(span, "matrix cell projection shape is not literal");
                None
            }
        }
    }

    fn is_handler_error_nuntius_projection(&self, base_ty: MirType, field: Symbol) -> bool {
        let Some(interner) = self.context.interner else {
            return false;
        };
        if interner.resolve(field) != crate::HANDLER_MESSAGE_FIELD {
            return false;
        }
        matches!(
            self.type_kind(base_ty),
            Type::Primitive(Primitive::Textus | Primitive::Ignotum)
        )
    }

    fn project_variant_field(
        &mut self,
        base_ty: MirType,
        variant: DefId,
        field: Symbol,
        span: Span,
    ) -> Option<MirType> {
        if let Some(enum_def) = self.enum_def_from_type(base_ty) {
            self.validate_variant_parent(variant, enum_def, span);
        } else {
            self.error(span, "variant field projection base is not enum");
        }
        self.context
            .variant_fields
            .get(&variant)
            .and_then(|fields| fields.get(&field))
            .copied()
            .or_else(|| {
                self.error(span, "variant field projection is missing field metadata");
                None
            })
    }

    fn validate_variant_parent(&mut self, variant: DefId, enum_def: DefId, span: Span) {
        match self.context.variant_parents.get(&variant).copied() {
            Some(parent) if parent == enum_def => {}
            Some(_) => self.error(span, "enum variant does not belong to enum type"),
            None => self.error(span, "enum variant is missing parent metadata"),
        }
    }

    fn project_index(
        &mut self,
        base_ty: MirType,
        index: Option<(&MirOperand, MirType)>,
        span: Span,
    ) -> Option<MirType> {
        match self.type_kind(base_ty).clone() {
            Type::Array(inner) => {
                if let Some((_, index_ty)) = index {
                    self.require_assignable(
                        index_ty,
                        self.primitive(Primitive::Numerus),
                        span,
                        "array index is not numerus",
                    );
                }
                Some(MirType::semantic(inner))
            }
            Type::Tensor(inner, _) => {
                if let Some((_, index_ty)) = index {
                    self.require_device_view_index(index_ty, span);
                }
                Some(MirType::semantic(inner))
            }
            Type::Map(key, value) => {
                if let Some((index_op, index_ty)) = index {
                    self.require_assignable_or_fitting_constant(
                        index_op,
                        index_ty,
                        MirType::semantic(key),
                        span,
                        "map index key type mismatch",
                    );
                }
                Some(MirType::semantic(value))
            }
            Type::Primitive(Primitive::Textus) => {
                if let Some((_, index_ty)) = index {
                    self.require_assignable(
                        index_ty,
                        self.primitive(Primitive::Numerus),
                        span,
                        "textus index is not numerus",
                    );
                }
                Some(self.primitive(Primitive::Textus))
            }
            Type::Primitive(Primitive::Octeti) => {
                if let Some((_, index_ty)) = index {
                    self.require_assignable(
                        index_ty,
                        self.primitive(Primitive::Numerus),
                        span,
                        "octeti index is not numerus",
                    );
                }
                match crate::ty::index_projection_result_ty(base_ty, self.context.types) {
                    Ok(ty) => Some(ty),
                    Err(error) => {
                        self.error(span, error.message);
                        None
                    }
                }
            }
            Type::Tuple(elements) => match index {
                Some((MirOperand::Constant(MirConstant::Int(n)), _)) => {
                    // Negative or non-fitting indices are out of bounds; usize::MAX is the
                    // out-of-range sentinel (matches the old `as` wrap for negatives on 64-bit).
                    let idx = usize::try_from(*n).unwrap_or(usize::MAX);
                    if idx >= elements.len() {
                        self.error(
                            span,
                            format!("tuple index {n} out of bounds for arity {}", elements.len()),
                        );
                        None
                    } else {
                        Some(MirType::semantic(elements[idx]))
                    }
                }
                Some((MirOperand::Constant(MirConstant::UInt(n)), _)) => {
                    // Non-fitting indices are out of bounds; usize::MAX is the out-of-range sentinel.
                    let idx = usize::try_from(*n).unwrap_or(usize::MAX);
                    if idx >= elements.len() {
                        self.error(
                            span,
                            format!("tuple index {n} out of bounds for arity {}", elements.len()),
                        );
                        None
                    } else {
                        Some(MirType::semantic(elements[idx]))
                    }
                }
                Some(_) => {
                    self.error(span, "tuple subscript requires compile-time constant index");
                    None
                }
                None => None,
            },
            _ => {
                self.error(span, "index projection base is not indexable");
                None
            }
        }
    }

    fn validate_projection_representation(
        &mut self,
        base_ty: MirType,
        result_ty: MirType,
        projection: &MirProjection,
        span: Span,
    ) {
        let Ok(base_layout) = self.context.layouts.layout_for_type(base_ty) else {
            return;
        };
        match base_layout.kind {
            MirLayoutKind::DeviceView(view) => {
                if !matches!(projection, MirProjection::Index(_)) {
                    self.error(span, "device view projection must be an index");
                }
                if !view.row_major || view.offset_elements != 0 {
                    self.error(
                        span,
                        "device view index projection requires materialized row-major storage",
                    );
                }
                if view.rank != 1 {
                    self.error(span, "device view index projection requires rank-1 storage");
                }
                let Ok(result_layout) = self.context.layouts.layout_for_type(result_ty) else {
                    self.error(span, "device view projection result has no layout");
                    return;
                };
                if result_layout.kind != MirLayoutKind::Scalar(view.element) {
                    self.error(span, "device view projection element layout mismatch");
                }
            }
            MirLayoutKind::Aggregate(layout)
                if layout.carrier == MirAggregateCarrier::PhysicalValue
                    && layout.family != MirAggregateFamily::Tuple =>
            {
                if matches!(projection, MirProjection::Index(_)) {
                    self.error(
                        span,
                        "physical aggregate representation has no index projection contract",
                    );
                }
            }
            MirLayoutKind::Aggregate(layout)
                if layout.family == MirAggregateFamily::TensorRuntime =>
            {
                if matches!(projection, MirProjection::Index(_)) {
                    self.error(
                        span,
                        "tensor runtime aggregate has no device-view projection contract",
                    );
                }
            }
            MirLayoutKind::DeviceVector(vector) => {
                if !matches!(
                    projection,
                    MirProjection::Field(_) | MirProjection::VectorLane(_)
                ) {
                    self.error(
                        span,
                        "device vector projection must be a swizzle field or lane",
                    );
                }
                let Ok(result_layout) = self.context.layouts.layout_for_type(result_ty) else {
                    self.error(span, "device vector projection result has no layout");
                    return;
                };
                if let MirLayoutKind::DeviceVector(result_vector) = result_layout.kind {
                    if result_vector.element != vector.element {
                        self.error(span, "device vector swizzle element layout mismatch");
                    }
                } else if let MirLayoutKind::Scalar(result_scalar) = result_layout.kind {
                    if result_scalar != vector.element {
                        self.error(span, "device vector lane element layout mismatch");
                    }
                } else {
                    self.error(
                        span,
                        "device vector projection result must be a vector or element scalar",
                    );
                }
            }
            MirLayoutKind::DeviceMatrix(matrix) => {
                if !matches!(projection, MirProjection::MatrixCell { .. }) {
                    self.error(span, "device matrix projection must be a matrix cell");
                }
                let Ok(result_layout) = self.context.layouts.layout_for_type(result_ty) else {
                    self.error(span, "device matrix projection result has no layout");
                    return;
                };
                if result_layout.kind != MirLayoutKind::Scalar(matrix.element) {
                    self.error(span, "device matrix cell element layout mismatch");
                }
            }
            MirLayoutKind::PackedNumeric(packed) => {
                self.error(span, packed.unsupported_execution_issue());
            }
            MirLayoutKind::Aggregate(_)
            | MirLayoutKind::Scalar(_)
            | MirLayoutKind::Atomic(_)
            | MirLayoutKind::OpaqueHandle
            | MirLayoutKind::Void
            | MirLayoutKind::Never => {}
        }
    }

    fn require_device_view_index(&mut self, index_ty: MirType, span: Span) {
        match self.type_kind(index_ty) {
            Type::Primitive(Primitive::Numerus) | Type::SizedNumeric(Primitive::Numerus, _) => {}
            _ => self.error(span, "device view index is not integer"),
        }
    }

    fn require_block(&mut self, scope: &FunctionScope<'_, '_>, block: MirBlockId, span: Span) {
        if !scope.blocks.contains(&block) {
            self.error(span, format!("block bb{} does not exist", block.0));
        }
    }

    fn validate_mir_type(&mut self, ty: MirType, span: Span) {
        let mut seen = FxHashSet::default();
        self.validate_type_id(ty.semantic_id(), span, &mut seen);
    }

    fn validate_type_id(
        &mut self,
        ty: radix_types::TypeId,
        span: Span,
        seen: &mut FxHashSet<radix_types::TypeId>,
    ) {
        if !seen.insert(ty) {
            return;
        }

        match self.context.types.get(ty).clone() {
            Type::Infer(_) => self.error(span, "MIR type is unresolved inference variable"),
            Type::Error => self.error(span, "MIR type is semantic error recovery type"),
            Type::Array(inner)
            | Type::Set(inner)
            | Type::Promissum(inner)
            | Type::Cursor(inner)
            | Type::Tensor(inner, _)
            | Type::Vector(inner, _)
            | Type::Matrix(inner, _)
            | Type::Sparsa(inner, _)
            | Type::Atomic(inner)
            | Type::Option(inner)
            | Type::Ref(_, inner)
            | Type::Alias(_, inner) => self.validate_type_id(inner, span, seen),
            Type::PromissumFailable(success, alternate) | Type::AsyncCursor(success, alternate) => {
                self.validate_type_id(success, span, seen);
                self.validate_type_id(alternate, span, seen);
            }
            Type::Map(key, value) => {
                self.validate_type_id(key, span, seen);
                self.validate_type_id(value, span, seen);
            }
            Type::Record(fields) => {
                for field_ty in fields.values() {
                    self.validate_type_id(*field_ty, span, seen);
                }
            }
            Type::Func(sig) => {
                for param in &sig.params {
                    self.validate_type_id(param.ty, span, seen);
                }
                self.validate_type_id(sig.ret, span, seen);
                if let Some(error_ty) = sig.err {
                    self.validate_type_id(error_ty, span, seen);
                }
            }
            Type::Applied(base, args) => {
                self.validate_type_id(base, span, seen);
                for arg in args {
                    self.validate_type_id(arg, span, seen);
                }
            }
            Type::Union(members) => {
                for member in members {
                    self.validate_type_id(member, span, seen);
                }
            }
            Type::Primitive(_)
            | Type::SizedNumeric(_, _)
            | Type::ModularWord(_)
            | Type::SizedInstans(_)
            | Type::Intervallum(_)
            | Type::Tuple(_)
            | Type::Struct(_)
            | Type::Enum(_)
            | Type::Interface(_)
            | Type::Param(_) => {}
        }
    }

    fn require_assignable(
        &mut self,
        from: MirType,
        to: MirType,
        span: Span,
        message: &'static str,
    ) {
        if !self
            .context
            .types
            .assignable(from.semantic_id(), to.semantic_id())
        {
            self.error(span, message);
        }
    }

    /// Like [`Self::require_assignable`], but accepts an integer literal
    /// constant that fits the expected integer or modular type. MIR constants
    /// carry plain `numerus` / `numerus<u64>` types, while source literals in
    /// a typed collection were already range-checked by semantic analysis
    /// against the declared element/key type — re-rejecting them here would
    /// break `lista<modulus<u32>> ← [4294967295, 3]`-style programs.
    fn require_assignable_or_fitting_constant(
        &mut self,
        operand: &MirOperand,
        found: MirType,
        expected: MirType,
        span: Span,
        message: &'static str,
    ) {
        if self
            .context
            .types
            .assignable(found.semantic_id(), expected.semantic_id())
        {
            return;
        }
        if self.integer_constant_fits(operand, expected) {
            return;
        }
        self.error(span, message);
    }

    fn integer_constant_fits(&self, operand: &MirOperand, expected: MirType) -> bool {
        let MirOperand::Constant(constant) = operand else {
            return false;
        };
        let (signed, unsigned) = match constant {
            MirConstant::Int(v) => (Some(*v), None),
            MirConstant::UInt(v) => (None, Some(*v)),
            _ => return false,
        };
        match self.type_kind(expected) {
            Type::ModularWord(width) => {
                let max = width.unsigned_max().unwrap_or(u64::MAX);
                match (signed, unsigned) {
                    (Some(v), _) => u64::try_from(v).is_ok_and(|vu| vu <= max),
                    (_, Some(v)) => v <= max,
                    (None, None) => false,
                }
            }
            Type::SizedNumeric(Primitive::Numerus, width) => {
                let (lo, hi) = numerus_width_range(*width);
                match (signed, unsigned) {
                    (Some(v), _) => (lo..=hi).contains(&i128::from(v)),
                    (_, Some(v)) => (lo..=hi).contains(&i128::from(v)),
                    (None, None) => false,
                }
            }
            Type::Primitive(Primitive::Numerus) => {
                signed.is_some() || unsigned.is_some_and(|v| i64::try_from(v).is_ok())
            }
            _ => false,
        }
    }

    fn require_exact(
        &mut self,
        found: MirType,
        expected: MirType,
        span: Span,
        message: &'static str,
    ) {
        if found.semantic_id() != expected.semantic_id() {
            self.error(span, message);
        }
    }

    fn constant_ty(&self, value: &MirConstant) -> MirType {
        match value {
            MirConstant::Int(_) => self.primitive(Primitive::Numerus),
            // Unsigned constants carry the sized `numerus<u64>` type so typed
            // boundaries (tabula keys, copia members) accept them exactly.
            MirConstant::UInt(_) => self
                .context
                .types
                .find_sized_numeric(Primitive::Numerus, NumericWidth::U64)
                .map_or_else(|| self.primitive(Primitive::Numerus), MirType::semantic),
            MirConstant::Float(_) => self.primitive(Primitive::Fractus),
            MirConstant::String(_) => self.primitive(Primitive::Textus),
            MirConstant::Ascii(_) => self.primitive(Primitive::Ascii),
            MirConstant::Bool(_) => self.primitive(Primitive::Bivalens),
            MirConstant::Nil => self.primitive(Primitive::Nihil),
            MirConstant::Unit => self.primitive(Primitive::Vacuum),
            MirConstant::Octeti(_) => self.primitive(Primitive::Octeti),
            MirConstant::Regex { .. } => self.primitive(Primitive::Regex),
            MirConstant::Function(_) => self.primitive(Primitive::Ignotum),
        }
    }

    fn primitive(&self, primitive: Primitive) -> MirType {
        MirType::semantic(self.context.types.primitive(primitive))
    }

    fn is_u32_scalar(&self, ty: MirType) -> bool {
        self.context
            .layouts
            .layout_for_type(ty)
            .is_ok_and(|layout| layout.kind == MirLayoutKind::Scalar(MirScalarLayout::U32))
    }

    fn is_u32_vector3(&self, ty: MirType) -> bool {
        let Ok(layout) = self.context.layouts.layout_for_type(ty) else {
            return false;
        };
        let MirLayoutKind::DeviceVector(vector) = layout.kind else {
            return false;
        };
        vector.element == MirScalarLayout::U32 && vector.width == MirKernelBuiltin::VECTOR_WIDTH
    }

    fn is_vacuum(&self, ty: MirType) -> bool {
        ty.semantic_id() == self.context.types.primitive(Primitive::Vacuum)
    }

    fn is_unit_return(&self, ty: MirType) -> bool {
        matches!(
            self.context.types.get(ty.semantic_id()),
            Type::Primitive(Primitive::Vacuum | Primitive::Nihil)
        )
    }

    fn is_numquam(&self, ty: MirType) -> bool {
        ty.semantic_id() == self.context.types.primitive(Primitive::Numquam)
    }

    fn type_kind(&self, ty: MirType) -> &Type {
        self.context.types.get(ty.semantic_id())
    }

    fn enum_def_from_type(&self, ty: MirType) -> Option<DefId> {
        match self.type_kind(ty) {
            Type::Enum(def_id) => Some(*def_id),
            Type::Applied(base, _) => self.enum_def_from_type(MirType::semantic(*base)),
            Type::Alias(_, inner) => self.enum_def_from_type(MirType::semantic(*inner)),
            Type::Ref(_, inner) => self.enum_def_from_type(MirType::semantic(*inner)),
            _ => None,
        }
    }

    fn nullable_inner_ty(&self, ty: MirType) -> Option<MirType> {
        self.context
            .types
            .nullable_inner(ty.semantic_id())
            .map(MirType::semantic)
    }

    fn report_duplicate_local_ids(&mut self, pairs: &[(MirLocalId, Span)]) {
        for (id, span) in pairs {
            self.error(*span, format!("duplicate MIR local id _{}", id.0));
        }
    }

    fn error(&mut self, span: Span, message: impl Into<String>) {
        self.errors.push(MirValidationError::new(span, message));
    }
}

/// Inclusive `[min, max]` of a `numerus<W>` width as i128 so every width —
/// including `u64` — range-checks without host overflow.
fn numerus_width_range(width: NumericWidth) -> (i128, i128) {
    match width {
        NumericWidth::I8 => (i128::from(i8::MIN), i128::from(i8::MAX)),
        NumericWidth::I16 => (i128::from(i16::MIN), i128::from(i16::MAX)),
        NumericWidth::I32 => (i128::from(i32::MIN), i128::from(i32::MAX)),
        NumericWidth::I64 => (i128::from(i64::MIN), i128::from(i64::MAX)),
        NumericWidth::U8 => (0, i128::from(u8::MAX)),
        NumericWidth::U16 => (0, i128::from(u16::MAX)),
        NumericWidth::U32 => (0, i128::from(u32::MAX)),
        NumericWidth::U64 => (0, i128::from(u64::MAX)),
        NumericWidth::F16 | NumericWidth::F32 | NumericWidth::F64 => {
            (i128::from(i64::MIN), i128::from(i64::MAX))
        }
    }
}

#[cfg(test)]
#[path = "validate_test.rs"]
mod tests;
