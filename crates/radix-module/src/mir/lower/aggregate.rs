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
        if step.is_some() {
            self.errors.push(MirError::unsupported(
                expr.span,
                "stepped interval value before interval MIR lowering",
            ));
            return None;
        }
        let fields = vec![
            MirAggregateItem::Operand(self.lower_expr_value(start)?),
            MirAggregateItem::Operand(self.lower_expr_value(end)?),
        ];
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
            | Type::Queue(_)
            | Type::BoundedQueue { .. }
            | Type::Stack(_)
            | Type::BoundedStack { .. } => Some(self.construct_temp(
                MirAggregateKind::Array,
                MirAggregateFields::Ordered(Vec::new()),
                ty,
                expr.span,
            )),
            Type::Map(_, _) => {
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
    pub(super) fn lower_tuple(&mut self, items: &[HirExpression], expr: &HirExpression) -> Option<MirOperand> {
        let mut fields = Vec::with_capacity(items.len());
        for item in items {
            fields.push(MirAggregateItem::Operand(self.lower_expr_value(item)?));
        }
        let ty = self.expr_ty(expr)?;
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
        let ty = self.expr_ty(expr)?;
        Some(self.construct_temp(kind, MirAggregateFields::Ordered(fields), ty, expr.span))
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
            }
        }
        Some(fields)
    }

    /// Lower a struct literal whose target definition is already known.
    pub(super) fn lower_struct_literal(
        &mut self,
        def_id: DefId,
        fields: &[(crate::lexer::Symbol, HirExpression)],
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let explicit: Vec<_> = fields.iter().map(|(name, value)| (*name, value)).collect();
        let named = self.materialize_struct_fields(def_id, &explicit, StructFieldFill::Construction, expr.span)?;
        let ty = self.expr_ty(expr)?;
        Some(self.construct_temp(
            MirAggregateKind::Struct(def_id),
            MirAggregateFields::Named(named),
            ty,
            expr.span,
        ))
    }

    /// Lower a `verte` aggregate conversion.
    ///
    /// This is not a general cast path. It accepts only the HIR source shapes
    /// that can be represented as aggregate construction: object entries for
    /// structs/maps, array syntax for arrays/sets, and direct variant calls for
    /// enum payloads.
    pub(super) fn lower_verte(
        &mut self,
        source: &HirExpression,
        target: TypeId,
        entries: Option<&Vec<HirObjectField>>,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        match self.types.get(target) {
            Type::Struct(def_id) => {
                let Some(entries) = entries else {
                    self.errors.push(MirError::unsupported(
                        expr.span,
                        "struct construction without object entries before aggregate MIR lowering",
                    ));
                    return None;
                };
                let fields = self.lower_struct_object_fields(*def_id, entries, expr.span)?;
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
                            Type::Struct(expr_def) if expr_def == def_id
                        ),
                        _ => false,
                    })
                    .unwrap_or(MirType::semantic(target));
                Some(self.construct_temp(
                    MirAggregateKind::Struct(*def_id),
                    MirAggregateFields::Named(fields),
                    ty,
                    expr.span,
                ))
            }
            Type::Map(_, _) => {
                // An object source carries keyed entries; an empty array
                // source (`[] ∷ tabula<K, V>`) carries none, so both spell
                // the same construction. A non-empty array has no key/value
                // pairing to lower and fails closed.
                let fields = match entries {
                    Some(entries) => self.lower_map_object_fields(entries, expr.span)?,
                    None if matches!(&source.kind, HirExpressionKind::Array(elements) if elements.is_empty()) => {
                        Vec::new()
                    }
                    None => {
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
                Some(self.assign_temp(MirValueKind::Operand(operand), ty, expr.span))
            }
        }
    }

    fn lower_struct_object_fields(
        &mut self,
        def_id: DefId,
        entries: &[HirObjectField],
        span: Span,
    ) -> Option<Vec<MirNamedOperand>> {
        let mut explicit = Vec::with_capacity(entries.len());
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
                    self.errors
                        .push(MirError::unsupported(expr.span, "struct spread before aggregate MIR lowering"));
                    return None;
                }
            };
            let value = self.required_object_value(entry, span, "struct")?;
            explicit.push((name, value));
        }
        self.materialize_struct_fields(def_id, &explicit, StructFieldFill::Construction, span)
    }

    /// Merge explicit struct fields with static defaults and omitted `sponte` fields.
    pub(super) fn materialize_struct_fields(
        &mut self,
        def_id: DefId,
        explicit: &[(crate::lexer::Symbol, &HirExpression)],
        fill: StructFieldFill,
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
                            self.omitted_optional_operand(span)
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
        value: &crate::syntax::JsonValue,
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
            Type::Map(_, _) => {
                let crate::syntax::JsonValue::Object(members) = value else {
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
                let crate::syntax::JsonValue::Array(items) = value else {
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
                crate::syntax::JsonValue::Object(members) => {
                    let fields = self.lower_json_map_members(members, span)?;
                    Some(self.construct_temp(MirAggregateKind::Map, MirAggregateFields::Keyed(fields), mir_ty, span))
                }
                crate::syntax::JsonValue::Array(items) => {
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
                if let crate::syntax::JsonValue::Object(members) = value {
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
        members: &[crate::syntax::JsonMember],
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
        items: &[crate::syntax::JsonValue],
        span: Span,
    ) -> Option<Vec<MirAggregateItem>> {
        let mut fields = Vec::with_capacity(items.len());
        for item in items {
            fields.push(MirAggregateItem::Operand(self.lower_json_value(item, span)?));
        }
        Some(fields)
    }

    fn lower_json_value(&mut self, value: &crate::syntax::JsonValue, span: Span) -> Option<MirOperand> {
        match value {
            crate::syntax::JsonValue::String(sym) => Some(MirOperand::Constant(MirConstant::String(*sym))),
            crate::syntax::JsonValue::Numerus(value) => Some(MirOperand::Constant(MirConstant::Int(*value))),
            crate::syntax::JsonValue::Fractus(value) => Some(MirOperand::Constant(MirConstant::Float(*value))),
            crate::syntax::JsonValue::Bool(value) => Some(MirOperand::Constant(MirConstant::Bool(*value))),
            crate::syntax::JsonValue::Null => Some(MirOperand::Constant(MirConstant::Nil)),
            crate::syntax::JsonValue::Object(members) => {
                let fields = self.lower_json_map_members(members, span)?;
                let ty = MirType::semantic(self.types.primitive(Primitive::Valor));
                Some(self.construct_temp(MirAggregateKind::Map, MirAggregateFields::Keyed(fields), ty, span))
            }
            crate::syntax::JsonValue::Array(items) => {
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
}
