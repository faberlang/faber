//! Index inference integration on [`TypeChecker`].
//!
//! Constraint hooks for tensor shape inference during typechecking.

use crate::index::IndexExpr;
use crate::index::IndexId;
use crate::{IndexConstraint, SemanticErrorKind};
use radix_lexer::Span;

#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for typechecking
use super::*;

pub(super) const ISSUE_INCOMPATIBLE_TENSOR_INDEX: &str = "incompatible_tensor_index";

impl TypeChecker<'_> {
    /// Record a relation at the currently active shape boundary.
    pub(crate) fn record_shape_relation(
        &mut self,
        left: IndexId,
        right: IndexId,
        span: Span,
        boundary: ShapeRelationBoundary,
    ) {
        let relation = ShapeRelation {
            left,
            right,
            span,
            boundary,
        };
        let rows = match boundary {
            ShapeRelationBoundary::Call => self
                .current_shape_call
                .map(|site| self.shape_obligations.calls.entry(site).or_default()),
            ShapeRelationBoundary::FunctionEntry | ShapeRelationBoundary::FunctionReturn => self
                .current_function_def
                .map(|def_id| self.shape_obligations.functions.entry(def_id).or_default()),
        };
        let Some(rows) = rows else {
            return;
        };
        if !rows.contains(&relation) {
            rows.push(relation);
        }
    }

    /// Intern a runtime source for one tensor axis at the active call or
    /// return boundary. Call-argument ordinals ensure expression-created
    /// carriers are evaluated once and then queried through their value.
    pub(crate) fn runtime_boundary_axis(
        &mut self,
        axis: u32,
    ) -> Option<(IndexId, ShapeRelationBoundary)> {
        let source = if let Some(argument) = self.current_shape_argument {
            let source = if self.current_shape_fields.is_empty() {
                RuntimeExtentId::CallArgumentAxis { argument, axis }
            } else {
                RuntimeExtentId::CallArgumentFieldAxis {
                    argument,
                    fields: self.current_shape_fields.clone(),
                    axis,
                }
            };
            Some((source, ShapeRelationBoundary::Call))
        } else if let Some(expression) = self.current_shape_return_expr {
            let source = if self.current_shape_fields.is_empty() {
                RuntimeExtentId::ExpressionAxis {
                    expression: expression.0,
                    axis,
                }
            } else {
                RuntimeExtentId::ExpressionFieldAxis {
                    expression: expression.0,
                    fields: self.current_shape_fields.clone(),
                    axis,
                }
            };
            Some((source, ShapeRelationBoundary::FunctionReturn))
        } else {
            None
        }?;
        Some((
            self.types.intern_index(IndexExpr::Runtime(source.0)),
            source.1,
        ))
    }

    /// Intern an extent read from an expression-produced tensor carrier.
    /// Used by view intrinsics whose shape result copies axes from the receiver.
    pub(crate) fn runtime_expression_axis(
        &mut self,
        expression: HirId,
        fields: &[Symbol],
        axis: u32,
    ) -> IndexId {
        let source = if fields.is_empty() {
            RuntimeExtentId::ExpressionAxis {
                expression: expression.0,
                axis,
            }
        } else {
            RuntimeExtentId::ExpressionFieldAxis {
                expression: expression.0,
                fields: fields.to_vec(),
                axis,
            }
        };
        self.types.intern_index(IndexExpr::Runtime(source))
    }

    /// Use a runtime-aware shape equation when checking a call argument or a
    /// return value. `None` means the caller should keep the ordinary strict
    /// index-unification path; `Some(Err(()))` means this path emitted the
    /// usual incompatible-index diagnostic.
    pub(crate) fn unify_shape_at_boundary(
        &mut self,
        actual_shape: IndexId,
        expected_shape: IndexId,
        span: Span,
    ) -> Option<Result<IndexId, ()>> {
        let boundary = if self.current_shape_argument.is_some() {
            ShapeRelationBoundary::Call
        } else if self.current_shape_return_expr.is_some() {
            ShapeRelationBoundary::FunctionReturn
        } else {
            return None;
        };
        let actual_shape = self.resolve_index(actual_shape);
        let expected_shape = self.resolve_index(expected_shape);
        if matches!(
            self.types.get_index(actual_shape),
            IndexExpr::Infer(_) | IndexExpr::Unspecified
        ) || matches!(
            self.types.get_index(expected_shape),
            IndexExpr::Infer(_) | IndexExpr::Unspecified
        ) {
            // A root hole has no rank to preserve. A tuple with per-axis
            // Infer terms remains admissible below because its rank is known.
            self.missing_type_annotation("cannot_infer_tensor_shape", span);
            return Some(Err(()));
        }

        let actual_dims = Self::shape_dimensions(actual_shape, self.types)?;
        let expected_dims = Self::shape_dimensions(expected_shape, self.types)?;
        if actual_dims.len() != expected_dims.len() {
            self.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_INCOMPATIBLE_TENSOR_INDEX,
                span,
            );
            return Some(Err(()));
        }

        let mut unified_dims = Vec::with_capacity(actual_dims.len());
        for (axis, (actual, expected)) in actual_dims.into_iter().zip(expected_dims).enumerate() {
            let mut actual = self.resolve_index(actual);
            let expected = self.runtime_substitute_shape_params(expected);
            // A symbolic shape written on an actual tensor describes its
            // carrier, but the runtime check must read the carrier passed at
            // this boundary. Rebase dynamic dimensions to the call argument
            // (or returned expression) axis before unification so inferred
            // generic sizes and retained equations have a concrete value
            // source even when the tensor was constructed from a scalar
            // local or an imported generic parameter.
            let immutable_scalar_binding = boundary == ShapeRelationBoundary::Call
                && self.index_has_immutable_scalar_witness(actual);
            if radix_types::evaluate_index_dimension(actual, self.types)
                .ok()
                .flatten()
                .is_none()
                && !matches!(self.types.get_index(actual), IndexExpr::Infer(_))
            {
                if immutable_scalar_binding {
                    // Keep a stable immutable scalar witness for generic
                    // shape inference, but still guard the physical carrier
                    // axis against it for every argument at this call.
                    if let Some((runtime_axis, boundary)) = self.runtime_boundary_axis(axis as u32)
                    {
                        self.record_shape_relation(runtime_axis, actual, span, boundary);
                    }
                } else if let Some((runtime_axis, _)) = self.runtime_boundary_axis(axis as u32) {
                    actual = runtime_axis;
                }
            }
            let unified = {
                let mut unifier = self.index_infer.unifier(self.types);
                unifier.unify(actual, expected)
            };
            match unified {
                Ok(index) => unified_dims.push(index),
                Err(_) => {
                    // The unifier can have already bound this dimension's
                    // original inference hole while checking an earlier
                    // argument. Resolve that binding before retaining a
                    // recoverable relation; otherwise the backend receives
                    // an Infer node instead of the first argument's witness.
                    let expected = self.canonicalize_index(expected);
                    // A concrete arithmetic failure is a contradiction, not
                    // an unknown runtime relation. In particular, after an
                    // earlier argument binds D=5, `D / 2` cannot match a
                    // tensor of width 2: exactness must reject at this call.
                    if radix_types::evaluate_index_dimension(actual, self.types).is_err()
                        || radix_types::evaluate_index_dimension(expected, self.types).is_err()
                    {
                        self.error_issue(
                            SemanticErrorKind::TypeMismatch,
                            ISSUE_INCOMPATIBLE_TENSOR_INDEX,
                            span,
                        );
                        return Some(Err(()));
                    }
                    let actual_static = radix_types::evaluate_index_dimension(actual, self.types)
                        .ok()
                        .flatten();
                    let expected_static =
                        radix_types::evaluate_index_dimension(expected, self.types)
                            .ok()
                            .flatten();
                    if let (Some(left), Some(right)) = (actual_static, expected_static) {
                        if left != right {
                            self.error_issue(
                                SemanticErrorKind::TypeMismatch,
                                ISSUE_INCOMPATIBLE_TENSOR_INDEX,
                                span,
                            );
                            return Some(Err(()));
                        }
                        unified_dims.push(self.types.intern_index(IndexExpr::Literal(left)));
                        continue;
                    }

                    let Some((runtime_actual, _)) = self.runtime_boundary_axis(axis as u32) else {
                        return None;
                    };
                    self.record_shape_relation(runtime_actual, expected, span, boundary);
                    unified_dims.push(runtime_actual);
                }
            }
        }
        let unified = if matches!(self.types.get_index(actual_shape), IndexExpr::Tuple(_)) {
            self.types.intern_index(IndexExpr::Tuple(unified_dims))
        } else {
            unified_dims.first().copied().unwrap_or(actual_shape)
        };
        Some(Ok(unified))
    }

    fn shape_dimensions(shape: IndexId, types: &TypeTable) -> Option<Vec<IndexId>> {
        match types.get_index(shape) {
            IndexExpr::Tuple(dims) => Some(dims.clone()),
            IndexExpr::Literal(_)
            | IndexExpr::Param(_)
            | IndexExpr::Runtime(_)
            | IndexExpr::Affine { .. }
            | IndexExpr::Divide { .. } => Some(vec![shape]),
            // A root inference hole has unknown rank. Let ordinary inference
            // bind it; never manufacture a rank from a scalar assumption.
            IndexExpr::Infer(_) | IndexExpr::Unspecified => None,
        }
    }

    pub(super) fn runtime_substitute_shape_params(&mut self, index: IndexId) -> IndexId {
        match self.types.get_index(index).clone() {
            IndexExpr::Param(symbol) => self
                .current_shape_param_sources
                .get(&symbol)
                .copied()
                .unwrap_or(index),
            IndexExpr::Affine { base, offset } => {
                let base = self.runtime_substitute_shape_params(base);
                radix_types::add_index_constant(self.types, base, offset).unwrap_or(index)
            }
            IndexExpr::Divide { numerator, divisor } => {
                let numerator = self.runtime_substitute_shape_params(numerator);
                radix_types::divide_index_exact(self.types, numerator, divisor).unwrap_or(index)
            }
            _ => index,
        }
    }

    /// Install the size witnesses carried by a function's formal tensor axes,
    /// then retain repeated-symbol/affine consistency checks at entry.
    pub(crate) fn install_function_shape_sources(
        &mut self,
        type_params: &[radix_hir::HirTypeParam],
        params: &[HirParam],
    ) {
        let mut shapes = Vec::new();
        for (parameter, param) in params.iter().enumerate() {
            let mut fields = Vec::new();
            let mut seen = FxHashSet::default();
            self.collect_tensor_shapes(
                u32::try_from(parameter).unwrap_or(u32::MAX),
                param.ty,
                &mut fields,
                &mut seen,
                &mut shapes,
                param.span,
            );
        }

        // A direct dimension `N` is a usable witness for the generic symbol N.
        // Derived dimensions such as `N - 1` constrain an axis but cannot by
        // themselves recover N.
        for (parameter, fields, shape, span) in &shapes {
            let Some(dims) = Self::shape_dimensions(*shape, self.types) else {
                continue;
            };
            for (axis, dim) in dims.into_iter().enumerate() {
                let (symbol, offset) = match self.types.get_index(dim).clone() {
                    IndexExpr::Param(symbol) => (symbol, 0),
                    IndexExpr::Affine { base, offset } => match self.types.get_index(base) {
                        IndexExpr::Param(symbol) => (*symbol, offset),
                        _ => continue,
                    },
                    _ => continue,
                };
                let Some(ordinal) = type_params.iter().position(|param| {
                    param.name == symbol && param.kind == radix_syntax::GenericParamKind::Magnitudo
                }) else {
                    continue;
                };
                if self.current_shape_param_sources.contains_key(&symbol) {
                    continue;
                }
                let axis_source = runtime_formal_axis(*parameter, fields, axis as u32);
                let axis_witness = self.types.intern_index(IndexExpr::Runtime(axis_source));
                let witness = if offset == 0 {
                    axis_witness
                } else {
                    let Some(inverse) = offset.checked_neg() else {
                        continue;
                    };
                    let Ok(witness) =
                        radix_types::add_index_constant(self.types, axis_witness, inverse)
                    else {
                        continue;
                    };
                    witness
                };
                self.current_shape_param_sources.insert(symbol, witness);
                self.record_function_size_binding(
                    u32::try_from(ordinal).unwrap_or(u32::MAX),
                    witness,
                    *span,
                );
            }
        }

        for (parameter, fields, shape, span) in shapes {
            let Some(dims) = Self::shape_dimensions(shape, self.types) else {
                continue;
            };
            for (axis, dim) in dims.into_iter().enumerate() {
                let expected = self.runtime_substitute_shape_params(dim);
                let actual = self
                    .types
                    .intern_index(IndexExpr::Runtime(runtime_formal_axis(
                        parameter,
                        &fields,
                        axis as u32,
                    )));
                if !self.types.index_equals(actual, expected) {
                    self.record_shape_relation(
                        actual,
                        expected,
                        span,
                        ShapeRelationBoundary::FunctionEntry,
                    );
                }
            }
        }
    }

    fn record_function_size_binding(&mut self, parameter: u32, witness: IndexId, span: Span) {
        let Some(def_id) = self.current_function_def else {
            return;
        };
        let binding = RuntimeSizeBinding {
            parameter,
            witness,
            span,
        };
        let rows = self
            .shape_obligations
            .function_size_bindings
            .entry(def_id)
            .or_default();
        if !rows.iter().any(|row| row.parameter == parameter) {
            rows.push(binding);
        }
    }

    /// Preserve caller-provided runtime dimensions without placing their
    /// source identities in a generic instance key.
    pub(crate) fn capture_runtime_call_size_bindings(
        &mut self,
        params: &[radix_types::GenericParamDescriptor],
        shape_vars: &FxHashMap<Symbol, IndexId>,
        span: Span,
    ) -> FxHashSet<Symbol> {
        let Some(call) = self.current_shape_call else {
            return FxHashSet::default();
        };
        let mut runtime_symbols = FxHashSet::default();
        for (ordinal, param) in params.iter().enumerate() {
            if param.sort != GenericParamSort::Magnitudo {
                continue;
            }
            let Some(witness) = shape_vars.get(&param.name).copied() else {
                continue;
            };
            let witness = self.resolve_index(witness);
            if !self.index_has_runtime_extent(witness) {
                continue;
            }
            runtime_symbols.insert(param.name);
            let binding = RuntimeSizeBinding {
                parameter: u32::try_from(ordinal).unwrap_or(u32::MAX),
                witness,
                span,
            };
            let rows = self
                .shape_obligations
                .call_size_bindings
                .entry(call)
                .or_default();
            if !rows.iter().any(|row| row.parameter == binding.parameter) {
                rows.push(binding);
            }
        }
        runtime_symbols
    }

    /// Whether a dimension expression contains a runtime-origin leaf.
    pub(crate) fn index_has_runtime_extent(&self, index: IndexId) -> bool {
        fn visit(index: IndexId, types: &TypeTable, seen: &mut FxHashSet<IndexId>) -> bool {
            if !seen.insert(index) {
                return false;
            }
            match types.get_index(index) {
                IndexExpr::Runtime(_) => true,
                IndexExpr::Affine { base, .. } => visit(*base, types, seen),
                IndexExpr::Divide { numerator, .. } => visit(*numerator, types, seen),
                IndexExpr::Tuple(dims) => dims.iter().any(|dim| visit(*dim, types, seen)),
                _ => false,
            }
        }
        visit(index, self.types, &mut FxHashSet::default())
    }

    fn collect_tensor_shapes(
        &mut self,
        parameter: u32,
        ty: TypeId,
        fields: &mut Vec<Symbol>,
        seen: &mut FxHashSet<TypeId>,
        out: &mut Vec<(u32, Vec<Symbol>, IndexId, Span)>,
        span: Span,
    ) {
        let ty = self.resolve_inferred_type(ty);
        if !seen.insert(ty) {
            return;
        }
        match self.types.get(ty).clone() {
            Type::Tensor(_, shape)
            | Type::Matrix(_, shape)
            | Type::Sparsa(_, shape)
            | Type::Vector(_, shape) => {
                out.push((parameter, fields.clone(), shape, span));
            }
            Type::Record(record_fields) => {
                for (name, field_ty) in record_fields {
                    fields.push(name);
                    self.collect_tensor_shapes(parameter, field_ty, fields, seen, out, span);
                    fields.pop();
                }
            }
            Type::Applied(base, _) => {
                let base = self.resolve_inferred_type(base);
                if let Type::Struct(def_id) = self.types.get(base).clone() {
                    let field_types = self
                        .structs
                        .get(&def_id)
                        .map(|info| {
                            info.fields
                                .iter()
                                .map(|(name, field)| (*name, field.ty))
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    for (name, field_ty) in field_types {
                        let field_ty = self.applied_field_type(ty, def_id, field_ty);
                        fields.push(name);
                        self.collect_tensor_shapes(parameter, field_ty, fields, seen, out, span);
                        fields.pop();
                    }
                }
            }
            Type::Alias(_, inner) => {
                self.collect_tensor_shapes(parameter, inner, fields, seen, out, span);
            }
            Type::Ref(_, inner) => {
                self.collect_tensor_shapes(parameter, inner, fields, seen, out, span);
            }
            _ => {}
        }
        seen.remove(&ty);
    }

    fn index_has_immutable_scalar_witness(&self, index: IndexId) -> bool {
        match self.types.get_index(index) {
            IndexExpr::Runtime(RuntimeExtentId::ScalarBinding(def_id)) => {
                self.lookup_binding(*def_id).is_some_and(|binding| {
                    !binding.mutable
                        && crate::numeric_width::is_integer_index_scalar(
                            self.types,
                            self.resolve_inferred_type(binding.ty),
                        )
                })
            }
            IndexExpr::Affine { base, .. } => self.index_has_immutable_scalar_witness(*base),
            IndexExpr::Divide { numerator, .. } => {
                self.index_has_immutable_scalar_witness(*numerator)
            }
            _ => false,
        }
    }

    pub fn fresh_index_hole(&mut self) -> IndexId {
        self.index_infer.fresh_hole(self.types)
    }

    pub fn resolve_index(&mut self, id: IndexId) -> IndexId {
        self.index_infer.resolve(self.types, id)
    }

    pub fn canonicalize_index(&mut self, id: IndexId) -> IndexId {
        self.index_infer.canonicalize(self.types, id)
    }

    pub fn is_unresolved_index_hole(&mut self, id: IndexId) -> bool {
        self.index_infer.is_unresolved_hole(self.types, id)
    }

    pub fn index_contains_unresolved_hole(&mut self, id: IndexId) -> bool {
        self.index_infer.contains_unresolved_hole(self.types, id)
    }

    pub fn solve_index_constraints(&mut self, constraints: &[IndexConstraint]) -> bool {
        self.index_infer.solve_constraints(self.types, constraints)
    }

    pub fn constrain_index_equal(&mut self, a: IndexId, b: IndexId, span: Span) {
        if !self.solve_index_constraints(&[IndexConstraint::Equal(a, b)]) {
            self.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_INCOMPATIBLE_TENSOR_INDEX,
                span,
            );
        }
    }

    pub fn constrain_index_broadcast(
        &mut self,
        a: IndexId,
        b: IndexId,
        span: Span,
    ) -> Option<IndexId> {
        let mut unifier = self.index_infer.unifier(self.types);
        if let Ok(result) = unifier.unify_broadcast(a, b) {
            Some(result)
        } else {
            self.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_INCOMPATIBLE_TENSOR_INDEX,
                span,
            );
            None
        }
    }

    pub fn tensor_shape_has_unresolved_hole(&mut self, ty: TypeId) -> bool {
        let resolved = self.resolve_inferred_type(ty);
        let capacity = match self.types.get(resolved) {
            Type::Tensor(_, shape) => *shape,
            Type::BoundedAscii { capacity }
            | Type::BoundedTextus { capacity }
            | Type::BoundedOcteti { capacity } => *capacity,
            Type::BoundedArray { capacity, .. } => *capacity,
            _ => return false,
        };
        let capacity = self.resolve_index(capacity);
        if matches!(self.types.get_index(capacity), IndexExpr::Unspecified) {
            return true;
        }
        self.index_contains_unresolved_hole(capacity)
    }
}

fn runtime_formal_axis(parameter: u32, fields: &[Symbol], axis: u32) -> RuntimeExtentId {
    if fields.is_empty() {
        RuntimeExtentId::FunctionParameterAxis { parameter, axis }
    } else {
        RuntimeExtentId::FunctionParameterFieldAxis {
            parameter,
            fields: fields.to_vec(),
            axis,
        }
    }
}
