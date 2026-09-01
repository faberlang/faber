//! Aggregate shape and closure-value checks.

use super::{FunctionScope, GenericTypeBindings, Validator};
use crate::generic::MirShapeBindings;
use crate::{
    MirAggregate, MirAggregateFields, MirAggregateItem, MirAggregateKind, MirClosureEnvironmentId,
    MirClosureValue, MirConstant, MirNamedOperand, MirOperand, MirPlace, MirType,
};
use radix_hir::DefId;
use radix_lexer::{Span, Symbol};
use radix_types::{IndexExpr, Primitive, Type};
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
                self.validate_named_aggregate_fields(scope, *def_id, None, items, false, span);
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
                    ) {
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
