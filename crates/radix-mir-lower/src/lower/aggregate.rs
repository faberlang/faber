//! Aggregate construction lowering for typed HIR expressions.
//!
//! This module owns the bridge from high-level aggregate syntax to MIR
//! `Construct` statements. It handles tuple, array, set, map, struct, and enum
//! variant construction only when semantic lowering has already provided the
//! target type and a supported source shape. Missing type information is treated
//! as an upstream semantic bug and unsupported object forms fail closed with a
//! `MirError`.
//!
//! INVARIANTS
//! ==========
//! - Every aggregate construction writes through a fresh MIR temp.
//! - Struct and map object literals preserve their distinct named/keyed field
//!   contracts.
//! - Enum variant payloads use named fields only when the HIR context proves
//!   the variant field names and arity match the lowered operands.

#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for lowering
use super::*;

/// Policy for filling struct fields omitted from an explicit literal/object list.
pub(super) enum StructFieldFill {
    /// Apply static `init` expressions and `sponte` type-zero fills for missing fields.
    Construction,
    /// Zero every declared field regardless of `init`/`sponte` metadata.
    AllZero,
}

impl FunctionBuilder<'_> {
    pub(super) fn lower_interval_expr(
        &mut self,
        start: &HirExpression,
        end: &HirExpression,
        step: Option<&HirExpression>,
        kind: HirRangeKind,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        // A step is a positive stride over a window (`xs[a‥b per n]`): it rides
        // as an optional third operand, checked positive where the window is
        // read.
        let mut fields = vec![
            MirAggregateItem::Operand(self.lower_expr_value(start)?),
            MirAggregateItem::Operand(self.lower_expr_value(end)?),
        ];
        if let Some(step) = step {
            fields.push(MirAggregateItem::Operand(self.lower_expr_value(step)?));
        }
        let ty = self.expr_ty(expr)?;
        Some(self.construct_temp(
            MirAggregateKind::Interval { inclusive: matches!(kind, HirRangeKind::Inclusive) },
            MirAggregateFields::Ordered(fields),
            ty,
            expr.span,
        ))
    }

    pub(super) fn lower_interval_clamp(
        &mut self,
        source: &HirExpression,
        target: &HirExpression,
        recovery: Option<&HirExpression>,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        if recovery.is_some() {
            self.errors.push(MirError::unsupported(
                expr.span,
                "interval clamp recovery before interval MIR lowering",
            ));
            return None;
        }
        let source = self.lower_expr_value(source)?;
        let target = self.lower_expr_value(target)?;
        let ty = self.expr_ty(expr)?;
        Some(self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::IntervalClamp),
            vec![source, target],
            ty,
            expr.span,
        ))
    }

    /// Lower `vacua` into the empty aggregate form selected by its typed
    /// collection context.
    pub(super) fn lower_vacua(&mut self, expr: &HirExpression) -> Option<MirOperand> {
        let Some(type_id) = expr.ty else {
            self.errors
                .push(MirError::missing_type(expr.span, "vacua expression"));
            return None;
        };
        let ty = MirType::semantic(type_id);
        match self.normalized_type(type_id) {
            Type::Array(_)
            | Type::BoundedArray { .. }
            | Type::RangedArray { .. }
            | Type::Queue(_)
            | Type::BoundedQueue { .. }
            | Type::Stack(_)
            | Type::BoundedStack { .. } => Some(self.construct_temp(
                MirAggregateKind::Array,
                MirAggregateFields::Ordered(Vec::new()),
                ty,
                expr.span,
            )),
            Type::Map(_, _) | Type::Ledger(_, _) => {
                Some(self.construct_temp(MirAggregateKind::Map, MirAggregateFields::Keyed(Vec::new()), ty, expr.span))
            }
            Type::Set(_) => {
                Some(self.construct_temp(MirAggregateKind::Set, MirAggregateFields::Ordered(Vec::new()), ty, expr.span))
            }
            Type::Tensor(_, _) => Some(self.construct_temp(
                MirAggregateKind::Tensor,
                MirAggregateFields::Ordered(Vec::new()),
                ty,
                expr.span,
            )),
            Type::Sparsa(_, _) => Some(self.construct_temp(
                MirAggregateKind::SparseTensor,
                MirAggregateFields::Ordered(Vec::new()),
                ty,
                expr.span,
            )),
            _ => {
                self.errors.push(MirError::unsupported(
                    expr.span,
                    "vacua without collection type before aggregate MIR lowering",
                ));
                None
            }
        }
    }

    /// Lower a tuple literal into an ordered MIR aggregate temp.
    ///
    /// A numeric literal element takes its positional member's sized carrier.
    /// Semantic analysis types an integer literal `numerus`, so a bare `Int`
    /// constant reads as plain `numerus` at the validator and a `u8`/`i16`
    /// member is rejected; the typed temp is the same fix `lower_call_arg_value`
    /// applies to call arguments (d11-6 ruling 12a).
    pub(super) fn lower_tuple(&mut self, items: &[HirExpression], expr: &HirExpression) -> Option<MirOperand> {
        let ty = self.expr_ty(expr)?;
        let member_tys: Vec<TypeId> = match self.normalized_type(ty.semantic_id()) {
            Type::Tuple(elements) => elements.clone(),
            _ => Vec::new(),
        };
        let mut fields = Vec::with_capacity(items.len());
        for (index, item) in items.iter().enumerate() {
            let value = self.lower_expr_value(item)?;
            let value = match member_tys.get(index).copied() {
                Some(member_ty) if is_numeric_literal(item) && self.takes_sized_member_type(member_ty) => {
                    self.assign_temp(MirValueKind::Operand(value), MirType::semantic(member_ty), item.span)
                }
                _ => value,
            };
            fields.push(MirAggregateItem::Operand(value));
        }
        Some(self.construct_temp(MirAggregateKind::Tuple, MirAggregateFields::Ordered(fields), ty, expr.span))
    }

    /// Lower an array-like literal into the requested aggregate kind.
    ///
    /// The same ordered-field path is used for arrays and set casts from array
    /// syntax; the caller decides which MIR aggregate kind the target type
    /// requires.
    pub(super) fn lower_array(
        &mut self,
        elements: &[HirArrayElement],
        expr: &HirExpression,
        kind: MirAggregateKind,
    ) -> Option<MirOperand> {
        let fields = self.lower_array_items(elements)?;
        self.construct_ordered(kind, fields, expr)
    }

    /// Build the list a folded imported list constant stands for, from its
    /// already lowered element operands.
    pub(super) fn construct_array_from_operands(
        &mut self,
        operands: Vec<MirOperand>,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let fields = operands
            .into_iter()
            .map(MirAggregateItem::Operand)
            .collect();
        self.construct_ordered(MirAggregateKind::Array, fields, expr)
    }

    fn construct_ordered(
        &mut self,
        kind: MirAggregateKind,
        fields: Vec<MirAggregateItem>,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let ty = self.expr_ty(expr)?;
        let ty = MirType::semantic(self.ordered_literal_carrier(ty.semantic_id()));
        Some(self.construct_temp(kind, MirAggregateFields::Ordered(fields), ty, expr.span))
    }

    /// The ordered collection a literal constructs when its expression type is
    /// the slot it lands in. Typecheck types an expression by its destination,
    /// so a literal stored into `list<T> ∪ none` (or a union with one
    /// collection member) carries the optional/union type; the aggregate
    /// itself is the single collection member, and the surrounding store
    /// widens it into the slot.
    fn ordered_literal_carrier(&self, ty: TypeId) -> TypeId {
        let is_ordered = |ty: TypeId| {
            matches!(
                self.normalized_type(ty),
                Type::Array(_)
                    | Type::BoundedArray { .. }
                    | Type::RangedArray { .. }
                    | Type::Queue(_)
                    | Type::BoundedQueue { .. }
                    | Type::Stack(_)
                    | Type::BoundedStack { .. }
                    | Type::Set(_)
                    | Type::Vector(_, _)
            )
        };
        let members: Vec<TypeId> = match self.normalized_type(ty) {
            Type::Option(inner) => vec![*inner],
            Type::Union(members) => members.clone(),
            _ => return ty,
        };
        let mut ordered = members
            .into_iter()
            .map(|member| self.ordered_literal_carrier(member))
            .filter(|member| is_ordered(*member));
        match (ordered.next(), ordered.next()) {
            (Some(only), None) => only,
            _ => ty,
        }
    }

    fn lower_array_items(&mut self, elements: &[HirArrayElement]) -> Option<Vec<MirAggregateItem>> {
        let mut fields = Vec::with_capacity(elements.len());
        for element in elements {
            match element {
                HirArrayElement::Expr(expr) => {
                    fields.push(MirAggregateItem::Operand(self.lower_expr_value(expr)?));
                }
                HirArrayElement::Spread(expr) => {
                    fields.push(MirAggregateItem::Spread(self.lower_expr_value(expr)?));
                }
                HirArrayElement::Infer(span) => {
                    self.errors.push(MirError::unsupported(
                        *span,
                        "unresolved shape inference hole before MIR lowering",
                    ));
                    return None;
                }
            }
        }
        Some(fields)
    }

    /// Lower a struct literal whose target definition is already known.
    pub(super) fn lower_struct_literal(
        &mut self,
        def_id: DefId,
        fields: &[(radix_lexer::Symbol, HirExpression)],
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let explicit: Vec<_> = fields.iter().map(|(name, value)| (*name, value)).collect();
        let named = self.materialize_struct_fields(def_id, &explicit, &StructFieldFill::Construction, expr.span)?;
        let ty = self.expr_ty(expr)?;
        let constructed = self.construct_temp(
            MirAggregateKind::Struct(def_id),
            MirAggregateFields::Named(named),
            ty,
            expr.span,
        );
        self.call_creo_hook(def_id, &constructed, expr.span);
        Some(constructed)
    }

    /// Run the genus's `creo` post-construction hook on a freshly constructed
    /// aggregate, after field defaults and overrides are merged. A genus
    /// without a `creo` method constructs unchanged.
    fn call_creo_hook(&mut self, def_id: DefId, constructed: &MirOperand, span: Span) {
        let Some(hook) = self.context.creo_hooks.get(&def_id).copied() else {
            return;
        };
        self.append_stmt(MirStatement {
            kind: MirStatementKind::Call {
                destination: None,
                callee: MirCallee::definition_with_type_args(hook, Vec::new()),
                args: vec![constructed.clone()],
            },
            span,
        });
    }

    /// Lower a `verte` aggregate conversion.
    ///
    /// This is not a general cast path. It accepts only the HIR source shapes
    /// that can be represented as aggregate construction: object entries for
    /// structs/maps, array syntax for arrays/sets, and direct variant calls for
    /// enum payloads.
    pub fn lower_verte(
        &mut self,
        source: &HirExpression,
        target: TypeId,
        entries: Option<&Vec<HirObjectField>>,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        // An explicitly parameterised constructor (`Cista<int> { … }`) carries
        // the applied type as the cast target: construct the struct at that
        // applied type, as the inferred form does.
        let applied_struct = match self.types.get(target) {
            Type::Applied(base, _) => match self.types.get(*base) {
                Type::Struct(def_id) => Some(*def_id),
                _ => None,
            },
            _ => None,
        };
        if let Some(def_id) = applied_struct {
            return self.lower_verte_struct(def_id, target, entries, expr);
        }
        match self.types.get(target) {
            Type::Struct(def_id) => {
                let def_id = *def_id;
                self.lower_verte_struct(def_id, target, entries, expr)
            }
            Type::Record(field_tys) => {
                let field_tys = field_tys.clone();
                let Some(entries) = entries else {
                    self.errors.push(MirError::unsupported(
                        expr.span,
                        "record construction without object entries before aggregate MIR lowering",
                    ));
                    return None;
                };
                let fields = self.lower_record_object_fields(entries, &field_tys, expr.span)?;
                // `entries` retain source order; the target record contributes
                // only the solved field carrier for each named value.
                let ty = self.expr_ty(expr)?;
                Some(self.construct_temp(MirAggregateKind::Record, MirAggregateFields::Named(fields), ty, expr.span))
            }
            Type::Map(_, _) | Type::Ledger(_, _) => {
                // A bare `{ "k": v }` literal ascribed with `∷` stays a JSON
                // valor literal source (it has no object entries); its members
                // are the key/value pairs of the ascribed map type, exactly as
                // the declaration form lowers them.
                let fields = match (entries, &source.kind) {
                    (Some(entries), _) => self.lower_map_object_fields(entries, expr.span)?,
                    (
                        None,
                        HirExpressionKind::Literal(HirLiteral::JsonValor(radix_syntax::JsonValue::Object(members))),
                    ) => self.lower_json_map_members(members, expr.span)?,
                    (None, _) => {
                        self.errors.push(MirError::unsupported(
                            expr.span,
                            "map construction without object entries before aggregate MIR lowering",
                        ));
                        return None;
                    }
                };
                let ty = MirType::semantic(target);
                Some(self.construct_temp(MirAggregateKind::Map, MirAggregateFields::Keyed(fields), ty, expr.span))
            }
            Type::Array(_) => {
                let HirExpressionKind::Array(elements) = &source.kind else {
                    self.errors.push(MirError::unsupported(
                        expr.span,
                        "array construction from non-array source before aggregate MIR lowering",
                    ));
                    return None;
                };
                let fields = self.lower_array_items(elements)?;
                let ty = MirType::semantic(target);
                Some(self.construct_temp(MirAggregateKind::Array, MirAggregateFields::Ordered(fields), ty, expr.span))
            }
            Type::Vector(_, _) => {
                let HirExpressionKind::Array(elements) = &source.kind else {
                    self.errors.push(MirError::unsupported(
                        expr.span,
                        "vector construction from non-array source before aggregate MIR lowering",
                    ));
                    return None;
                };
                let fields = self.lower_array_items(elements)?;
                let ty = MirType::semantic(target);
                Some(self.construct_temp(MirAggregateKind::Vector, MirAggregateFields::Ordered(fields), ty, expr.span))
            }
            Type::Set(_) => {
                let HirExpressionKind::Array(elements) = &source.kind else {
                    self.errors.push(MirError::unsupported(
                        expr.span,
                        "set construction from non-array source before aggregate MIR lowering",
                    ));
                    return None;
                };
                let fields = self.lower_array_items(elements)?;
                let ty = MirType::semantic(target);
                Some(self.construct_temp(MirAggregateKind::Set, MirAggregateFields::Ordered(fields), ty, expr.span))
            }
            Type::Enum(_) => {
                let HirExpressionKind::Call(callee, _, args, _) = &source.kind else {
                    self.errors.push(MirError::unsupported(
                        expr.span,
                        "enum construction from non-variant source before aggregate MIR lowering",
                    ));
                    return None;
                };
                let HirExpressionKind::Path(def_id) = &callee.kind else {
                    self.errors.push(MirError::unsupported(
                        expr.span,
                        "enum construction from indirect variant before aggregate MIR lowering",
                    ));
                    return None;
                };
                if !self.context.variant_parents.contains_key(def_id) {
                    self.errors.push(MirError::unsupported(
                        expr.span,
                        "enum construction from non-variant call before aggregate MIR lowering",
                    ));
                    return None;
                }
                let mut lowered_args = Vec::with_capacity(args.len());
                for arg in args {
                    lowered_args.push(self.lower_expr_value(&arg.expr)?);
                }
                let fields = self.variant_payload(*def_id, lowered_args, expr.span);
                Some(self.construct_temp(
                    MirAggregateKind::EnumVariant(*def_id),
                    fields,
                    MirType::semantic(target),
                    expr.span,
                ))
            }
            _ => {
                if entries.is_some() {
                    self.errors
                        .push(MirError::unsupported(expr.span, "verte cast before aggregate MIR lowering"));
                    return None;
                }
                let operand = self.lower_expr_value(source)?;
                let ty = MirType::semantic(target);
                if let Some(conversion) = self.scalar_ascription_cast(source.ty, target) {
                    return Some(self.runtime_call_value(
                        MirIntrinsic::Convert(conversion),
                        vec![operand],
                        ty,
                        expr.span,
                    ));
                }
                Some(self.assign_temp(MirValueKind::Operand(operand), ty, expr.span))
            }
        }
    }

    /// Lower a struct construction whose cast target is the struct (or the
    /// struct applied to type/size arguments) to a struct aggregate.
    fn lower_verte_struct(
        &mut self,
        def_id: DefId,
        target: TypeId,
        entries: Option<&Vec<HirObjectField>>,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let Some(entries) = entries else {
            self.errors.push(MirError::unsupported(
                expr.span,
                "struct construction without object entries before aggregate MIR lowering",
            ));
            return None;
        };
        let fields = self.lower_struct_object_fields(def_id, entries, expr.span)?;
        // A generic struct written bare (`Box { … }`) carries the
        // written raw head in `target`; the checker's contextual
        // instantiation records the applied type (`Box<N>`) on the
        // expression. Construct at the applied type so generic-
        // template instantiation can specialize the temp.
        let ty = self
            .expr_ty(expr)
            .filter(|ty| match self.types.get(ty.semantic_id()) {
                Type::Applied(base, _) => matches!(
                    self.types.get(*base),
                    Type::Struct(expr_def) if *expr_def == def_id
                ),
                _ => false,
            })
            .unwrap_or(MirType::semantic(target));
        let constructed = self.construct_temp(
            MirAggregateKind::Struct(def_id),
            MirAggregateFields::Named(fields),
            ty,
            expr.span,
        );
        self.call_creo_hook(def_id, &constructed, expr.span);
        Some(constructed)
    }

    /// The `∷` ascription cast for a `bivalens` source into a `numerus`
    /// target, or `None` for every other pair.
    ///
    /// WHY: `∷` states only what the compiler can prove (D1.11), so typecheck
    /// rejects every float-to-integer ascription (`SEM016`); the lossy
    /// conversion is `↦`. A boolean is the one cross-family fact left, and the
    /// runner must store it as the 0/1 integer rather than the raw bool.
    fn scalar_ascription_cast(&self, source: Option<TypeId>, target: TypeId) -> Option<MirConversion> {
        let source = source?;
        if !matches!(
            self.normalized_type(target),
            Type::Primitive(Primitive::Numerus) | Type::SizedNumeric(Primitive::Numerus, _)
        ) {
            return None;
        }
        if !matches!(self.normalized_type(source), Type::Primitive(Primitive::Bivalens)) {
            return None;
        }
        Some(MirConversion {
            store: None,
            flavor: MirConversionFlavor::Cast,
            source_ty: MirType::semantic(source),
            target_ty: MirType::semantic(target),
            params: Vec::new(),
            recovery: None,
            struct_defaults: Vec::new(),
        })
    }

    fn lower_struct_object_fields(
        &mut self,
        def_id: DefId,
        entries: &[HirObjectField],
        span: Span,
    ) -> Option<Vec<MirNamedOperand>> {
        let mut explicit = Vec::with_capacity(entries.len());
        let mut source = None;
        for entry in entries {
            let name = match &entry.key {
                HirObjectKey::Ident(name) | HirObjectKey::String(name) => *name,
                HirObjectKey::Computed(expr) => {
                    self.errors.push(MirError::unsupported(
                        expr.span,
                        "computed struct keys before aggregate MIR lowering",
                    ));
                    return None;
                }
                HirObjectKey::Spread(expr) => {
                    source = Some(expr);
                    continue;
                }
            };
            let value = self.required_object_value(entry, span, "struct")?;
            explicit.push((name, value));
        }
        match source {
            Some(source) => self.lower_struct_update_fields(def_id, &explicit, source),
            None => self.materialize_struct_fields(def_id, &explicit, &StructFieldFill::Construction, span),
        }
    }

    /// Labelled record literal entries, in written order (the runner renders a
    /// record in that order). A record has no defaults, spread or computed
    /// keys: every label is written once with its value. A numeric literal
    /// entry takes its field's sized carrier, exactly as a tuple element does.
    fn lower_record_object_fields(
        &mut self,
        entries: &[HirObjectField],
        field_tys: &FxHashMap<Symbol, TypeId>,
        span: Span,
    ) -> Option<Vec<MirNamedOperand>> {
        let mut fields = Vec::with_capacity(entries.len());
        for entry in entries {
            let name = match &entry.key {
                HirObjectKey::Ident(name) | HirObjectKey::String(name) => *name,
                HirObjectKey::Computed(expr) | HirObjectKey::Spread(expr) => {
                    self.errors.push(MirError::unsupported(
                        expr.span,
                        "computed or spread record keys before aggregate MIR lowering",
                    ));
                    return None;
                }
            };
            let value = self.required_object_value(entry, span, "record")?;
            let lowered = self.lower_expr_value(value)?;
            let lowered = match field_tys.get(&name).copied() {
                Some(field_ty) if is_numeric_literal(value) && self.takes_sized_member_type(field_ty) => {
                    self.assign_temp(MirValueKind::Operand(lowered), MirType::semantic(field_ty), value.span)
                }
                _ => lowered,
            };
            fields.push(MirNamedOperand { name, value: lowered });
        }
        Some(fields)
    }

    /// Lower copy-with-changes (`Genus { … } ex source`, D15.1–D15.3).
    ///
    /// Evaluation follows source order: the braced overrides first, then the
    /// source. Every instance field the braces omit is read from the source
    /// place. `from` takes VALUES (need, rule 5): an omitted
    /// collection or object field is duplicated (`copy` on the field), so the
    /// new value shares nothing with the source. Field defaults never apply:
    /// the source supplies every omitted field.
    fn lower_struct_update_fields(
        &mut self,
        def_id: DefId,
        explicit: &[(radix_lexer::Symbol, &HirExpression)],
        source: &HirExpression,
    ) -> Option<Vec<MirNamedOperand>> {
        let mut supplied = FxHashSet::default();
        let mut fields = Vec::with_capacity(explicit.len());
        for (name, value) in explicit {
            supplied.insert(*name);
            fields.push(MirNamedOperand { name: *name, value: self.lower_expr_value(value)? });
        }
        let base = self.lower_projectable_place(source)?;
        let names: Vec<(radix_lexer::Symbol, TypeId)> = if let Some(field_defs) = self.context.structs.get(&def_id) {
            field_defs
                .iter()
                .map(|field| (field.name, field.ty))
                .collect()
        } else if let Some(field_types) = self.context.struct_field_types.get(&def_id) {
            let mut names: Vec<_> = field_types
                .iter()
                .map(|(name, ty)| (*name, ty.semantic_id()))
                .collect();
            names.sort_by_key(|(name, _)| name.0);
            names
        } else {
            self.errors.push(MirError::unsupported(
                source.span,
                "copy-with-changes source of a genus without field metadata",
            ));
            return None;
        };
        for (name, ty) in names {
            if supplied.contains(&name) {
                continue;
            }
            let mut place = base.clone();
            place.projections.push(MirProjection::Field(name));
            let mut value = MirOperand::Place(place);
            if self.field_needs_value_copy(ty) {
                value =
                    self.runtime_call_value(MirIntrinsic::Duplicate, vec![value], MirType::semantic(ty), source.span);
            }
            fields.push(MirNamedOperand { name, value });
        }
        Some(fields)
    }

    /// Whether an inherited field is a referential value (collection, record,
    /// tuple or genus instance, through aliases and options) that
    /// copy-with-changes must duplicate rather than share.
    fn field_needs_value_copy(&self, ty: TypeId) -> bool {
        let mut ty = ty;
        loop {
            match self.types.get(ty) {
                Type::Alias(_, inner) | Type::Option(inner) => ty = *inner,
                Type::Struct(_) | Type::Record(_) | Type::Tuple(_) => return true,
                _ => return self.types.is_aliasing_container(ty),
            }
        }
    }

    /// Merge explicit struct fields with static defaults and omitted `sponte` fields.
    pub(super) fn materialize_struct_fields(
        &mut self,
        def_id: DefId,
        explicit: &[(radix_lexer::Symbol, &HirExpression)],
        fill: &StructFieldFill,
        span: Span,
    ) -> Option<Vec<MirNamedOperand>> {
        let mut supplied = FxHashSet::default();
        let mut fields = Vec::new();
        for (name, value) in explicit {
            supplied.insert(*name);
            fields.push(MirNamedOperand { name: *name, value: self.lower_expr_value(value)? });
        }

        if let Some(field_defs) = self.context.structs.get(&def_id).cloned() {
            for field in field_defs {
                if supplied.contains(&field.name) {
                    continue;
                }
                let value = match fill {
                    StructFieldFill::AllZero => self.type_zero_operand(field.ty, span)?,
                    StructFieldFill::Construction => {
                        if let Some(init) = &field.init {
                            self.lower_expr_value(init)?
                        } else if field.sponte {
                            // The field's own optional flag rides the aggregate
                            // metadata, so the bare nil needs no typed slot here.
                            self.omitted_optional_operand(None, span)
                        } else {
                            continue;
                        }
                    }
                };
                fields.push(MirNamedOperand { name: field.name, value });
            }
        } else if matches!(fill, StructFieldFill::Construction) {
            if let Some(defaults) = self.context.imported_struct_defaults.get(&def_id).cloned() {
                for (name, init) in defaults {
                    if supplied.contains(&name) {
                        continue;
                    }
                    let value = self.lower_expr_value(&init)?;
                    fields.push(MirNamedOperand { name, value });
                }
            }
        } else if matches!(fill, StructFieldFill::AllZero) {
            // Zero-init of a value whose struct is only known through the
            // validation metadata (an imported file-interface struct): the
            // local `context.structs` map carries no entry for it, so fill
            // every field with its type zero instead of producing a hollow
            // aggregate (FMIR e2e-hardening CTO-1).
            if let Some(field_types) = self.context.struct_field_types.get(&def_id).cloned() {
                for (name, ty) in field_types {
                    if supplied.contains(&name) {
                        continue;
                    }
                    let value = self.type_zero_operand(ty.semantic_id(), span)?;
                    fields.push(MirNamedOperand { name, value });
                }
            }
        }

        Some(fields)
    }

    fn lower_map_object_fields(&mut self, entries: &[HirObjectField], span: Span) -> Option<Vec<MirKeyValueOperand>> {
        let mut fields = Vec::with_capacity(entries.len());
        for entry in entries {
            let key = match &entry.key {
                HirObjectKey::Ident(name) | HirObjectKey::String(name) => {
                    MirOperand::Constant(MirConstant::String(*name))
                }
                HirObjectKey::Computed(expr) => self.lower_expr_value(expr)?,
                HirObjectKey::Spread(expr) => {
                    self.errors
                        .push(MirError::unsupported(expr.span, "map spread before aggregate MIR lowering"));
                    return None;
                }
            };
            let value = self.required_object_value(entry, span, "map")?;
            fields.push(MirKeyValueOperand { key, value: self.lower_expr_value(value)? });
        }
        Some(fields)
    }

    /// Lower a compile-time JSON valor literal into map/array aggregates when
    /// semantic typing already fixed the target collection shape.
    pub(super) fn lower_json_valor_literal(
        &mut self,
        value: &radix_syntax::JsonValue,
        ty: Option<TypeId>,
        span: Span,
    ) -> Option<MirOperand> {
        let Some(type_id) = ty else {
            self.errors
                .push(MirError::missing_type(span, "JSON valor literal"));
            return None;
        };
        let mir_ty = MirType::semantic(type_id);
        match self.types.get(type_id) {
            Type::Map(_, _) | Type::Ledger(_, _) => {
                let radix_syntax::JsonValue::Object(members) = value else {
                    self.errors.push(MirError::unsupported(
                        span,
                        "JSON valor map literal without object shape before aggregate MIR lowering",
                    ));
                    return None;
                };
                let fields = self.lower_json_map_members(members, span)?;
                Some(self.construct_temp(MirAggregateKind::Map, MirAggregateFields::Keyed(fields), mir_ty, span))
            }
            Type::Array(_) => {
                let radix_syntax::JsonValue::Array(items) = value else {
                    self.errors.push(MirError::unsupported(
                        span,
                        "JSON valor array literal without array shape before aggregate MIR lowering",
                    ));
                    return None;
                };
                let fields = self.lower_json_array_items(items, span)?;
                Some(self.construct_temp(MirAggregateKind::Array, MirAggregateFields::Ordered(fields), mir_ty, span))
            }
            Type::Primitive(Primitive::Valor) => match value {
                radix_syntax::JsonValue::Object(members) => {
                    let fields = self.lower_json_map_members(members, span)?;
                    Some(self.construct_temp(MirAggregateKind::Map, MirAggregateFields::Keyed(fields), mir_ty, span))
                }
                radix_syntax::JsonValue::Array(items) => {
                    let fields = self.lower_json_array_items(items, span)?;
                    Some(self.construct_temp(
                        MirAggregateKind::Array,
                        MirAggregateFields::Ordered(fields),
                        mir_ty,
                        span,
                    ))
                }
                scalar => self.lower_json_value(scalar, span),
            },
            Type::Primitive(Primitive::Json) => {
                if let radix_syntax::JsonValue::Object(members) = value {
                    let fields = self.lower_json_map_members(members, span)?;
                    Some(self.construct_temp(MirAggregateKind::Map, MirAggregateFields::Keyed(fields), mir_ty, span))
                } else {
                    self.errors.push(MirError::unsupported(
                        span,
                        "JSON document literal without object shape before MIR lowering",
                    ));
                    None
                }
            }
            _ => {
                self.errors.push(MirError::unsupported(
                    span,
                    "JSON valor literal without aggregate target type before runtime valor MIR lowering",
                ));
                None
            }
        }
    }

    fn lower_json_map_members(
        &mut self,
        members: &[radix_syntax::JsonMember],
        span: Span,
    ) -> Option<Vec<MirKeyValueOperand>> {
        let mut fields = Vec::with_capacity(members.len());
        for member in members {
            let key = MirOperand::Constant(MirConstant::String(member.key));
            let value = self.lower_json_value(&member.value, span)?;
            fields.push(MirKeyValueOperand { key, value });
        }
        Some(fields)
    }

    fn lower_json_array_items(
        &mut self,
        items: &[radix_syntax::JsonValue],
        span: Span,
    ) -> Option<Vec<MirAggregateItem>> {
        let mut fields = Vec::with_capacity(items.len());
        for item in items {
            fields.push(MirAggregateItem::Operand(self.lower_json_value(item, span)?));
        }
        Some(fields)
    }

    fn lower_json_value(&mut self, value: &radix_syntax::JsonValue, span: Span) -> Option<MirOperand> {
        match value {
            radix_syntax::JsonValue::String(sym) => Some(MirOperand::Constant(MirConstant::String(*sym))),
            radix_syntax::JsonValue::Numerus(value) => Some(MirOperand::Constant(MirConstant::Int(*value))),
            radix_syntax::JsonValue::Fractus(value) => Some(MirOperand::Constant(MirConstant::Float(*value))),
            radix_syntax::JsonValue::Bool(value) => Some(MirOperand::Constant(MirConstant::Bool(*value))),
            radix_syntax::JsonValue::Null => Some(MirOperand::Constant(MirConstant::Nil)),
            radix_syntax::JsonValue::Object(members) => {
                let fields = self.lower_json_map_members(members, span)?;
                let ty = MirType::semantic(self.types.primitive(Primitive::Valor));
                Some(self.construct_temp(MirAggregateKind::Map, MirAggregateFields::Keyed(fields), ty, span))
            }
            radix_syntax::JsonValue::Array(items) => {
                let fields = self.lower_json_array_items(items, span)?;
                let ty = MirType::semantic(self.types.primitive(Primitive::Valor));
                Some(self.construct_temp(MirAggregateKind::Array, MirAggregateFields::Ordered(fields), ty, span))
            }
        }
    }

    fn required_object_value<'a>(
        &mut self,
        entry: &'a HirObjectField,
        span: Span,
        aggregate_kind: &'static str,
    ) -> Option<&'a HirExpression> {
        if let Some(value) = &entry.value {
            Some(value)
        } else {
            self.errors.push(MirError::unsupported(
                span,
                format!("{aggregate_kind} field without value before aggregate MIR lowering"),
            ));
            None
        }
    }

    /// Whether an aggregate member slot requires a numeric literal at its own
    /// sized carrier: a sized numeric (signed/unsigned/decimal), a float
    /// width, or a wrapping/saturating word (whose store reduces at the temp).
    ///
    /// Mirrors the classification `lower_call_arg_value` uses for call
    /// arguments (d11-6 ruling 12a).
    fn takes_sized_member_type(&self, member_ty: TypeId) -> bool {
        match self.normalized_type(self.types.peel_access_marker(member_ty).1) {
            Type::SizedNumeric(Primitive::Numerus, width) => width.integer_range().is_some() || width.is_decimal(),
            Type::SizedNumeric(Primitive::Fractus, _)
            | Type::ModularWord(_)
            | Type::SaturatingWord(_)
            | Type::RangedInt { .. } => true,
            _ => false,
        }
    }
}

/// Whether a HIR expression is an integer or decimal literal, the elements an
/// aggregate member slot re-types to its sized carrier.
fn is_numeric_literal(expr: &HirExpression) -> bool {
    matches!(
        expr.kind,
        HirExpressionKind::Literal(HirLiteral::Int(_) | HirLiteral::Float(_) | HirLiteral::FloatText { .. })
    )
}
