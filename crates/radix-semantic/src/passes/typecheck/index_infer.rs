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

/// Size-type Z3: a value parameter's name in a signature param/return figura
/// is a remote binding — the evidence crosses through the value slot, which
/// the reader cannot see from the signature. The did-you-mean points at the
/// size-typed generic slot (`<size N>`) or a spelled `size` lift.
pub(super) const ISSUE_SIGNATURE_EXTENT_BARE_VALUE_NAME: &str = "signature_extent_bare_value_name";

impl TypeChecker<'_> {
    /// Whether an index term is a value shape: a whole-figura `list<int>`
    /// value name (`tensor<f32, shape>` where `shape` binds a list), lowered
    /// as `Runtime(ScalarBinding)` exactly like a scalar extent. Only the
    /// binding's list type distinguishes the two readings.
    pub(crate) fn extent_binding_is_value_shape(&self, index: IndexId) -> bool {
        let index = match self.types.get_index(index) {
            IndexExpr::Tuple(dims) => match dims.as_slice() {
                [only] => *only,
                _ => index,
            },
            _ => index,
        };
        let IndexExpr::Runtime(RuntimeExtentId::ScalarBinding(def_id)) =
            self.types.get_index(index)
        else {
            return false;
        };
        self.lookup_binding(*def_id).is_some_and(|binding| {
            crate::numeric_width::is_integer_index_vector(
                self.types,
                self.resolve_inferred_type(binding.ty),
            )
        })
    }

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
            ShapeRelationBoundary::Store => self
                .current_store_expr
                .map(|expr| self.shape_obligations.stores.entry(expr).or_default()),
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
        } else if let Some(expression) = self.current_store_expr {
            let source = RuntimeExtentId::ExpressionAxis {
                expression: expression.0,
                axis,
            };
            Some((source, ShapeRelationBoundary::Store))
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
        } else if self.current_store_expr.is_some() {
            ShapeRelationBoundary::Store
        } else {
            return None;
        };
        let actual_shape = self.resolve_index(actual_shape);
        let expected_shape = self.resolve_index(expected_shape);
        // A whole-shape generic parameter (`size Shape` in `tensor<T, Shape>`)
        // is a fresh root hole at the call site: it has no rank to preserve
        // and nothing to relate at runtime, so the ordinary unifier binds it
        // to the actual shape.
        if matches!(self.types.get_index(expected_shape), IndexExpr::Infer(_)) {
            return None;
        }
        // An actual root hole (an intrinsic result such as
        // `seed.from_flat(...)`) is bound by the ordinary unifier to a
        // whole-shape expectation, which carries no rank either. Against a
        // ranked expectation it would have to manufacture a rank, so that
        // case (and an unspecified shape on either side) is reported.
        let actual_is_hole = matches!(self.types.get_index(actual_shape), IndexExpr::Infer(_));
        if actual_is_hole && !matches!(self.types.get_index(expected_shape), IndexExpr::Tuple(_)) {
            return None;
        }
        if actual_is_hole
            || matches!(self.types.get_index(actual_shape), IndexExpr::Unspecified)
            || matches!(self.types.get_index(expected_shape), IndexExpr::Unspecified)
        {
            // A tuple with per-axis Infer terms remains admissible below
            // because its rank is known.
            self.missing_type_annotation("cannot_infer_tensor_shape", span);
            return Some(Err(()));
        }

        let actual_dims = Self::shape_dimensions(actual_shape, self.types)?;
        let expected_dims = Self::shape_dimensions(expected_shape, self.types)?;
        if actual_dims.len() != expected_dims.len() {
            self.incompatible_tensor_index(span);
            return Some(Err(()));
        }

        let unified_dims =
            match self.unify_shape_dimensions(boundary, span, actual_dims, expected_dims) {
                Some(Ok(dims)) => dims,
                Some(Err(())) => return Some(Err(())),
                None => return None,
            };
        let unified = if matches!(self.types.get_index(actual_shape), IndexExpr::Tuple(_)) {
            self.types.intern_index(IndexExpr::Tuple(unified_dims))
        } else {
            unified_dims.first().copied().unwrap_or(actual_shape)
        };
        Some(Ok(unified))
    }

    /// Report a concrete tensor-shape contradiction at `span`.
    fn incompatible_tensor_index(&mut self, span: Span) {
        self.error_issue(
            SemanticErrorKind::TypeMismatch,
            ISSUE_INCOMPATIBLE_TENSOR_INDEX,
            span,
        );
    }

    /// Unify a boundary call's or return's shape one axis at a time, retaining
    /// recoverable runtime relations and rejecting concrete contradictions.
    /// `None` means no runtime boundary was active for an axis read.
    fn unify_shape_dimensions(
        &mut self,
        boundary: ShapeRelationBoundary,
        span: Span,
        actual_dims: Vec<IndexId>,
        expected_dims: Vec<IndexId>,
    ) -> Option<Result<Vec<IndexId>, ()>> {
        let mut unified_dims = Vec::with_capacity(actual_dims.len());
        for (axis, (actual, expected)) in actual_dims.into_iter().zip(expected_dims).enumerate() {
            let Ok(axis) = u32::try_from(axis) else {
                self.incompatible_tensor_index(span);
                return Some(Err(()));
            };
            let mut actual = self.resolve_index(actual);
            // The same compile-time dimension term on both sides (a
            // function's own `N` returned or passed as `N`) is equal by
            // construction. It needs no runtime relation, and it keeps its
            // symbolic identity so the value's type monomorphizes exactly like
            // the declared type. Substituting the formal witness first would
            // rebase it to the carrier's runtime axis and split it from the
            // declared type. A runtime extent still guards its carrier below.
            let declared = self.resolve_index(expected);
            if !self.index_has_runtime_extent(actual) && self.types.index_equals(actual, declared) {
                unified_dims.push(actual);
                continue;
            }
            let expected = self.runtime_substitute_shape_params(expected);
            // A symbolic shape written on an actual tensor describes its
            // carrier, but the runtime check must read the carrier passed at
            // this boundary. Rebase dynamic dimensions to the call argument
            // (or returned expression) axis before unification so inferred
            // generic sizes and retained equations have a concrete value
            // source even when the tensor was constructed from a scalar
            // local or an imported generic parameter. The caller's own rigid
            // size parameters are not dynamic: they are the symbolic names
            // the caller's annotations are written in, so they keep their
            // identity instead of becoming the argument's runtime axis.
            let immutable_scalar_binding = boundary == ShapeRelationBoundary::Call
                && self.index_has_immutable_scalar_witness(actual);
            if radix_types::evaluate_index_dimension(actual, self.types)
                .ok()
                .flatten()
                .is_none()
                && !matches!(
                    self.types.get_index(actual),
                    IndexExpr::Infer(_) | IndexExpr::Param(_)
                )
            {
                if immutable_scalar_binding {
                    // Keep a stable immutable scalar witness for generic
                    // shape inference, but still guard the physical carrier
                    // axis against it for every argument at this call.
                    if let Some((runtime_axis, boundary)) = self.runtime_boundary_axis(axis) {
                        self.record_shape_relation(runtime_axis, actual, span, boundary);
                    }
                } else if let Some((runtime_axis, _)) = self.runtime_boundary_axis(axis) {
                    actual = runtime_axis;
                }
            }
            let unified = {
                let mut unifier = self.index_infer.unifier(self.types);
                unifier.unify(actual, expected)
            };
            if let Ok(index) = unified {
                unified_dims.push(index);
            } else {
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
                    self.incompatible_tensor_index(span);
                    return Some(Err(()));
                }
                let actual_static = radix_types::evaluate_index_dimension(actual, self.types)
                    .ok()
                    .flatten();
                let expected_static = radix_types::evaluate_index_dimension(expected, self.types)
                    .ok()
                    .flatten();
                if let (Some(left), Some(right)) = (actual_static, expected_static) {
                    if left != right {
                        self.incompatible_tensor_index(span);
                        return Some(Err(()));
                    }
                    unified_dims.push(self.types.intern_index(IndexExpr::Literal(left)));
                    continue;
                }

                let (runtime_actual, _) = self.runtime_boundary_axis(axis)?;
                // A value-shape extent (a whole-figura `list<int>` value
                // name) has no scalar read: the producing conversion's
                // runtime count check owns the fact, so no axis equation
                // is retained against it.
                if !self.extent_binding_is_value_shape(expected) {
                    self.record_shape_relation(runtime_actual, expected, span, boundary);
                }
                unified_dims.push(runtime_actual);
            }
        }
        Some(Ok(unified_dims))
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
            IndexExpr::Param(symbol) => {
                // A formal ordinal at a call boundary denotes the callee's
                // argument, not this function's parameter. Caller size symbols
                // must retain their carrier's binding identity instead.
                let sources = if self.current_shape_call.is_some() {
                    &self.current_shape_param_call_sources
                } else {
                    &self.current_shape_param_sources
                };
                sources.get(&symbol).copied().unwrap_or(index)
            }
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

    /// Size-type Z3: refuse the value-parameter names a signature's
    /// param/return figuras bind remotely, with a did-you-mean naming each.
    pub(crate) fn reject_signature_value_name_extents(
        &mut self,
        ty: TypeId,
        value_param_names: &[radix_lexer::Symbol],
        span: Span,
    ) {
        let mut ordinals = FxHashSet::default();
        collect_value_parameter_extents(
            ty,
            self.types,
            &mut FxHashSet::default(),
            &mut FxHashSet::default(),
            &mut ordinals,
        );
        for ordinal in ordinals {
            let Some(symbol) = value_param_names.get(ordinal as usize).copied() else {
                continue;
            };
            let name = self.interner.map_or_else(
                || "n".to_owned(),
                |interner| interner.resolve(symbol).to_owned(),
            );
            self.invalid_operand_types_with_args(
                ISSUE_SIGNATURE_EXTENT_BARE_VALUE_NAME,
                span,
                &[("param", name)],
            );
        }
    }

    /// Install the size witnesses carried by a function's formal tensor axes,
    /// then retain repeated-symbol/affine consistency checks at entry.
    ///
    /// Entry checks are only for host-callable functions whose extents are not
    /// statically known. A statically known extent is a compile-time fact
    /// proven at every typed call site, and a device function (kernel or
    /// nucleum fragment) has no runtime shape-check call to emit: its extents
    /// are checked at the typed call site and the host launch boundary.
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
        self.install_shape_symbol_witnesses(type_params, params, &shapes);

        for (parameter, fields, shape, span) in shapes {
            let Some(dims) = Self::shape_dimensions(shape, self.types) else {
                continue;
            };
            for (axis, dim) in dims.into_iter().enumerate() {
                let Ok(axis) = u32::try_from(axis) else {
                    self.incompatible_tensor_index(span);
                    return;
                };
                let expected = self.runtime_substitute_shape_params(dim);
                let actual = self
                    .types
                    .intern_index(IndexExpr::Runtime(runtime_formal_axis(
                        parameter, &fields, axis,
                    )));
                // A statically known extent is a compile-time fact, and a
                // non-scalar term (a rank tuple passed as a size argument,
                // `Pondo<[D]>`) has no per-axis equation to check at runtime.
                // The same holds for a value-shape extent: a whole-figura
                // `list<int>` value name is read by the producing
                // conversion's runtime count check, never per axis.
                let no_entry_equation = matches!(
                    radix_types::evaluate_index_dimension(expected, self.types),
                    Ok(Some(_)) | Err(radix_types::IndexArithmeticError::NotScalar)
                ) || self.extent_binding_is_value_shape(expected);
                if !self.current_device_function
                    && !no_entry_equation
                    && !self.types.index_equals(actual, expected)
                {
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

    /// Install the generic-size witnesses a signature's formal tensor axes
    /// carry, so each call records its own relation against the callee's axis.
    fn install_shape_symbol_witnesses(
        &mut self,
        type_params: &[radix_hir::HirTypeParam],
        params: &[HirParam],
        shapes: &[(u32, Vec<Symbol>, IndexId, Span)],
    ) {
        for (parameter, fields, shape, span) in shapes {
            let Some(dims) = Self::shape_dimensions(*shape, self.types) else {
                continue;
            };
            for (axis, dim) in dims.into_iter().enumerate() {
                let Ok(axis) = u32::try_from(axis) else {
                    self.incompatible_tensor_index(*span);
                    return;
                };
                // Size-type Z3 (Q3): a `_` extent parameter is an anonymous
                // size chosen per call. At entry the hole falls through to
                // the carrier: the formal axis is the witness the body reads
                // (runtime-tensors L3), and each call records its own
                // relation against it.
                let resolved = self.resolve_index(dim);
                if let IndexExpr::Infer(var) = self.types.get_index(resolved) {
                    let var = *var;
                    let axis_witness =
                        self.types
                            .intern_index(IndexExpr::Runtime(runtime_formal_axis(
                                *parameter, fields, axis,
                            )));
                    self.index_infer.rebind(var, axis_witness);
                }
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
                let Some(inverse) = offset.checked_neg() else {
                    continue;
                };
                let axis_source = runtime_formal_axis(*parameter, fields, axis);
                let axis_witness = self.types.intern_index(IndexExpr::Runtime(axis_source));
                let Ok(witness) =
                    radix_types::add_index_constant(self.types, axis_witness, inverse)
                else {
                    continue;
                };
                let binding = params[*parameter as usize].def_id;
                let call_source = if fields.is_empty() {
                    RuntimeExtentId::TensorBindingAxis { binding, axis }
                } else {
                    RuntimeExtentId::TensorFieldAxis {
                        binding,
                        fields: fields.clone(),
                        axis,
                    }
                };
                let call_axis = self.types.intern_index(IndexExpr::Runtime(call_source));
                let Ok(call_witness) =
                    radix_types::add_index_constant(self.types, call_axis, inverse)
                else {
                    continue;
                };
                self.current_shape_param_sources.insert(symbol, witness);
                self.current_shape_param_call_sources
                    .insert(symbol, call_witness);
                self.record_function_size_binding(
                    u32::try_from(ordinal).unwrap_or(u32::MAX),
                    witness,
                    *span,
                );
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
            Type::Alias(_, inner) | Type::Ref(_, inner) => {
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
            | Type::BoundedOcteti { capacity }
            | Type::BoundedArray { capacity, .. } => *capacity,
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

/// Collect the value-parameter ordinals named by a declared signature type's
/// extent terms. `Runtime(ScalarParameter { parameter })` leaves exist only
/// where the signature's own lowering recorded a value parameter's name
/// (generic size params lower as `Param`, declared sizes as `ScalarBinding`),
/// so every hit is a remote value→size crossing (size-type Z3).
fn collect_value_parameter_extents(
    ty: TypeId,
    types: &TypeTable,
    seen_types: &mut FxHashSet<TypeId>,
    seen_indices: &mut FxHashSet<IndexId>,
    out: &mut FxHashSet<u32>,
) {
    if !seen_types.insert(ty) {
        return;
    }
    let check_type = |ty: TypeId,
                      seen_types: &mut FxHashSet<TypeId>,
                      seen_indices: &mut FxHashSet<IndexId>,
                      out: &mut FxHashSet<u32>| {
        collect_value_parameter_extents(ty, types, seen_types, seen_indices, out);
    };
    let check_index =
        |id: IndexId, seen_indices: &mut FxHashSet<IndexId>, out: &mut FxHashSet<u32>| {
            collect_value_parameter_extents_in_index(id, types, seen_indices, out);
        };

    match types.get(ty) {
        Type::Array(inner)
        | Type::Queue(inner)
        | Type::Stack(inner)
        | Type::Set(inner)
        | Type::Promissum(inner)
        | Type::Cursor(inner)
        | Type::Atomic(inner)
        | Type::Intervallum(inner, _)
        | Type::Option(inner)
        | Type::Ref(_, inner)
        | Type::Alias(_, inner) => check_type(*inner, seen_types, seen_indices, out),
        Type::BoundedArray { element, capacity }
        | Type::BoundedQueue { element, capacity }
        | Type::BoundedStack { element, capacity } => {
            check_index(*capacity, seen_indices, out);
            check_type(*element, seen_types, seen_indices, out);
        }
        Type::BoundedTextus { capacity }
        | Type::BoundedAscii { capacity }
        | Type::BoundedOcteti { capacity } => {
            check_index(*capacity, seen_indices, out);
        }
        Type::Map(key, value) | Type::Ledger(key, value) => {
            check_type(*key, seen_types, seen_indices, out);
            check_type(*value, seen_types, seen_indices, out);
        }
        Type::Record(fields) => {
            for field in fields.values() {
                check_type(*field, seen_types, seen_indices, out);
            }
        }
        Type::PromissumFailable(success, alternate) | Type::AsyncCursor(success, alternate) => {
            check_type(*success, seen_types, seen_indices, out);
            check_type(*alternate, seen_types, seen_indices, out);
        }
        Type::Tensor(element, shape)
        | Type::Vector(element, shape)
        | Type::Matrix(element, shape)
        | Type::Sparsa(element, shape) => {
            check_index(*shape, seen_indices, out);
            check_type(*element, seen_types, seen_indices, out);
        }
        Type::Func(signature) => {
            for param in &signature.params {
                check_type(param.ty, seen_types, seen_indices, out);
            }
            check_type(signature.ret, seen_types, seen_indices, out);
            if let Some(err) = signature.err {
                check_type(err, seen_types, seen_indices, out);
            }
        }
        Type::Applied(base, args) => {
            check_type(*base, seen_types, seen_indices, out);
            for arg in args {
                match arg {
                    radix_types::AppliedArg::Type(ty) => {
                        check_type(*ty, seen_types, seen_indices, out);
                    }
                    radix_types::AppliedArg::Size(index) => check_index(*index, seen_indices, out),
                }
            }
        }
        Type::Union(members) | Type::Intersection(members) | Type::Tuple(members) => {
            for member in members {
                check_type(*member, seen_types, seen_indices, out);
            }
        }
        _ => {}
    }
}

fn collect_value_parameter_extents_in_index(
    id: IndexId,
    types: &TypeTable,
    seen_indices: &mut FxHashSet<IndexId>,
    out: &mut FxHashSet<u32>,
) {
    if !seen_indices.insert(id) {
        return;
    }
    match types.get_index(id) {
        IndexExpr::Runtime(RuntimeExtentId::ScalarParameter { parameter }) => {
            out.insert(*parameter);
        }
        IndexExpr::Tuple(dims) => {
            for dim in dims {
                collect_value_parameter_extents_in_index(*dim, types, seen_indices, out);
            }
        }
        IndexExpr::Affine { base, .. } => {
            collect_value_parameter_extents_in_index(*base, types, seen_indices, out);
        }
        IndexExpr::Divide { numerator, .. } => {
            collect_value_parameter_extents_in_index(*numerator, types, seen_indices, out);
        }
        _ => {}
    }
}
