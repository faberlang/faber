//! Aggregate shape and closure-value checks.

use super::{FunctionScope, GenericTypeBindings, Validator};
use crate::generic::MirShapeBindings;
use crate::{
    MirAggregate, MirAggregateFields, MirAggregateItem, MirAggregateKind, MirClosureEnvironmentId,
    MirClosureValue, MirConstant, MirGenericBindings, MirNamedOperand, MirOperand, MirPlace,
    MirType,
};
use radix_hir::DefId;
use radix_lexer::{Span, Symbol};
use radix_types::{IndexExpr, Primitive, RuntimeExtentId, Type};
use rustc_hash::{FxHashMap, FxHashSet};

impl Validator<'_, '_> {
    pub(crate) fn validate_aggregate(
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
                if !matches!(items.len(), 2 | 3) {
                    self.error(
                        span,
                        "interval aggregate requires two bounds and an optional step",
                    );
                }
                // The optional third operand is a window step; it is numeric,
                // not an element of the interval.
                let (bounds, step) = items.split_at(items.len().min(2));
                for item in bounds {
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
                if let Some(step) = step.first() {
                    self.validate_interval_step(scope, step, span);
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
                if let Some(expected_len) = expected_len
                    && items.len() != expected_len
                {
                    self.error(span, "vector aggregate element count does not match width");
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
                            crate::matrix_shape::literal_matrix_shape(
                                shape,
                                self.context.types.as_table(),
                            ) {
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
                if let Some(expected_len) = expected_len
                    && items.len() != expected_len
                {
                    self.error(span, "matrix aggregate element count does not match shape");
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
                    Type::Array(inner)
                    | Type::BoundedArray { element: inner, .. }
                    | Type::Queue(inner)
                    | Type::BoundedQueue { element: inner, .. }
                    | Type::Stack(inner)
                    | Type::BoundedStack { element: inner, .. }
                    | Type::Set(inner) => Some(MirType::semantic(*inner)),
                    Type::Primitive(Primitive::Valor)
                        if matches!(aggregate.kind, MirAggregateKind::Array) =>
                    {
                        None
                    }
                    _ => {
                        self.error(
                            span,
                            "ordered collection aggregate type is not array, queue, stack, or set",
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
                                // WHY: a literal containing a spread gets no literal
                                // capacity, so a bounded-array source never matches the
                                // plain array type whole; its elements are what flow.
                                let spread_element = match self.type_kind(value_ty) {
                                    Type::Array(inner)
                                    | Type::BoundedArray { element: inner, .. }
                                        if matches!(aggregate.kind, MirAggregateKind::Array) =>
                                    {
                                        Some(MirType::semantic(*inner))
                                    }
                                    _ => None,
                                };
                                match (spread_element, element_ty) {
                                    (Some(spread_element), Some(element_ty)) => {
                                        self.require_assignable(
                                            spread_element,
                                            element_ty,
                                            span,
                                            "aggregate spread type mismatch",
                                        );
                                    }
                                    _ => self.require_assignable(
                                        value_ty,
                                        aggregate.ty,
                                        span,
                                        "aggregate spread type mismatch",
                                    ),
                                }
                            }
                        }
                    }
                }
            }
            (MirAggregateKind::Map, MirAggregateFields::Keyed(items)) => {
                let (key_ty, value_ty) = match self.type_kind(aggregate.ty) {
                    Type::Map(key, value) | Type::Ledger(key, value) => (
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
                    // A generic struct construction carries its applied type
                    // (`Box<N>`); the base must still be this struct.
                    Type::Applied(base, _) => {
                        if !matches!(
                            self.context.types.get(*base),
                            Type::Struct(type_def) if type_def == def_id
                        ) {
                            self.error(span, "struct aggregate type is not struct");
                        }
                    }
                    _ => self.error(span, "struct aggregate type is not struct"),
                }
                self.validate_named_aggregate_fields(
                    scope,
                    *def_id,
                    Some(aggregate.ty),
                    items,
                    true,
                    span,
                );
            }
            (MirAggregateKind::EnumVariant(def_id), MirAggregateFields::Named(items)) => {
                if let Some(enum_def) = self.enum_def_from_type(aggregate.ty) {
                    self.validate_variant_parent(*def_id, enum_def, span);
                } else {
                    self.error(span, "enum variant aggregate type is not enum");
                }
                self.validate_named_aggregate_fields(
                    scope,
                    *def_id,
                    Some(aggregate.ty),
                    items,
                    false,
                    span,
                );
            }
            (MirAggregateKind::EnumVariant(def_id), MirAggregateFields::Ordered(items)) => {
                if let Some(enum_def) = self.enum_def_from_type(aggregate.ty) {
                    self.validate_variant_parent(*def_id, enum_def, span);
                } else {
                    self.error(span, "enum variant aggregate type is not enum");
                }
                self.validate_ordered_variant_payload(scope, *def_id, aggregate.ty, items, span);
            }
            (MirAggregateKind::SharedCell, MirAggregateFields::Ordered(items)) => {
                self.validate_shared_cell_aggregate(scope, aggregate.ty, items, span);
            }
            _ => self.error(
                span,
                "aggregate payload shape does not match aggregate kind",
            ),
        }
    }

    /// The optional third interval operand: a stride, so it is a defined
    /// numeric operand and never a spread.
    fn validate_interval_step(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        step: &MirAggregateItem,
        span: Span,
    ) {
        let MirAggregateItem::Operand(step) = step else {
            self.error(span, "interval aggregate step cannot spread");
            return;
        };
        if let Some(step_ty) = self.validate_operand(scope, step, span)
            && !self.is_numeric_type(step_ty)
        {
            self.error(span, "interval aggregate step must be numeric");
        }
    }

    /// A shared cell holds exactly one positional value of the cell's
    /// (content) type.
    fn validate_shared_cell_aggregate(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        cell_ty: MirType,
        items: &[MirAggregateItem],
        span: Span,
    ) {
        let [MirAggregateItem::Operand(value)] = items else {
            self.error(span, "shared cell aggregate requires exactly one operand");
            return;
        };
        if let Some(value_ty) = self.validate_operand(scope, value, span) {
            self.require_assignable_or_fitting_constant(
                value,
                value_ty,
                cell_ty,
                span,
                "shared cell content type mismatch",
            );
        }
    }

    pub(crate) fn validate_record_aggregate_fields(
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

    pub(crate) fn validate_closure_environment_aggregate(
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

    pub(crate) fn validate_closure_value(
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

    pub(crate) fn validate_named_aggregate_fields(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        def_id: DefId,
        aggregate_ty: Option<MirType>,
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
            // A generic construction's field types must instantiate at the
            // aggregate's applied arguments; the raw metadata still carries
            // the declaration's size params.
            let expected_ty = match (aggregate_ty, is_struct) {
                (Some(aggregate_ty), true)
                    if matches!(self.type_kind(aggregate_ty), Type::Applied(..)) =>
                {
                    match self.project_field(aggregate_ty, item.name, span) {
                        Some(ty) => ty,
                        None => continue,
                    }
                }
                (Some(aggregate_ty), false)
                    if matches!(self.type_kind(aggregate_ty), Type::Applied(..)) =>
                {
                    match self.instantiate_variant_field_ty(aggregate_ty, expected_ty, span) {
                        Some(ty) => ty,
                        None => continue,
                    }
                }
                _ => expected_ty,
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
                self.require_assignable_or_fitting_constant(
                    &item.value,
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

    /// A positional (`Ordered`) variant aggregate carries the variant's
    /// payload fields in declaration order. Check arity and operand types the
    /// way the tuple case and the named-variant path do; `variant_fields` is
    /// unordered, so the declaration order comes from `variant_field_order`.
    fn validate_ordered_variant_payload(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        def_id: DefId,
        aggregate_ty: MirType,
        items: &[MirAggregateItem],
        span: Span,
    ) {
        let Some(fields) = self.context.variant_fields.get(&def_id).cloned() else {
            // No declared payload metadata to check against; still validate
            // every operand (and reject a positional spread).
            for item in items {
                match item {
                    MirAggregateItem::Operand(value) => {
                        self.validate_operand(scope, value, span);
                    }
                    MirAggregateItem::Spread(value) => {
                        self.validate_operand(scope, value, span);
                        self.error(span, "enum variant aggregate spread is unsupported");
                    }
                }
            }
            return;
        };
        let Some(field_tys) = self.variant_payload_field_tys(def_id, &fields, span) else {
            return;
        };
        if items.len() != field_tys.len() {
            self.error(
                span,
                format!(
                    "enum variant aggregate element count {} does not match variant payload arity {}",
                    items.len(),
                    field_tys.len()
                ),
            );
        }
        for (i, item) in items.iter().enumerate() {
            let (value, is_spread) = match item {
                MirAggregateItem::Operand(value) => (value, false),
                MirAggregateItem::Spread(value) => (value, true),
            };
            let value_ty = self.validate_operand(scope, value, span);
            if is_spread {
                self.error(span, "enum variant aggregate spread is unsupported");
                continue;
            }
            let Some(raw_field_ty) = field_tys.get(i).copied() else {
                continue;
            };
            // A generic construction's payload field types instantiate at the
            // aggregate's applied arguments, as in the named-variant path.
            let expected_ty = if matches!(self.type_kind(aggregate_ty), Type::Applied(..)) {
                match self.instantiate_variant_field_ty(aggregate_ty, raw_field_ty, span) {
                    Some(ty) => ty,
                    None => continue,
                }
            } else {
                raw_field_ty
            };
            if let Some(value_ty) = value_ty {
                self.require_assignable_or_fitting_constant(
                    value,
                    value_ty,
                    expected_ty,
                    span,
                    "enum variant aggregate element type mismatch",
                );
            }
        }
    }

    /// Declared payload field types of a variant in declaration order.
    ///
    /// `variant_fields` is an unordered map; `variant_field_order` records the
    /// order the positional form is written in. Without an order record, fall
    /// back to the same Symbol ordering `enum_representation` uses.
    fn variant_payload_field_tys(
        &mut self,
        def_id: DefId,
        fields: &FxHashMap<Symbol, MirType>,
        span: Span,
    ) -> Option<Vec<MirType>> {
        if let Some(order) = self.context.variant_field_order.get(&def_id).cloned() {
            let mut field_tys = Vec::with_capacity(order.len());
            for name in &order {
                match fields.get(name).copied() {
                    Some(field_ty) => field_tys.push(field_ty),
                    None => {
                        self.error(
                            span,
                            "enum variant aggregate payload metadata is missing a declared field",
                        );
                        return None;
                    }
                }
            }
            return Some(field_tys);
        }
        let mut entries = fields.iter().collect::<Vec<_>>();
        entries.sort_by_key(|(name, _)| name.0);
        Some(entries.into_iter().map(|(_, ty)| *ty).collect())
    }

    /// A generic variant construction's payload field types instantiate at
    /// the aggregate's applied arguments (`Opt<numerus>` makes `T` a
    /// `numerus`); the raw metadata still carries the declaration's params.
    pub(crate) fn instantiate_variant_field_ty(
        &mut self,
        aggregate_ty: MirType,
        raw_field_ty: MirType,
        span: Span,
    ) -> Option<MirType> {
        let Type::Applied(base, args) = self.type_kind(aggregate_ty).clone() else {
            return Some(raw_field_ty);
        };
        let Type::Enum(enum_def) = self.context.types.get(base) else {
            return Some(raw_field_ty);
        };
        let Some(params) = self.context.enum_type_params.get(enum_def) else {
            return Some(raw_field_ty);
        };
        if params.len() != args.len() {
            self.error(
                span,
                "applied enum variant aggregate type argument count mismatch",
            );
            return None;
        }
        let type_args = args
            .into_iter()
            .map(|arg| match arg {
                radix_types::AppliedArg::Type(id) => Some(id),
                radix_types::AppliedArg::Size(_) => None,
            })
            .collect::<Option<Vec<_>>>()?;
        let bindings = params
            .iter()
            .copied()
            .zip(type_args.into_iter().map(MirType::semantic))
            .collect::<MirGenericBindings>();
        self.instantiate_existing(raw_field_ty, &bindings, &MirShapeBindings::default(), span)
            .map_err(|error| self.error(error.span, error.message))
            .ok()
    }

    /// A call result whose destination tensor type carries an
    /// expression-origin runtime axis (`Runtime(ExpressionAxis)`, recorded by
    /// the checker when the call's extent is not a compile-time constant at
    /// the store boundary) against a callee result whose extent is concrete,
    /// for example the monomorphized instance of an affine generic
    /// (`tensor<f32, [N - 1]>` at `N = 4`). The equality of those two extents
    /// is the store's runtime shape check, so only the axes the destination
    /// marks as expression-origin may differ; element type, rank, and every
    /// other axis must still agree exactly.
    fn destination_defers_to_store_shape_check(
        &self,
        expected_ty: MirType,
        destination_ty: MirType,
    ) -> bool {
        let types = self.context.types.as_table();
        let (Some((element, expected_dims)), Some((destination_element, destination_dims))) = (
            super::value::tensor_type_dims(types, expected_ty.semantic_id()),
            super::value::tensor_type_dims(types, destination_ty.semantic_id()),
        ) else {
            return false;
        };
        if !types.equals(element, destination_element)
            || expected_dims.len() != destination_dims.len()
        {
            return false;
        }
        let mut deferred = false;
        for (expected_dim, destination_dim) in expected_dims.iter().zip(&destination_dims) {
            if types.index_equals(*expected_dim, *destination_dim) {
                continue;
            }
            if !matches!(
                types.get_index(*destination_dim),
                IndexExpr::Runtime(RuntimeExtentId::ExpressionAxis { .. })
            ) {
                return false;
            }
            deferred = true;
        }
        deferred
    }

    pub(crate) fn validate_optional_destination(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        destination: Option<&MirPlace>,
        expected_ty: Option<MirType>,
        span: Span,
        generic_bindings: &mut GenericTypeBindings,
    ) {
        let mut shape_bindings = MirShapeBindings::default();
        self.validate_optional_destination_with_shapes(
            scope,
            destination,
            expected_ty,
            span,
            generic_bindings,
            &mut shape_bindings,
        );
    }

    pub(crate) fn validate_optional_destination_with_shapes(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        destination: Option<&MirPlace>,
        expected_ty: Option<MirType>,
        span: Span,
        generic_bindings: &mut GenericTypeBindings,
        shape_bindings: &mut MirShapeBindings,
    ) {
        match (destination, expected_ty) {
            (Some(destination), Some(expected_ty)) => {
                if let Some(destination_ty) = self.validate_place(scope, destination, span) {
                    self.bind_generic_type_witness(
                        expected_ty,
                        destination_ty,
                        generic_bindings,
                        shape_bindings,
                        span,
                        "destination type mismatch",
                    );
                    if let Some(expected_ty) = self.instantiate_generic_type(
                        expected_ty,
                        generic_bindings,
                        shape_bindings,
                        span,
                    ) && !self
                        .destination_defers_to_store_shape_check(expected_ty, destination_ty)
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
}
