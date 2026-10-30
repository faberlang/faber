//! Runtime and intrinsic call lowering.
//!
//! This module is the MIR boundary for operations that are not plain local
//! function calls or primitive expressions. Provider imports, collection
//! methods, diagnostic output, string formatting, conversions, and panic all
//! lower to explicit MIR runtime/intrinsic calls. Ordinary direct calls stay as
//! definition calls, while enum variant calls are redirected to aggregate
//! construction.
//!
//! BOUNDARIES
//! ==========
//! - Indirect calls are not guessed into MIR callable values; they fail closed.
//! - Collection methods are recognized only through resolved receiver type,
//!   method name, and arity.
//! - Failable function calls require an active local `cape` handler so the MIR
//!   can encode both success and error CFG edges.
//! - Runtime calls with `vacuum` return type emit no destination; value-returning
//!   calls allocate a temp and return that temp as the expression operand.

#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for lowering
use super::*;
use radix_hir::HirCallArg;
use radix_hir::visit::{HirVisitor, walk_expr};
use radix_mir::lane_op::stored_lane;
use radix_semantic::IndexExpr;
use radix_semantic::lista_tensor_conversio::{
    ListaToTensorShape, analyze_rectangular_lista_literal, lista_numerus_type, padded_literal_facts,
    resolve_bounded_lista_padded_shape, resolve_lista_to_tensor_shape, validate_lista_to_tensor_source,
};

pub fn shape_relation_applies_to_return(types: &TypeTable, relation: &ShapeRelation, expression: u32) -> bool {
    fn mentions_expression(types: &TypeTable, index: IndexId, expression: u32) -> bool {
        match types.get_index(index) {
            IndexExpr::Runtime(
                RuntimeExtentId::ScalarExpression { expression: source }
                | RuntimeExtentId::ExpressionAxis { expression: source, .. }
                | RuntimeExtentId::ExpressionFieldAxis { expression: source, .. },
            ) => *source == expression,
            IndexExpr::Affine { base, .. } => mentions_expression(types, *base, expression),
            IndexExpr::Divide { numerator, .. } => mentions_expression(types, *numerator, expression),
            IndexExpr::Tuple(dimensions) => dimensions
                .iter()
                .any(|dimension| mentions_expression(types, *dimension, expression)),
            _ => false,
        }
    }

    fn has_expression_source(types: &TypeTable, index: IndexId) -> bool {
        match types.get_index(index) {
            IndexExpr::Runtime(
                RuntimeExtentId::ScalarExpression { .. }
                | RuntimeExtentId::ExpressionAxis { .. }
                | RuntimeExtentId::ExpressionFieldAxis { .. },
            ) => true,
            IndexExpr::Affine { base, .. } => has_expression_source(types, *base),
            IndexExpr::Divide { numerator, .. } => has_expression_source(types, *numerator),
            IndexExpr::Tuple(dimensions) => dimensions
                .iter()
                .any(|dimension| has_expression_source(types, *dimension)),
            _ => false,
        }
    }

    mentions_expression(types, relation.left, expression)
        || mentions_expression(types, relation.right, expression)
        || (!has_expression_source(types, relation.left) && !has_expression_source(types, relation.right))
}

/// Signature of a synthesized check helper: two call sites whose helpers
/// would be byte-identical share one function instead of minting a copy
/// per site. The body depends only on these values (context, device role,
/// and the type table are fixed for the whole lowering).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) enum SyntheticHelperKey {
    CheckedAt {
        keyed: bool,
        params: [MirType; 2],
        return_ty: MirType,
        error_ty: MirType,
    },
    CheckedTensor {
        op: MirCollectionOp,
        params: Vec<MirType>,
        return_ty: MirType,
        error_ty: MirType,
    },
    Conversio {
        params: Vec<MirType>,
        target: TypeId,
        conversion_params: Vec<Symbol>,
        return_ty: MirType,
        error_ty: MirType,
    },
    BoundedAppend {
        receiver_ty: MirType,
        item_ty: MirType,
        return_ty: MirType,
        error_ty: MirType,
    },
    /// A length-ranged list push or removal: `Append` past the
    /// maximum or `RemoveFirst` / `RemoveLast` below the minimum, checked
    /// against `limit` before the op runs.
    RangedList {
        op: MirCollectionOp,
        receiver_ty: MirType,
        item_ty: Option<MirType>,
        limit: u64,
        return_ty: MirType,
        error_ty: MirType,
    },
    /// A `lista ↦ tensor` count check run inside a handler: `TensorFromFlat`
    /// over a flat list and its shape operand.
    TensorFromFlat {
        flat_ty: MirType,
        shape_ty: MirType,
        return_ty: MirType,
        error_ty: MirType,
    },
    /// A checked list trim: `Slice` (the window) or `Take` (the prefix), with
    /// the receiver and index parameter types.
    CheckedTrim {
        op: MirCollectionOp,
        params: Vec<MirType>,
        return_ty: MirType,
        error_ty: MirType,
    },
}

/// Whether a `⊥` default expression is side-effecting: a call or method call
/// anywhere inside it. Pure recovery expressions (literals, paths, call-free
/// compound values) need no deferral — evaluating them eagerly is
/// side-effect-free (CTO-3 recovery-eval fix).
fn recovery_contains_call(recovery: &HirExpression) -> bool {
    struct CallScan {
        found: bool,
    }
    impl HirVisitor for CallScan {
        fn visit_expr(&mut self, expr: &HirExpression) {
            if matches!(&expr.kind, HirExpressionKind::Call(..) | HirExpressionKind::MethodCall(..)) {
                self.found = true;
            }
            walk_expr(self, expr);
        }
    }
    let mut scan = CallScan { found: false };
    scan.visit_expr(recovery);
    scan.found
}

struct HandledConversio<'a> {
    source: MirOperand,
    /// The run-time shape of an `octeti ↦ tensor` target, as the conversion's
    /// second operand (see [`FunctionBuilder::lower_octeti_tensor_shape`]).
    shape: Option<(MirOperand, MirType)>,
    source_ty: MirType,
    target: TypeId,
    params: &'a [Symbol],
    return_ty: MirType,
    handler: HandlerContext,
    span: Span,
}

impl FunctionBuilder<'_> {
    pub(super) fn emit_call_shape_check(
        &mut self,
        site: radix_hir::HirId,
        arguments: &[MirOperand],
        callee_parameter_offset: u32,
        span: Span,
    ) -> Option<()> {
        let Some(relations) = self.context.shape_calls.get(&site).cloned() else {
            let bindings = self
                .context
                .call_size_bindings
                .get(&site)
                .cloned()
                .unwrap_or_default();
            if bindings.is_empty() {
                return Some(());
            }
            return self.emit_shape_check(
                &[],
                &bindings,
                ShapeRelationBoundary::Call,
                arguments,
                None,
                callee_parameter_offset,
                span,
            );
        };
        let bindings = self
            .context
            .call_size_bindings
            .get(&site)
            .cloned()
            .unwrap_or_default();
        self.emit_shape_check(
            &relations,
            &bindings,
            ShapeRelationBoundary::Call,
            arguments,
            None,
            callee_parameter_offset,
            span,
        )
    }

    pub(super) fn emit_store_shape_check(
        &mut self,
        expression: radix_hir::HirId,
        value: MirOperand,
        span: Span,
    ) -> Option<()> {
        let relations = self
            .context
            .shape_stores
            .get(&expression)
            .cloned()
            .unwrap_or_default();
        if relations.is_empty() {
            return Some(());
        }
        self.emit_shape_check(
            &relations,
            &[],
            ShapeRelationBoundary::Store,
            &[],
            Some(&(expression.0, value)),
            0,
            span,
        )
    }

    pub(super) fn emit_function_entry_shape_check(&mut self, span: Span) -> Option<()> {
        let relations = self.function_shape_relations.clone();
        self.emit_shape_check(&relations, &[], ShapeRelationBoundary::FunctionEntry, &[], None, 0, span)
    }

    pub(super) fn emit_function_return_shape_check(
        &mut self,
        expression: u32,
        value: MirOperand,
        span: Span,
    ) -> Option<()> {
        let relations = self.function_shape_relations.clone();
        let expression_value = (expression, value);
        self.emit_shape_check(
            &relations,
            &[],
            ShapeRelationBoundary::FunctionReturn,
            &[],
            Some(&expression_value),
            0,
            span,
        )
    }

    fn emit_shape_check(
        &mut self,
        relations: &[ShapeRelation],
        size_bindings: &[RuntimeSizeBinding],
        boundary: ShapeRelationBoundary,
        arguments: &[MirOperand],
        expression_value: Option<&(u32, MirOperand)>,
        callee_parameter_offset: u32,
        span: Span,
    ) -> Option<()> {
        let retained: Vec<_> = relations
            .iter()
            .filter(|relation| relation.boundary == boundary)
            .filter(|relation| {
                boundary != ShapeRelationBoundary::FunctionReturn
                    || expression_value.is_none_or(|(expression, _)| {
                        shape_relation_applies_to_return(self.types, relation, *expression)
                    })
            })
            .copied()
            .collect();
        if retained.is_empty() && size_bindings.is_empty() {
            return Some(());
        }
        let mut check_arguments = arguments.to_vec();
        let mut lowered = Vec::with_capacity(retained.len());
        let mut lowered_bindings = Vec::with_capacity(size_bindings.len());
        for binding in size_bindings {
            let witness = match self.lower_shape_expr(
                binding.witness,
                &mut check_arguments,
                expression_value,
                boundary,
                callee_parameter_offset,
            ) {
                Ok(expression) => expression,
                Err(message) => {
                    self.errors
                        .push(MirError::unsupported(binding.span, message));
                    return None;
                }
            };
            lowered_bindings.push(MirShapeBinding { parameter: binding.parameter, witness });
        }
        for relation in &retained {
            let left = match self.lower_shape_expr(
                relation.left,
                &mut check_arguments,
                expression_value,
                boundary,
                callee_parameter_offset,
            ) {
                Ok(expression) => expression,
                Err(message) => {
                    self.errors
                        .push(MirError::unsupported(relation.span, message));
                    return None;
                }
            };
            let right = match self.lower_shape_expr(
                relation.right,
                &mut check_arguments,
                expression_value,
                boundary,
                callee_parameter_offset,
            ) {
                Ok(expression) => expression,
                Err(message) => {
                    self.errors
                        .push(MirError::unsupported(relation.span, message));
                    return None;
                }
            };
            lowered.push(MirShapeRelation { left, right });
        }
        let check_span = retained.first().map_or(span, |relation| relation.span);
        self.runtime_call_to_place(
            None,
            MirIntrinsic::TensorShapeCheck(MirShapeCheck { relations: lowered, size_bindings: lowered_bindings }),
            check_arguments,
            MirType::semantic(self.types.primitive(Primitive::Vacuum)),
            check_span,
        );
        Some(())
    }

    pub(super) fn lower_shape_expr(
        &mut self,
        index: IndexId,
        arguments: &mut Vec<MirOperand>,
        expression_value: Option<&(u32, MirOperand)>,
        boundary: ShapeRelationBoundary,
        callee_parameter_offset: u32,
    ) -> Result<MirShapeExpr, String> {
        match self.types.get_index(index).clone() {
            IndexExpr::Literal(value) => Ok(MirShapeExpr::Literal(value)),
            IndexExpr::Runtime(source) => {
                self.lower_runtime_shape_source(source, arguments, expression_value, boundary, callee_parameter_offset)
            }
            IndexExpr::Affine { base, offset } => Ok(MirShapeExpr::Affine {
                base: Box::new(self.lower_shape_expr(
                    base,
                    arguments,
                    expression_value,
                    boundary,
                    callee_parameter_offset,
                )?),
                offset,
            }),
            IndexExpr::Divide { numerator, divisor } if divisor != 0 => Ok(MirShapeExpr::Divide {
                numerator: Box::new(self.lower_shape_expr(
                    numerator,
                    arguments,
                    expression_value,
                    boundary,
                    callee_parameter_offset,
                )?),
                divisor,
            }),
            IndexExpr::Divide { .. } => Err("runtime shape division has a zero divisor".to_owned()),
            IndexExpr::Param(symbol) => {
                if let Some(source) = self.size_param_source(symbol) {
                    return self.lower_shape_expr(
                        source,
                        arguments,
                        expression_value,
                        boundary,
                        callee_parameter_offset,
                    );
                }

                let known_size_parameter = self.size_params.iter().any(|(_, name)| *name == symbol);
                if !known_size_parameter {
                    return Err("runtime shape relation retains an unbound generic dimension".to_owned());
                }

                // A formal tensor axis can witness a direct shape parameter,
                // but not the inverse of an affine expression such as D / 2.
                // Preserve D as the existing compiler-owned placeholder so
                // generic monomorphization can substitute a concrete call's
                // size argument before its survivor check.
                let operand = MirOperand::Constant(MirConstant::ShapeSize { shape: index, axis: None });
                let argument = if let Some(position) = arguments.iter().position(|existing| existing == &operand) {
                    u32::try_from(position).map_err(|_| "runtime shape check has too many operands".to_owned())?
                } else {
                    let position = u32::try_from(arguments.len())
                        .map_err(|_| "runtime shape check has too many operands".to_owned())?;
                    arguments.push(operand);
                    position
                };
                Ok(MirShapeExpr::ScalarArgument { argument })
            }
            IndexExpr::Tuple(_) => Err("runtime shape relation expected a scalar dimension".to_owned()),
            IndexExpr::Infer(_) | IndexExpr::Unspecified => {
                Err("runtime shape relation retains an unresolved dimension".to_owned())
            }
        }
    }

    fn lower_runtime_shape_source(
        &mut self,
        source: RuntimeExtentId,
        arguments: &mut Vec<MirOperand>,
        expression_value: Option<&(u32, MirOperand)>,
        boundary: ShapeRelationBoundary,
        callee_parameter_offset: u32,
    ) -> Result<MirShapeExpr, String> {
        let formal_operand = |this: &Self, parameter: u32| -> Result<MirOperand, String> {
            let ordinal = parameter
                .checked_add(this.formal_parameter_offset)
                .ok_or_else(|| "runtime shape formal parameter ordinal overflow".to_owned())?;
            let param = this
                .params
                .get(
                    usize::try_from(ordinal)
                        .map_err(|_| "runtime shape formal ordinal is not addressable".to_owned())?,
                )
                .ok_or_else(|| "runtime shape formal parameter is out of range".to_owned())?;
            Ok(MirOperand::Place(MirPlace::local(param.local)))
        };
        let call_operand = |arguments: &[MirOperand], parameter: u32| -> Result<u32, String> {
            let ordinal = parameter
                .checked_add(callee_parameter_offset)
                .ok_or_else(|| "runtime shape call argument ordinal overflow".to_owned())?;
            if usize::try_from(ordinal).map_or(true, |index| index >= arguments.len()) {
                return Err("runtime shape call argument is out of range".to_owned());
            }
            Ok(ordinal)
        };
        let parameter_shape = |this: &Self,
                               arguments: &mut Vec<MirOperand>,
                               parameter: u32,
                               axis: Option<(Vec<Symbol>, u32)>|
         -> Result<MirShapeExpr, String> {
            if boundary == ShapeRelationBoundary::Call {
                let argument = call_operand(arguments, parameter)?;
                return Ok(match axis {
                    Some((fields, axis)) => MirShapeExpr::TensorFieldAxis { argument, fields, axis },
                    None => MirShapeExpr::ScalarArgument { argument },
                });
            }
            let argument = append_runtime_shape_argument(arguments, formal_operand(this, parameter)?)?;
            Ok(match axis {
                Some((fields, axis)) => MirShapeExpr::TensorFieldAxis { argument, fields, axis },
                None => MirShapeExpr::ScalarArgument { argument },
            })
        };
        match source {
            RuntimeExtentId::CallArgumentAxis { argument, axis } => {
                let argument = if boundary == ShapeRelationBoundary::Call {
                    argument
                        .checked_add(callee_parameter_offset)
                        .ok_or_else(|| "runtime shape call-argument ordinal overflow".to_owned())?
                } else {
                    argument
                };
                if usize::try_from(argument).map_or(true, |index| index >= arguments.len()) {
                    return Err("runtime shape call-argument axis is out of range".to_owned());
                }
                Ok(MirShapeExpr::TensorAxis { argument, axis })
            }
            RuntimeExtentId::CallArgumentFieldAxis { argument, fields, axis } => {
                let argument = if boundary == ShapeRelationBoundary::Call {
                    argument
                        .checked_add(callee_parameter_offset)
                        .ok_or_else(|| "runtime shape call-argument ordinal overflow".to_owned())?
                } else {
                    argument
                };
                if usize::try_from(argument).map_or(true, |index| index >= arguments.len()) {
                    return Err("runtime shape call-argument field axis is out of range".to_owned());
                }
                Ok(MirShapeExpr::TensorFieldAxis { argument, fields, axis })
            }
            RuntimeExtentId::ScalarParameter { parameter } => parameter_shape(self, arguments, parameter, None),
            RuntimeExtentId::FunctionParameterAxis { parameter, axis } => {
                if boundary == ShapeRelationBoundary::Call {
                    let argument = call_operand(arguments, parameter)?;
                    Ok(MirShapeExpr::TensorAxis { argument, axis })
                } else {
                    let argument = append_runtime_shape_argument(arguments, formal_operand(self, parameter)?)?;
                    Ok(MirShapeExpr::TensorAxis { argument, axis })
                }
            }
            RuntimeExtentId::FunctionParameterFieldAxis { parameter, fields, axis } => {
                parameter_shape(self, arguments, parameter, Some((fields, axis)))
            }
            other => self.lower_runtime_shape_leaf_source(other, arguments, expression_value),
        }
    }

    /// Resolve a shape extent source in the binding/expression families into a
    /// `MirShapeExpr`, appending its operand to `arguments`. The parameter and
    /// call-argument families are handled by the caller's `parameter_shape`.
    fn lower_runtime_shape_leaf_source(
        &self,
        source: RuntimeExtentId,
        arguments: &mut Vec<MirOperand>,
        expression_value: Option<&(u32, MirOperand)>,
    ) -> Result<MirShapeExpr, String> {
        match source {
            RuntimeExtentId::ScalarBinding(binding) => {
                let operand = self
                    .runtime_shape_binding_operand(binding)
                    .ok_or_else(|| "runtime scalar extent binding is unavailable in MIR".to_owned())?;
                let argument = append_runtime_shape_argument(arguments, operand)?;
                Ok(MirShapeExpr::ScalarArgument { argument })
            }
            RuntimeExtentId::TensorBindingAxis { binding, axis } => {
                let operand = self
                    .runtime_shape_binding_operand(binding)
                    .ok_or_else(|| "runtime tensor extent binding is unavailable in MIR".to_owned())?;
                let argument = append_runtime_shape_argument(arguments, operand)?;
                Ok(MirShapeExpr::TensorAxis { argument, axis })
            }
            RuntimeExtentId::TensorFieldAxis { binding, fields, axis } => {
                let operand = self
                    .runtime_shape_binding_operand(binding)
                    .ok_or_else(|| "runtime tensor-field extent binding is unavailable in MIR".to_owned())?;
                let argument = append_runtime_shape_argument(arguments, operand)?;
                Ok(MirShapeExpr::TensorFieldAxis { argument, fields, axis })
            }
            RuntimeExtentId::ScalarExpression { expression } => {
                let operand = expression_value
                    .filter(|(available, _)| *available == expression)
                    .map(|(_, operand)| operand.clone())
                    .ok_or_else(|| "runtime scalar expression source is unavailable at this boundary".to_owned())?;
                let argument = append_runtime_shape_argument(arguments, operand)?;
                Ok(MirShapeExpr::ScalarArgument { argument })
            }
            RuntimeExtentId::ExpressionAxis { expression, axis } => {
                let operand = expression_value
                    .filter(|(available, _)| *available == expression)
                    .map(|(_, operand)| operand.clone())
                    .ok_or_else(|| "runtime shape expression source is unavailable at this boundary".to_owned())?;
                let argument = append_runtime_shape_argument(arguments, operand)?;
                Ok(MirShapeExpr::TensorAxis { argument, axis })
            }
            RuntimeExtentId::ExpressionFieldAxis { expression, fields, axis } => {
                let operand = expression_value
                    .filter(|(available, _)| *available == expression)
                    .map(|(_, operand)| operand.clone())
                    .ok_or_else(|| {
                        "runtime tensor-field expression source is unavailable at this boundary".to_owned()
                    })?;
                let argument = append_runtime_shape_argument(arguments, operand)?;
                Ok(MirShapeExpr::TensorFieldAxis { argument, fields, axis })
            }
            _ => Err("runtime shape relation reached an unexpected extent source".to_owned()),
        }
    }

    /// Resolve a captured or lexical binding to the MIR operand its shape
    /// extent names.
    fn runtime_shape_binding_operand(&self, binding: DefId) -> Option<MirOperand> {
        if let Some(place) = self.captured_bindings.get(&binding) {
            Some(MirOperand::Place(place.clone()))
        } else {
            self.bindings
                .get(&binding)
                .map(|binding| MirOperand::Place(MirPlace::local(binding.local)))
        }
    }

    pub(super) fn lower_hir_call_args(&mut self, args: &[HirCallArg]) -> Option<Vec<MirOperand>> {
        let mut lowered = Vec::with_capacity(args.len());
        for arg in args {
            lowered.push(self.lower_call_arg_value(&arg.expr)?);
        }
        Some(lowered)
    }

    /// Lower a method call into a provider or collection intrinsic.
    ///
    /// Provider method calls are recognized when the receiver path resolves to a
    /// provider import. Collection methods are deliberately name/type/arity
    /// gated so unsupported methods do not masquerade as generic runtime calls.
    pub(super) fn lower_method_call(
        &mut self,
        receiver: &HirExpression,
        method: Symbol,
        generic_args: &[TypeId],
        args: &[HirCallArg],
        conversion: Option<DefId>,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let mark = self.asserted_receivers.len();
        let result = self.lower_method_call_on_receiver(receiver, method, generic_args, args, conversion, expr);
        if self.method_writes_receiver(receiver, method, args.len()) {
            self.store_back_asserted_receivers(mark, expr.span);
        } else {
            self.asserted_receivers.truncate(mark);
        }
        result
    }

    /// Whether the registry marks `receiver.method(..)` as rewriting its
    /// receiver in place (the same predicate typecheck gates on).
    fn method_writes_receiver(&self, receiver: &HirExpression, method: Symbol, arg_count: usize) -> bool {
        let (Some(ty), Some(name)) = (receiver.ty, self.resolve_method_name(method)) else {
            return false;
        };
        let payload = self.types.get(self.method_receiver_payload_id(ty));
        radix_intrinsics::lookup_method_for_type(payload, name, arg_count)
            .is_some_and(radix_intrinsics::is_mutating_receiver)
    }

    fn lower_method_call_on_receiver(
        &mut self,
        receiver: &HirExpression,
        method: Symbol,
        generic_args: &[TypeId],
        args: &[HirCallArg],
        conversion: Option<DefId>,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        if let Some(target) = self.genus_method_target(receiver, method) {
            let mut lowered_args = Vec::with_capacity(args.len() + 1);
            lowered_args.push(self.lower_expr_value(receiver)?);
            lowered_args.extend(self.lower_hir_call_args(args)?);
            return self.lower_definition_call(target.def_id, Vec::new(), lowered_args, conversion, 1, expr);
        }
        if let Some(result) = self.lower_union_method_call(receiver, method, args, conversion, expr) {
            return result;
        }
        if let Some(target) = self.bound_method_target(receiver, method) {
            // D1.1: a call through a type-parameter bound targets the contract
            // method with the receiver type as its witness; monomorphization
            // rewrites it to the witness's own method.
            let receiver_ty = MirType::semantic(receiver.ty?);
            let mut lowered_args = Vec::with_capacity(args.len() + 1);
            lowered_args.push(self.lower_expr_value(receiver)?);
            lowered_args.extend(self.lower_hir_call_args(args)?);
            return self.lower_definition_call(target, vec![receiver_ty], lowered_args, conversion, 1, expr);
        }

        let imported_method_target = receiver.ty.and_then(|receiver_ty| {
            let def_id = self.method_receiver_struct_def(receiver_ty)?;
            self.context
                .imported_method_providers
                .get(&(def_id, method))
                .cloned()
        });
        if let Some(target) = imported_method_target {
            let mut lowered_args = Vec::with_capacity(args.len() + 1);
            lowered_args.push(self.lower_expr_value(receiver)?);
            lowered_args.extend(self.lower_hir_call_args(args)?);
            self.emit_call_shape_check(expr.id, &lowered_args, 1, expr.span)?;
            let ty = self.expr_ty(expr)?;
            let mut module = target.provider.module;
            module.push(target.provider.item);
            return Some(self.runtime_call_value(
                MirIntrinsic::Provider(MirProvider { kind: target.provider.kind, module, name: target.canonical_name }),
                lowered_args,
                ty,
                expr.span,
            ));
        }

        if let HirExpressionKind::Path(def_id) = receiver.kind
            && let Some(import) = self.context.provider_imports.get(&def_id).cloned()
        {
            // D-PA1/D-PA2: a resolvable user-code provider call (the
            // import resolved to a sibling module under package identity
            // context) lowers to a signature-known external definition
            // keyed to the VERB — the direct cross-module symbol reference,
            // not a versioned host ABI call and not the raw provider route.
            if let Some(verb_def) = import
                .file_interface_functions
                .as_ref()
                .and_then(|functions| functions.get(&method))
            {
                let lowered_args = self.lower_hir_call_args(args)?;
                return self.lower_definition_call(*verb_def, Vec::new(), lowered_args, conversion, 0, expr);
            }
            let lowered_args = self.lower_hir_call_args(args)?;
            self.emit_call_shape_check(expr.id, &lowered_args, 0, expr.span)?;
            let ty = self.expr_ty(expr)?;
            let mut module = import.module;
            module.push(import.item);
            return Some(self.runtime_call_value(
                MirIntrinsic::Provider(MirProvider { kind: import.kind, module, name: method }),
                lowered_args,
                ty,
                expr.span,
            ));
        }

        // A function-typed genus field (`h.f(1)`) is a callable value stored
        // in a struct field, not a method: lower the call through the field's
        // projected place like any other value callee. Local struct fields
        // only; an imported function-typed field stays fail-closed.
        if let Some(receiver_ty) = receiver.ty
            && let Some(def_id) = self.method_receiver_struct_def(receiver_ty)
            && let Some(field_ty) = self
                .context
                .struct_field_types
                .get(&def_id)
                .and_then(|fields| fields.get(&method))
                .copied()
        {
            let field_signature = match self.normalized_type(field_ty.semantic_id()) {
                Type::Func(signature) => Some(signature.clone()),
                _ => None,
            };
            if let Some(signature) = field_signature {
                let callee = self.lower_field(receiver, method, expr)?;
                let lowered_args = self.lower_positional_call_args(args)?;
                self.emit_call_shape_check(expr.id, &lowered_args, 0, expr.span)?;
                let ty = self.expr_ty(expr)?;
                if let Some(err) = signature.err {
                    return self.lower_failable_call(
                        MirCallee::Value(callee),
                        lowered_args,
                        ty,
                        MirType::semantic(err),
                        conversion,
                        expr.span,
                    );
                }
                return Some(self.call_value_operand(callee, lowered_args, ty, expr.span));
            }
        }

        let method_name = self.resolve_method_name(method)?.to_owned();
        if let Some(receiver_ty) = receiver.ty {
            let payload = self.types.get(self.method_receiver_payload_id(receiver_ty));
            if radix_intrinsics::lookup_method_for_type(payload, &method_name, args.len())
                .is_some_and(|entry| matches!(entry.mir_kind, radix_intrinsics::IntrinsicMirKind::TensorLimes))
            {
                return self.lower_tensor_limes(receiver, args, expr);
            }
        }
        if method_name == "rope_norm" || method_name == "rope_half" {
            return self.lower_tensor_rope(receiver, method, &method_name, generic_args, args, expr);
        }
        if let Some(lowered) = self.lower_sermo_method(receiver, &method_name, args, expr) {
            return Some(lowered);
        }
        if let [arg] = args
            && let Some(lowered) = self.lower_register_extremum(receiver, &method_name, &arg.expr, expr)
        {
            return lowered;
        }
        if let Some(lowered) = self.lower_vector_method(receiver, &method_name, args, expr) {
            return lowered;
        }
        if let Some(lowered) = self.lower_matrix_method(receiver, &method_name, args, expr) {
            return Some(lowered);
        }
        if let Some(lowered) = self.lower_numeric_operator_method(receiver, &method_name, args, expr) {
            return Some(lowered);
        }
        // WHY: scalar `.to_text()` is the scribe `"§"(value)` render. Package
        // MIR test helpers stringify numbers this way; the collection-method
        // fallback would fail closed after the unit-variant metadata remap.
        if (method_name == "to_text" || method_name == "ad_textum") && args.is_empty() {
            let rec_ty = self.expr_ty(receiver)?;
            let value = self.lower_expr_value(receiver)?;
            let text_ty = self.expr_ty(expr)?;
            return Some(self.render_scribe_value_text(value, rec_ty, text_ty, expr.span));
        }
        if let Some(lowered) = self.lower_atomic_method(receiver, &method_name, args, expr) {
            return Some(lowered);
        }
        if let Some(lowered) = self.lower_lista_ordina(receiver, &method_name, args, expr) {
            return Some(lowered);
        }
        if method_name == "silu"
            && let Some(receiver_ty) = receiver.ty
        {
            let payload = self.types.get(self.method_receiver_payload_id(receiver_ty));
            if let Some(entry) = radix_intrinsics::lookup_method_for_type(payload, &method_name, args.len())
                && matches!(entry.receiver, radix_intrinsics::IntrinsicReceiver::Tensor)
            {
                return self.lower_tensor_silu(receiver, expr);
            }
        }

        if let Some(receiver_ty) = receiver.ty {
            let payload = self.types.get(self.method_receiver_payload_id(receiver_ty));
            if let Some(entry) = radix_intrinsics::lookup_method_for_type(payload, &method_name, args.len())
                && matches!(entry.mir_kind, radix_intrinsics::IntrinsicMirKind::ListExtend)
            {
                return self.lower_extende(receiver, &args[0].expr, expr);
            }
        }

        if let Some(receiver_ty) = receiver.ty {
            let payload = self.types.get(self.method_receiver_payload_id(receiver_ty));
            if let Some(entry) = radix_intrinsics::lookup_method_for_type(payload, &method_name, args.len())
                && radix_intrinsics::mir_is_higher_order_lista(entry)
            {
                let closure = &args[0].expr;
                return match entry.mir_kind {
                    radix_intrinsics::IntrinsicMirKind::HigherOrderAll => self.lower_omnia(receiver, closure, expr),
                    radix_intrinsics::IntrinsicMirKind::HigherOrderAny => self.lower_quilibet(receiver, closure, expr),
                    radix_intrinsics::IntrinsicMirKind::HigherOrderFind => self.lower_reperta(receiver, closure, expr),
                    radix_intrinsics::IntrinsicMirKind::HigherOrderFiltrata => {
                        self.lower_filtrata(receiver, closure, expr)
                    }
                    radix_intrinsics::IntrinsicMirKind::HigherOrderMap => self.lower_mappata(receiver, closure, expr),
                    radix_intrinsics::IntrinsicMirKind::HigherOrderReduce => {
                        let initial = &args[1].expr;
                        self.lower_reducta(receiver, closure, initial, expr)
                    }
                    radix_intrinsics::IntrinsicMirKind::HigherOrderCumulata => {
                        let initial = &args[1].expr;
                        self.lower_cumulata(receiver, closure, initial, expr)
                    }
                    // The keyed `ordinata(key)` is retired; only the
                    // in-place `ordina(key)` reaches lowering.
                    radix_intrinsics::IntrinsicMirKind::HigherOrderSortBy => {
                        self.lower_sort_by_key(receiver, closure, expr)
                    }
                    _ => None,
                };
            }
        }

        if let Some(receiver_ty) = receiver.ty {
            let payload = self.types.get(self.method_receiver_payload_id(receiver_ty));
            if let Some(entry) = radix_intrinsics::lookup_method_for_type(payload, &method_name, args.len())
                && entry
                    .params
                    .contains(&radix_intrinsics::IntrinsicParamKind::TextusOrReplacer)
                && matches!(args[1].expr.kind, HirExpressionKind::Clausura(..))
            {
                return self.lower_regex_replace_closure(receiver, &args[0].expr, &args[1].expr, expr);
            }
        }

        let Some(op) = self.collection_method_op(receiver, method, args, expr.span) else {
            self.errors.push(MirError::unsupported(
                expr.span,
                "method call before runtime/provider MIR lowering",
            ));
            return None;
        };

        // Q-X10: the zero-arg generic `grid.formata<T>()` shares
        // `TensorReshape` with the one-arg `grid.forma(shape)` method but
        // carries no value argument; its shape comes from the result type.
        if op == MirCollectionOp::TensorReshape && args.is_empty() {
            return self.lower_tensor_formata(receiver, expr);
        }

        let mut lowered_receiver = self.lower_expr_value(receiver)?;
        let expr_ty = self.expr_ty(expr)?;
        if matches!(
            op,
            MirCollectionOp::ScalarMath(_) | MirCollectionOp::Absolute | MirCollectionOp::Signum
        ) {
            lowered_receiver = self.settle_unsized_float_receiver(receiver, lowered_receiver, expr_ty, expr.span)?;
        }
        // A bounded cord (`ascii<N>` / `textus<N>`) receiver widens to the
        // unbounded `textus` carrier before a text-receiver runtime call, the
        // same copy `↦ textus` performs (D7). The checker already resolves
        // these intrinsics for cord receivers; the MIR rows require `textus`.
        if is_text_receiver_op(op)
            && let Some(source_ty) = self.expr_ty(receiver)
            && matches!(
                self.normalized_type(source_ty.semantic_id()),
                Type::BoundedTextus { .. } | Type::BoundedAscii { .. }
            )
        {
            let textus = MirType::semantic(self.types.primitive(Primitive::Textus));
            lowered_receiver = self.runtime_call_value(
                MirIntrinsic::Convert(MirConversion {
                    store: None,
                    flavor: MirConversionFlavor::Runtime,
                    source_ty,
                    target_ty: textus,
                    params: Vec::new(),
                    recovery: None,
                    struct_defaults: Vec::new(),
                }),
                vec![lowered_receiver],
                textus,
                expr.span,
            );
        }
        if matches!(op, MirCollectionOp::TensorGet | MirCollectionOp::TensorSet)
            && let Some(lowered) = self.lower_kernel_checked_tensor_access(
                op,
                &lowered_receiver,
                receiver,
                &method_name,
                args,
                expr_ty,
                expr.span,
            )
        {
            return lowered;
        }
        let shape_arg = tensor_shape_hole_argument(op, args);
        let mut lowered_flat = None;
        let lowered_shape = if shape_arg.is_some() {
            let flat = if op == MirCollectionOp::TensorFromFlat {
                let flat = self.lower_expr_value(&args[0].expr)?;
                lowered_flat = Some(flat.clone());
                flat
            } else {
                lowered_receiver.clone()
            };
            let expression_value = (receiver.id.0, lowered_receiver.clone());
            // GSF-2: symbolic axes keep the finalizable ShapeSize term here;
            // only a declared Runtime extent lowers to the TensorShapeSize
            // read. Witnessed size parameters stay symbolic on this path.
            Some(self.lower_lista_to_tensor_shape_with_expression(
                flat,
                expr_ty.semantic_id(),
                Some(&expression_value),
                expr.span,
                false,
            )?)
        } else {
            None
        };
        let mut lowered_args = Vec::with_capacity(args.len() + 1);
        lowered_args.push(lowered_receiver);
        for (index, arg) in args.iter().enumerate() {
            if Some(index) == shape_arg
                && let Some(shape) = &lowered_shape
            {
                lowered_args.push(shape.clone());
                continue;
            }
            if index == 0
                && let Some(flat) = &lowered_flat
            {
                lowered_args.push(flat.clone());
                continue;
            }
            lowered_args.push(self.lower_collection_call_arg(op, index, &arg.expr)?);
        }
        // WHY: semantic expected-type unification can annotate a scalar
        // collection intrinsic with its nullable enclosing type at the call
        // site. The MIR operation contract remains scalar; adapt the value at
        // the expression boundary instead of weakening intrinsic validation.
        //
        // Tier 2 selects the checked read at this lowering seam from the
        // receiver's declared type. The runner sees the strict element return
        // type and traps on OOB; the source-level nullable carrier is restored
        // with `Some` so existing `vel`/`coalesce` expressions stay typed.
        let call_ty = match op {
            MirCollectionOp::Length => MirType::semantic(self.types.primitive(Primitive::Numerus)),
            MirCollectionOp::Index | MirCollectionOp::TensorGet => self
                .checked_read_result_type(op, receiver.ty, expr_ty)
                .or_else(|| self.index_read_result_type(receiver.ty, expr_ty))
                .unwrap_or(expr_ty),
            _ => expr_ty,
        };
        if op == MirCollectionOp::TensorSet && self.is_checked_tensor_method(receiver, &method_name, args) {
            let (Some(index_ty), Some(value_ty), Some(receiver_ty)) = (
                self.expr_ty(&args[0].expr),
                self.expr_ty(&args[1].expr),
                self.checked_tensor_receiver_ty(receiver),
            ) else {
                return None;
            };
            return self.lower_checked_tensor_access(
                op,
                lowered_args,
                &[receiver_ty, index_ty, value_ty],
                call_ty,
                expr.span,
            );
        }
        if op == MirCollectionOp::TensorGet && self.is_checked_tensor_method(receiver, &method_name, args) {
            let (Some(index_ty), Some(receiver_ty)) =
                (self.expr_ty(&args[0].expr), self.checked_tensor_receiver_ty(receiver))
            else {
                return None;
            };
            return self.lower_checked_tensor_access(op, lowered_args, &[receiver_ty, index_ty], expr_ty, expr.span);
        }
        if op == MirCollectionOp::Index && method_name == "apud" && args.len() == 1 && self.device_role.is_none() {
            // need: the `at` read reports an out-of-range index or a
            // missing key on `⇥ textus` through a checked helper, the seam the
            // checked tensor access above uses.
            let (Some(index_ty), Some(receiver_ty)) =
                (self.expr_ty(&args[0].expr), self.checked_tensor_receiver_ty(receiver))
            else {
                return None;
            };
            let keyed = matches!(self.types.get(receiver_ty.semantic_id()), Type::Map(..) | Type::Ledger(..));
            let textus = MirType::semantic(self.types.primitive(Primitive::Textus));
            let helper = self.push_checked_at_helper(keyed, [receiver_ty, index_ty], expr_ty, textus, expr.span);
            return self.lower_failable_call(
                MirCallee::Function(helper),
                lowered_args,
                expr_ty,
                textus,
                None,
                expr.span,
            );
        }
        if op == MirCollectionOp::Slice
            && method_name == "sectio"
            && args.len() == 2
            && self.device_role.is_none()
            && (self.is_lista_receiver(receiver) || self.is_textus_receiver(receiver))
            && let (Some(start_ty), Some(end_ty), Some(receiver_ty)) = (
                self.expr_ty(&args[0].expr),
                self.expr_ty(&args[1].expr),
                self.checked_window_receiver_ty(receiver),
            )
        {
            // need / Q-B3: the list and text windows report out-of-range
            // bounds on `⇥`, the same fault family as the `at` read. The bare runtime
            // op still trims, so it is only reached behind the guard.
            let textus = MirType::semantic(self.types.primitive(Primitive::Textus));
            let helper =
                self.push_checked_trim_helper(op, &[receiver_ty, start_ty, end_ty], expr_ty, textus, expr.span);
            return self.lower_failable_call(
                MirCallee::Function(helper),
                lowered_args,
                expr_ty,
                textus,
                None,
                expr.span,
            );
        }
        if op == MirCollectionOp::Take
            && method_name == "prima"
            && args.len() == 1
            && self.device_role.is_none()
            && self.is_lista_receiver(receiver)
            && let (Some(count_ty), Some(receiver_ty)) =
                (self.expr_ty(&args[0].expr), self.checked_tensor_receiver_ty(receiver))
        {
            // need: the list prefix reports a count over the length
            // on `⇥`; the runtime `Take` below only ever runs guarded.
            let textus = MirType::semantic(self.types.primitive(Primitive::Textus));
            let helper = self.push_checked_trim_helper(op, &[receiver_ty, count_ty], expr_ty, textus, expr.span);
            return self.lower_failable_call(
                MirCallee::Function(helper),
                lowered_args,
                expr_ty,
                textus,
                None,
                expr.span,
            );
        }
        if matches!(
            op,
            MirCollectionOp::Append | MirCollectionOp::RemoveFirst | MirCollectionOp::RemoveLast
        ) && let Some(limit) = self.ranged_list_limit(receiver, op)
            && let Some(receiver_ty) = self.checked_tensor_receiver_ty(receiver)
        {
            // A length-ranged list push (past the maximum) or removal (below
            // the minimum) reports on `⇥`, like the `at` read: the checked
            // helper always runs, so the handler, `⊥` default, `⇥` channel or
            // trap sees the failure and a failed op leaves the list untouched.
            let item_ty = if op == MirCollectionOp::Append {
                Some(self.expr_ty(&args[0].expr)?)
            } else {
                None
            };
            let textus = MirType::semantic(self.types.primitive(Primitive::Textus));
            let helper = self.push_ranged_list_helper(op, receiver_ty, item_ty, limit, call_ty, textus, expr.span);
            return self.lower_failable_call(
                MirCallee::Function(helper),
                lowered_args,
                call_ty,
                textus,
                None,
                expr.span,
            );
        }
        if op == MirCollectionOp::Append
            && self.is_bounded_append_under_handler(receiver)
            && let (Some(item_ty), Some(receiver_ty)) =
                (self.expr_ty(&args[0].expr), self.checked_tensor_receiver_ty(receiver))
        {
            // A bounded append that overflows is a recoverable failure. The
            // bare runtime call has no error edge, so inside a `fac … cape` or
            // `capta` it goes through a failable helper whose `TryCall` error
            // block is the enclosing handler (the same seam as the checked
            // tensor access above).
            let textus = MirType::semantic(self.types.primitive(Primitive::Textus));
            let helper = self.push_bounded_append_helper(receiver_ty, item_ty, call_ty, textus, expr.span);
            return self.lower_failable_call(
                MirCallee::Function(helper),
                lowered_args,
                call_ty,
                textus,
                None,
                expr.span,
            );
        }
        let value = self.runtime_call_value(MirIntrinsic::Collection(op), lowered_args, call_ty, expr.span);
        if call_ty != expr_ty && self.types.nullable_inner(expr_ty.semantic_id()) == Some(call_ty.semantic_id()) {
            return Some(self.assign_temp(MirValueKind::Option(MirOptionOp::Some(value)), expr_ty, expr.span));
        }
        Some(value)
    }

    /// A same-as-receiver float intrinsic (`x.sqrt()`, `x.abs()`, ...) returns
    /// its receiver's type, but a float literal receiver has no width of its
    /// own: it is the unsized `fractus` until a store gives it one. When the
    /// store repaints the call to a sized slot (`let f64 y ← (8.0).cbrt()`) the
    /// receiver has to take that width too, or the call's result and receiver
    /// types disagree. The width is applied here, at the receiver's own value,
    /// exactly as the store would apply it to the literal.
    fn settle_unsized_float_receiver(
        &mut self,
        receiver: &HirExpression,
        lowered: MirOperand,
        result_ty: MirType,
        span: Span,
    ) -> Option<MirOperand> {
        let receiver_ty = self.expr_ty(receiver)?;
        let unsized_float = matches!(
            self.normalized_type(receiver_ty.semantic_id()),
            Type::Primitive(Primitive::Fractus)
        );
        let sized_float = matches!(
            self.normalized_type(result_ty.semantic_id()),
            Type::SizedNumeric(Primitive::Fractus, _)
        );
        if unsized_float && sized_float {
            return Some(self.assign_temp(MirValueKind::Operand(lowered), result_ty, span));
        }
        Some(lowered)
    }

    fn lower_tensor_limes(
        &mut self,
        receiver: &HirExpression,
        args: &[HirCallArg],
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        if !args.is_empty() {
            self.errors
                .push(MirError::validation(expr.span, "tensor limes does not accept value arguments"));
            return None;
        }
        let Some(policy) = self
            .context
            .tensor_edge_policies
            .calls
            .get(&expr.id)
            .copied()
        else {
            self.errors.push(MirError::unsupported(
                expr.span,
                "tensor limes policy resolution is missing from semantic analysis",
            ));
            return None;
        };
        let (mir_policy, pending) = match policy {
            radix_hir::ResolvedTensorEdgePolicy::Builtin(policy) => (
                MirEdgePolicy::Builtin(match policy {
                    radix_types::BuiltinTensorEdgePolicy::Clamp => MirBuiltinEdgePolicy::Clamp,
                    radix_types::BuiltinTensorEdgePolicy::Reflect => MirBuiltinEdgePolicy::Reflect,
                    radix_types::BuiltinTensorEdgePolicy::Wrap => MirBuiltinEdgePolicy::Wrap,
                }),
                None,
            ),
            radix_hir::ResolvedTensorEdgePolicy::Custom { policy_def, remap, transform } => (
                // Private staging tag only. `resolve_pending_tensor_limes`
                // replaces this before MIR validation and no serialized MIR
                // can contain this placeholder as the selected policy.
                MirEdgePolicy::Builtin(MirBuiltinEdgePolicy::Clamp),
                Some(super::PendingTensorLimesDispatch::Custom { policy_def, remap, transform }),
            ),
            radix_hir::ResolvedTensorEdgePolicy::Deferred { policy_type } => (
                // A concrete instance replaces this private placeholder in
                // the existing MIR monomorphization pass. The generic template
                // itself is not executed by the runner.
                MirEdgePolicy::Builtin(MirBuiltinEdgePolicy::Clamp),
                Some(super::PendingTensorLimesDispatch::Deferred { policy_type }),
            ),
        };
        let lowered_receiver = self.lower_expr_value(receiver)?;
        let return_ty = self.expr_ty(expr)?;
        let value = self.runtime_call_value(
            MirIntrinsic::TensorLimes(mir_policy),
            vec![lowered_receiver],
            return_ty,
            expr.span,
        );
        if let Some(dispatch) = pending {
            let MirOperand::Temp(destination) = value else {
                self.errors.push(MirError::unsupported(
                    expr.span,
                    "custom tensor limes did not lower to a value temporary",
                ));
                return None;
            };
            if let Ok(mut queue) = self.context.pending_tensor_limes.lock() {
                queue.push(super::PendingTensorLimes {
                    function: self.parent_function_id,
                    destination,
                    dispatch,
                    span: expr.span,
                });
            } else {
                self.errors.push(MirError::unsupported(
                    expr.span,
                    "tensor limes callback queue was poisoned during lowering",
                ));
                return None;
            }
        }
        Some(value)
    }

    /// Lower the clean-break `rope_norm<dim>(pos, rope_table)` and
    /// `rope_half<dim>(pos, rope_table)` surfaces to the shared MIR
    /// `[x, pos, dim, rope_table]` collection contract, emitting exactly the
    /// MIR collection op the intrinsic registry selected for `method_name`
    /// (`TensorRope` consecutive pairs, `TensorRopeHalf` half-split pairs). A
    /// concrete generic argument becomes an integer constant; a generic
    /// template carries the compiler-owned symbolic shape-size constant until
    /// MIR monomorphization binds it.
    fn lower_tensor_rope(
        &mut self,
        receiver: &HirExpression,
        method: Symbol,
        method_name: &str,
        generic_args: &[TypeId],
        args: &[HirCallArg],
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let Some(op) = self.collection_method_op(receiver, method, args, expr.span) else {
            self.errors.push(MirError::unsupported(
                expr.span,
                format!("{method_name} method is not admitted for this receiver"),
            ));
            return None;
        };
        let (MirCollectionOp::TensorRope | MirCollectionOp::TensorRopeHalf) = op else {
            self.errors.push(MirError::unsupported(
                expr.span,
                format!("{method_name} is not backed by a TensorRope MIR collection"),
            ));
            return None;
        };
        let [pos, rope_table] = args else {
            self.errors.push(MirError::validation(
                expr.span,
                format!("{method_name} requires a position and an explicit rope table argument"),
            ));
            return None;
        };
        let receiver = self.lower_expr_value(receiver)?;
        let pos = self.lower_expr_value(&pos.expr)?;
        let rope_table = self.lower_expr_value(&rope_table.expr)?;
        let dim = self.lower_rope_dim(method_name, generic_args, expr.span)?;
        let ty = self.expr_ty(expr)?;
        Some(self.runtime_call_value(
            MirIntrinsic::Collection(op),
            vec![receiver, pos, dim, rope_table],
            ty,
            expr.span,
        ))
    }

    fn lower_rope_dim(&mut self, method_name: &str, generic_args: &[TypeId], span: Span) -> Option<MirOperand> {
        let Some(&generic_arg) = generic_args.first() else {
            self.errors.push(MirError::validation(
                span,
                format!("{method_name} requires one concrete magnitudo generic argument"),
            ));
            return None;
        };
        let size = match self.types.get(generic_arg) {
            Type::Param(symbol) => self
                .context
                .interner
                .and_then(|interner| radix_types::parse_explicit_size(interner.resolve(*symbol))),
            _ => None,
        };
        let name = match size {
            Some(radix_types::ExplicitSize::Natural(value)) => {
                #[expect(
                    clippy::cast_possible_wrap,
                    reason = "Faber natural capacities are bounded before MIR lowering"
                )]
                return Some(MirOperand::Constant(MirConstant::Int(value as i64)));
            }
            Some(radix_types::ExplicitSize::Name(name)) => name,
            Some(radix_types::ExplicitSize::Shape(_)) | None => {
                self.errors.push(MirError::validation(
                    span,
                    format!("{method_name} generic dimension is not a magnitudo argument"),
                ));
                return None;
            }
        };
        let symbol = self
            .context
            .interner
            .and_then(|interner| interner.lookup(name));
        let Some(shape) = symbol.and_then(|symbol| {
            (0..self.types.index_count()).find_map(|ordinal| {
                #[expect(clippy::cast_possible_truncation, reason = "IndexId is a bounded arena index")]
                let id = IndexId(ordinal as u32);
                matches!(self.types.get_index(id), IndexExpr::Param(found) if *found == symbol).then_some(id)
            })
        }) else {
            self.errors.push(MirError::validation(
                span,
                format!("{method_name} generic dimension has no shape witness"),
            ));
            return None;
        };
        Some(MirOperand::Constant(MirConstant::ShapeSize { shape, axis: None }))
    }

    /// Lower A1/C5 numeric collection arithmetic, the list-only `% ÷ ⊙ ⊘` and
    /// the per-element comparisons `≺ ≻ ≤ ≥ ≅ ≇` of a numeric `lista`.
    ///
    /// Every pair reaches one collection runtime op, so the runner pairs the
    /// live lengths (a mismatch is an error) and stores each exact result under
    /// the result element type. A literal `BoundedArray` capacity is a storage
    /// limit, never the live length, so it never stands in for one; queue/stack
    /// capacities are the same.
    pub(super) fn lower_lista_elementwise(
        &mut self,
        op: HirBinOp,
        lhs: &HirExpression,
        rhs: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let collection_op = match op {
            HirBinOp::Add => MirCollectionOp::ArrayAdd,
            HirBinOp::Sub => MirCollectionOp::ArraySub,
            HirBinOp::Mul | HirBinOp::Hadamard => MirCollectionOp::ArrayMul,
            HirBinOp::Div | HirBinOp::HadamardDiv => MirCollectionOp::ArrayDiv,
            HirBinOp::Mod => MirCollectionOp::ArrayMod,
            HirBinOp::TrueDiv => MirCollectionOp::ArrayTrueDiv,
            HirBinOp::Lt => MirCollectionOp::ArrayLt,
            HirBinOp::Gt => MirCollectionOp::ArrayGt,
            HirBinOp::LtEq => MirCollectionOp::ArrayLtEq,
            HirBinOp::GtEq => MirCollectionOp::ArrayGtEq,
            HirBinOp::ApproxEq => MirCollectionOp::ArrayValueEq,
            HirBinOp::ApproxNotEq => MirCollectionOp::ArrayValueNotEq,
            HirBinOp::And => MirCollectionOp::ArrayAnd,
            HirBinOp::Or => MirCollectionOp::ArrayOr,
            _ => {
                self.errors.push(MirError::unsupported(
                    expr.span,
                    "lista elementwise lowering received a non-arithmetic operator",
                ));
                return None;
            }
        };
        let lhs_value = self.lower_expr_value(lhs)?;
        let rhs_value = self.lower_expr_value(rhs)?;
        let result_ty = self.expr_ty(expr)?;
        Some(self.runtime_call_value(
            MirIntrinsic::Collection(collection_op),
            vec![lhs_value, rhs_value],
            result_ty,
            expr.span,
        ))
    }

    /// Lower unary `-` (or `non` over a `lista<bivalens>`) to the elementwise
    /// collection op. `None` means the operand is not a list.
    pub(super) fn lower_lista_negate(
        &mut self,
        op: HirUnOp,
        operand: &HirExpression,
        expr: &HirExpression,
    ) -> Option<Option<MirOperand>> {
        let ty = operand.ty?;
        let payload = self.types.peel_access_marker(ty).1;
        if !matches!(
            self.normalized_type(payload),
            Type::Array(_) | Type::BoundedArray { .. } | Type::RangedArray { .. }
        ) {
            return None;
        }
        let lowered = (|| {
            let value = self.lower_expr_value(operand)?;
            let result_ty = self.expr_ty(expr)?;
            Some(self.runtime_call_value(
                MirIntrinsic::Collection(if op == HirUnOp::Not {
                    MirCollectionOp::ArrayNot
                } else {
                    MirCollectionOp::ArrayNeg
                }),
                vec![value],
                result_ty,
                expr.span,
            ))
        })();
        Some(lowered)
    }

    /// Lower a glyph-product binop (`·` `×` `⊗` `⊙` `⊘`) by operand family,
    /// reusing the vector/matrix method machinery and the collection-op
    /// intrinsic seam. Every unsupported shape/type combination fails closed
    /// with a stable structured diagnostic — never a silent pass.
    pub(super) fn lower_glyph_product(
        &mut self,
        op: HirBinOp,
        lhs: &HirExpression,
        rhs: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let Some(lhs_ty_id) = lhs.ty else {
            self.errors
                .push(MirError::missing_type(expr.span, "glyph product lhs operand"));
            return None;
        };
        let Some(rhs_ty_id) = rhs.ty else {
            self.errors
                .push(MirError::missing_type(expr.span, "glyph product rhs operand"));
            return None;
        };
        // An access marker is a binding fact, not part of the carrier.
        let lhs_ty = self
            .normalized_type(self.types.peel_access_marker(lhs_ty_id).1)
            .clone();
        let rhs_ty = self
            .normalized_type(self.types.peel_access_marker(rhs_ty_id).1)
            .clone();

        // `⊗` outer: vector⊗vector builds the matrix-register aggregate; the
        // Kronecker forms (matrix⊗matrix / tensor⊗tensor) stay fail-closed —
        // no new MIR op is invented for them.
        if op == HirBinOp::TensorProduct {
            if matches!((&lhs_ty, &rhs_ty), (Type::Vector(..), Type::Vector(..))) {
                return self.lower_vector_outer(lhs, rhs, expr);
            }
            let detail = if matches!((&lhs_ty, &rhs_ty), (Type::Matrix(..), Type::Matrix(..))) {
                "matrix tensor product (Kronecker)"
            } else if matches!((&lhs_ty, &rhs_ty), (Type::Tensor(..), Type::Tensor(..))) {
                "tensor tensor product (Kronecker)"
            } else {
                "tensor product"
            };
            self.glyph_fail_closed(detail, expr.span);
            return None;
        }

        // Row-vector convention (ruling 2026-10-04): `v · m` with `v` of width K
        // and `m` of shape [K, N] is a width-N vector, out[j] = sum_k v[k] * m[k, j].
        if op == HirBinOp::Dot && matches!((&lhs_ty, &rhs_ty), (Type::Vector(..), Type::Matrix(..))) {
            return self.lower_vector_matrix_dot(lhs, rhs, expr);
        }

        let Some(method_name) = glyph_method_name(op, &lhs_ty, &rhs_ty) else {
            self.glyph_fail_closed("glyph product operand types", expr.span);
            return None;
        };

        match (&lhs_ty, &rhs_ty) {
            (Type::Tensor(..), Type::Tensor(..)) => self.lower_glyph_collection_op(lhs, method_name, rhs, expr),
            (Type::Vector(..), Type::Vector(..)) => {
                let arg = HirCallArg { name: None, spread: false, marker: None, expr: rhs.clone(), span: rhs.span };
                self.lower_vector_method(lhs, method_name, std::slice::from_ref(&arg), expr)
                    .flatten()
            }
            (Type::Matrix(..), Type::Matrix(..) | Type::Vector(..)) => {
                let arg = HirCallArg { name: None, spread: false, marker: None, expr: rhs.clone(), span: rhs.span };
                self.lower_matrix_method(lhs, method_name, std::slice::from_ref(&arg), expr)
            }
            _ => {
                self.glyph_fail_closed("glyph product operand types", expr.span);
                None
            }
        }
    }

    /// Fail closed for a glyph-product shape/type combination with a stable
    /// structured diagnostic.
    fn glyph_fail_closed(&mut self, detail: &str, span: Span) {
        self.errors.push(MirError::unsupported(
            span,
            format!("{detail} is not supported on the MIR target"),
        ));
    }

    /// Tensor glyphs route through the collection-op intrinsic seam exactly
    /// like the method twins (`a.matmul(b)` / `a.multiplicata(b)`): the registry
    /// row is resolved by receiver type + name + arity, then lowered to a
    /// `MirIntrinsic::Collection` runtime call.
    fn lower_glyph_collection_op(
        &mut self,
        lhs: &HirExpression,
        method_name: &str,
        rhs: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let Some(receiver_ty) = lhs.ty else {
            self.errors
                .push(MirError::missing_type(expr.span, "glyph collection receiver"));
            return None;
        };
        let receiver_ty = self.normalized_type(self.method_receiver_payload_id(receiver_ty));
        let entry = radix_intrinsics::lookup_method_for_type(receiver_ty, method_name, 1)?;
        let op = radix_intrinsics::mir_collection_op(entry)?;
        let lowered_args = vec![self.lower_expr_value(lhs)?, self.lower_expr_value(rhs)?];
        let ty = self.expr_ty(expr)?;
        Some(self.runtime_call_value(MirIntrinsic::Collection(op), lowered_args, ty, expr.span))
    }

    /// Outer product `v ⊗ w` → `matrix<elem, [n, m]>` register aggregate built
    /// from lane-product cells (row-major: result row i = lhs lane i). The
    /// smallest matrix-register construct — no new MIR node type.
    fn lower_vector_outer(
        &mut self,
        lhs: &HirExpression,
        rhs: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let (Type::Vector(lhs_elem, lhs_width), Type::Vector(_, rhs_width)) =
            (self.normalized_type(lhs.ty?).clone(), self.normalized_type(rhs.ty?).clone())
        else {
            self.errors
                .push(MirError::unsupported(expr.span, "outer product requires vector operands"));
            return None;
        };
        let Some(rows) = radix_semantic::literal_vector_width(lhs_width, self.types) else {
            self.errors.push(MirError::unsupported(
                expr.span,
                "outer product with non-literal width before MIR lowering",
            ));
            return None;
        };
        let Some(columns) = radix_semantic::literal_vector_width(rhs_width, self.types) else {
            self.errors.push(MirError::unsupported(
                expr.span,
                "outer product with non-literal width before MIR lowering",
            ));
            return None;
        };
        // Register-matrix CPU subset: the result matrix must stay ≤4×4.
        if rows > 4 || columns > 4 {
            self.errors.push(MirError::unsupported(
                expr.span,
                "outer product matrix register exceeds 4x4 CPU subset",
            ));
            return None;
        }
        let lhs_operand = self.lower_expr_value(lhs)?;
        let lhs_place = self.value_base_place(lhs_operand, MirType::semantic(lhs.ty?), lhs.span);
        let rhs_operand = self.lower_expr_value(rhs)?;
        let rhs_place = self.value_base_place(rhs_operand, MirType::semantic(rhs.ty?), rhs.span);
        let elem_ty = MirType::semantic(lhs_elem);
        let matrix_ty = self.expr_ty(expr)?;
        let mut fields = Vec::with_capacity(usize::from(rows) * usize::from(columns));
        for row in 0..rows {
            for column in 0..columns {
                let lhs = vector_lane_operand(&lhs_place, row);
                let rhs = vector_lane_operand(&rhs_place, column);
                let value = self.assign_temp(MirValueKind::Binary { op: MirBinOp::Mul, lhs, rhs }, elem_ty, expr.span);
                fields.push(MirAggregateItem::Operand(value));
            }
        }
        self.construct_temp(
            MirAggregateKind::Matrix,
            MirAggregateFields::Ordered(fields),
            matrix_ty,
            expr.span,
        )
        .into()
    }

    fn lower_sermo_method(
        &mut self,
        receiver: &HirExpression,
        method_name: &str,
        args: &[HirCallArg],
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let receiver_ty = receiver.ty?;
        let receiver_def = match self.normalized_type(receiver_ty) {
            Type::Struct(def_id) => Some(*def_id),
            Type::Applied(base, _) => match self.normalized_type(*base) {
                Type::Struct(def_id) => Some(*def_id),
                _ => None,
            },
            _ => None,
        }?;
        let op = if Some(receiver_def) == self.context.sermo_def {
            match method_name {
                "meus" => radix_mir::MirSermoOp::ViewMeus,
                "tuus" => radix_mir::MirSermoOp::ViewTuus,
                "close" if args.is_empty() => return self.lower_sermo_close(receiver, expr),
                _ => return None,
            }
        } else if Some(receiver_def) == self.context.meus_def {
            match method_name {
                "sink" => radix_mir::MirSermoOp::Send,
                "flush" => radix_mir::MirSermoOp::Flush,
                "fini" => radix_mir::MirSermoOp::Finish,
                _ => return None,
            }
        } else if Some(receiver_def) == self.context.tuus_def {
            match method_name {
                "accipe" => radix_mir::MirSermoOp::Receive { frame: self.context.scrinium_def? },
                "cursor" => radix_mir::MirSermoOp::Cursor { frame: self.context.scrinium_def? },
                "fini" => radix_mir::MirSermoOp::Finish,
                _ => return None,
            }
        } else {
            return None;
        };
        let mut lowered_args = Vec::with_capacity(args.len() + 1);
        lowered_args.push(self.lower_expr_value(receiver)?);
        lowered_args.extend(self.lower_hir_call_args(args)?);
        let return_ty = self.expr_ty(expr)?;
        Some(self.runtime_call_value(MirIntrinsic::Sermo(op), lowered_args, return_ty, expr.span))
    }

    /// `sink ⇜ item` (need-5): the operator form of `sink.sink(item)`.
    ///
    /// A channel's send view is a sink, so `m ⇜ x` writes through the view by
    /// the same sermo send intrinsic `m.sink(x)` lowers to. Every other sink
    /// lowers to the sinking contract's `sink` method — the reporting method
    /// form; the die-on-failure trap around it is unit 0c4d3dc1-5.2.
    pub(super) fn lower_sink_into(
        &mut self,
        sink: &HirExpression,
        item: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let span = expr.span;
        let vacuum = self.expr_ty(expr)?;
        let Some(sink_ty) = sink.ty else {
            self.errors
                .push(MirError::missing_type(span, "sink-into receiver"));
            return None;
        };

        // A send view writes through the channel: `m ⇜ x` is `m.sink(x)`.
        if self
            .context
            .meus_def
            .is_some_and(|meus| self.method_receiver_struct_def(sink_ty) == Some(meus))
        {
            let view = self.lower_expr_value(sink)?;
            let item = self.lower_expr_value(item)?;
            return Some(self.runtime_call_value(
                MirIntrinsic::Sermo(radix_mir::MirSermoOp::Send),
                vec![view, item],
                vacuum,
                span,
            ));
        }

        // A list or set receiver has no `sink` method: the write is the
        // collection's one shared append (`appende` / `adde`). A bounded list
        // dies on overflow, so its append runs through the bounded helper
        // inside the same die trap the method form uses.
        let payload = self
            .types
            .get(self.method_receiver_payload_id(sink_ty))
            .clone();
        match payload {
            Type::Array(_) | Type::Set(_) | Type::RangedArray { max: None, .. } => {
                let receiver = self.lower_expr_value(sink)?;
                let item = self.lower_expr_value(item)?;
                return Some(self.runtime_call_value(
                    MirIntrinsic::Collection(MirCollectionOp::Append),
                    vec![receiver, item],
                    vacuum,
                    span,
                ));
            }
            Type::RangedArray { max: Some(max), .. } => {
                // A push past the maximum of a length range dies on `⇜`, like a
                // bounded list: the checked helper runs inside the die trap.
                let receiver_ty = MirType::semantic(self.method_receiver_payload_id(sink_ty));
                let item_ty = self.expr_ty(item)?;
                let textus = MirType::semantic(self.types.primitive(Primitive::Textus));
                let helper = self.push_ranged_list_helper(
                    MirCollectionOp::Append,
                    receiver_ty,
                    Some(item_ty),
                    max,
                    vacuum,
                    textus,
                    span,
                );
                let receiver = self.lower_expr_value(sink)?;
                let item = self.lower_expr_value(item)?;
                return self.lower_dying_failable_call(
                    MirCallee::Function(helper),
                    vec![receiver, item],
                    vacuum,
                    textus,
                    span,
                );
            }
            Type::BoundedArray { .. } => {
                let receiver_ty = MirType::semantic(self.method_receiver_payload_id(sink_ty));
                let item_ty = self.expr_ty(item)?;
                let textus = MirType::semantic(self.types.primitive(Primitive::Textus));
                let helper = self.push_bounded_append_helper(receiver_ty, item_ty, vacuum, textus, span);
                let receiver = self.lower_expr_value(sink)?;
                let item = self.lower_expr_value(item)?;
                return self.lower_dying_failable_call(
                    MirCallee::Function(helper),
                    vec![receiver, item],
                    vacuum,
                    textus,
                    span,
                );
            }
            _ => {}
        }

        let Some(method) = self
            .context
            .interner
            .and_then(|interner| interner.lookup(radix_semantic::radix_contract::SINKING_METHOD))
        else {
            self.errors.push(MirError::unsupported(
                span,
                "sink-into before the sinking contract's method name is interned",
            ));
            return None;
        };

        // The `sink` method the receiver resolves to, when that route carries a
        // MIR error channel. A route without one (an imported provider sink)
        // cannot fail, so the method form is already the whole lowering.
        let target = self
            .genus_method_target(sink, method)
            .map(|target| target.def_id)
            .or_else(|| self.bound_method_target(sink, method))
            .and_then(|def_id| {
                self.context
                    .function_errors
                    .get(&def_id)
                    .map(|err_ty| (def_id, *err_ty))
            });
        let Some((def_id, err_ty)) = target else {
            let argument = HirCallArg { name: None, spread: false, marker: None, expr: item.clone(), span: item.span };
            return self.lower_method_call(sink, method, &[], &[argument], None, expr);
        };

        // Both operands evaluate once, under the ambient error routing, before
        // the call itself is wrapped in its die trap.
        let receiver = self.lower_expr_value(sink)?;
        let item = self.lower_expr_value(item)?;
        let call_ty = self.expr_ty(expr)?;
        self.lower_dying_failable_call(MirCallee::definition(def_id), vec![receiver, item], call_ty, err_ty, span)
    }

    /// A failable call whose failure is a death: the call runs inside its own
    /// trap so its error edge lands on a block that panics instead of leaving
    /// through an enclosing `fac`/`cape` handler or the unit's `⇥` channel.
    ///
    /// The innermost trap wins ([`Self::take_innermost_trap`]), which is what
    /// keeps a dying `⇜` from being routed to a surrounding handler.
    fn lower_dying_failable_call(
        &mut self,
        callee: MirCallee,
        args: Vec<MirOperand>,
        ty: MirType,
        err_ty: MirType,
        span: Span,
    ) -> Option<MirOperand> {
        let error_temp = self.push_temp(err_ty, span);
        let fail_block = self.fresh_block(span);
        let join = self.fresh_block(span);
        self.capture_depth += 1;
        self.traps.push(TrapContext {
            destination: MirPlace::temp(error_temp),
            join_block: fail_block,
            error_paths: false,
            depth: self.capture_depth,
        });
        let call = self.lower_failable_call(callee, args, ty, err_ty, None, span);
        let error_paths = self.traps.pop().is_some_and(|trap| trap.error_paths);
        let call_reaches = call.is_some() && self.terminate_open_current(MirTerminatorKind::Goto(join), span);
        if error_paths {
            self.switch_to(fail_block);
            self.die_on_failure(MirOperand::Temp(error_temp), span);
        } else {
            self.seal_unreachable(fail_block, span);
        }
        if call_reaches {
            self.switch_to(join);
        } else {
            self.seal_unreachable(join, span);
        }
        call.map(|_| MirOperand::Constant(MirConstant::Unit))
    }

    /// The operator death terminal: panic on the failure payload, then
    /// unreachable. Mirrors `mori`'s terminal, which is where a `⇜` failure
    /// ends when the unit declares no `⇥` channel.
    fn die_on_failure(&mut self, error: MirOperand, span: Span) {
        let numquam = MirType::semantic(self.types.primitive(Primitive::Numquam));
        self.append_stmt(MirStatement {
            kind: MirStatementKind::RuntimeCall {
                destination: None,
                call: MirRuntimeCall { intrinsic: MirIntrinsic::Panic, args: vec![error], return_ty: numquam },
            },
            span,
        });
        self.terminate_current(MirTerminatorKind::Unreachable, span);
    }

    /// `s.close()` (D8.3): close, then route a reported error through the
    /// innermost alternate exit like `iace` with a `textus` payload.
    fn lower_sermo_close(&mut self, receiver: &HirExpression, expr: &HirExpression) -> Option<MirOperand> {
        let span = expr.span;
        let sermo = self.lower_expr_value(receiver)?;
        let bool_ty = MirType::semantic(self.types.primitive(Primitive::Bivalens));
        let textus = MirType::semantic(self.types.primitive(Primitive::Textus));
        let sermo_op = |op| MirIntrinsic::Sermo(op);
        let closed =
            self.runtime_call_value(sermo_op(radix_mir::MirSermoOp::Close), vec![sermo.clone()], bool_ty, span);
        let failed =
            self.runtime_call_value(sermo_op(radix_mir::MirSermoOp::CloseFailed), vec![sermo.clone()], bool_ty, span);
        let fail_id = self.fresh_block(span);
        let ok_id = self.fresh_block(span);
        self.terminate_current(
            MirTerminatorKind::Branch { condition: failed, then_block: fail_id, else_block: ok_id },
            span,
        );
        self.switch_to(fail_id);
        let message = self.runtime_call_value(sermo_op(radix_mir::MirSermoOp::CloseError), vec![sermo], textus, span);
        self.fail_with_textus(message, span);
        self.switch_to(ok_id);
        Some(closed)
    }

    fn lower_atomic_method(
        &mut self,
        receiver: &HirExpression,
        method_name: &str,
        args: &[HirCallArg],
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        // Atomic mutation requires a `mut` receiver (SEM020), so the receiver
        // type usually carries the access marker; look through it.
        let payload_ty = self.method_receiver_payload_id(receiver.ty?);
        if !matches!(self.normalized_type(payload_ty), Type::Atomic(_)) {
            return None;
        }
        let entry =
            radix_intrinsics::lookup_method_for_type(self.normalized_type(payload_ty), method_name, args.len())?;
        let op = radix_intrinsics::mir_atomic_op(entry)?;
        let mut lowered_args = Vec::with_capacity(args.len() + 1);
        lowered_args.push(self.lower_expr_value(receiver)?);
        lowered_args.extend(self.lower_hir_call_args(args)?);
        let ty = self.expr_ty(expr)?;
        Some(self.runtime_call_value(MirIntrinsic::Atomic(op), lowered_args, ty, expr.span))
    }

    /// Lower a vector method to one deferred [`MirIntrinsic::VectorOp`].
    ///
    /// Every vector method defers, at literal and symbolic widths alike: the
    /// lane rules live in `radix_mir::vector_expand`, which runs once a
    /// function has concrete types. The outer `Option` is `None` when the call
    /// is not a vector method (the caller keeps resolving); `Some(None)` means
    /// a diagnostic was recorded and the call is handled.
    fn lower_vector_method(
        &mut self,
        receiver: &HirExpression,
        method_name: &str,
        args: &[HirCallArg],
        expr: &HirExpression,
    ) -> Option<Option<MirOperand>> {
        let method = vector_method_kind(method_name)?;
        let receiver_ty = receiver.ty?;
        let Type::Vector(element_ty, _) = self.normalized_type(receiver_ty).clone() else {
            return None;
        };
        let vector_ty = MirType::semantic(receiver_ty);
        let (op, peer_args): (MirVectorOp, &[HirCallArg]) = match method {
            VectorMethodKind::Crea
            | VectorMethodKind::Longitudo
            | VectorMethodKind::Norm
            | VectorMethodKind::Normalizata => {
                if !args.is_empty() {
                    return None;
                }
                let op = match method {
                    VectorMethodKind::Crea => MirVectorOp::Crea,
                    VectorMethodKind::Longitudo => MirVectorOp::Count,
                    VectorMethodKind::Norm => MirVectorOp::Norm,
                    _ => MirVectorOp::Normalized,
                };
                (op, &[])
            }
            VectorMethodKind::Swizzle => {
                let [arg] = args else {
                    return None;
                };
                let Some(lanes) = self.vector_swizzle_pattern(&arg.expr, expr) else {
                    return Some(None);
                };
                (MirVectorOp::Swizzle(lanes), &[])
            }
            VectorMethodKind::Elementwise(_) | VectorMethodKind::Cross | VectorMethodKind::Dot => {
                if args.len() != 1 {
                    return None;
                }
                let op = match method {
                    VectorMethodKind::Elementwise(op) => {
                        // Each lane is stored under the element type at the
                        // operation (ruling 19(g)), as for the scalar lift.
                        let checked = self.register_lanes_are_checked(expr);
                        let peer_is_vector = args[0].expr.ty.is_some_and(|ty| {
                            matches!(self.normalized_type(self.types.peel_access_marker(ty).1), Type::Vector(..))
                        });
                        if peer_is_vector {
                            MirVectorOp::Elementwise { op, checked }
                        } else {
                            MirVectorOp::ElementwiseScalar { op, scalar_first: false, checked }
                        }
                    }
                    VectorMethodKind::Cross => MirVectorOp::Cross,
                    _ => MirVectorOp::Dot,
                };
                (op, args)
            }
        };

        let mut operands = Vec::with_capacity(op.arity());
        let Some(lhs) = self.lower_expr_value(receiver) else {
            return Some(None);
        };
        operands.push(self.vector_operand_place(lhs, vector_ty, receiver.span));
        if let [arg] = peer_args {
            let Some(rhs) = self.lower_expr_value(&arg.expr) else {
                return Some(None);
            };
            let rhs_ty = arg.expr.ty.map_or(vector_ty, MirType::semantic);
            operands.push(self.vector_operand_place(rhs, rhs_ty, arg.expr.span));
        }
        let result_ty = if op.returns_vector() {
            vector_ty
        } else if matches!(op, MirVectorOp::Count) {
            self.expr_ty(expr)?
        } else {
            MirType::semantic(element_ty)
        };
        Some(Some(self.runtime_call_value(
            MirIntrinsic::VectorOp(op),
            operands,
            result_ty,
            expr.span,
        )))
    }

    /// A vector operand the expander can read lanes from: a temp or an
    /// unprojected place. Anything else (a projected place, a value, a
    /// constant) is copied into a temp first.
    pub(super) fn vector_operand_place(&mut self, operand: MirOperand, ty: MirType, span: Span) -> MirOperand {
        match operand {
            MirOperand::Temp(_) => operand,
            MirOperand::Place(ref place) if place.projections.is_empty() => operand,
            MirOperand::Place(_) | MirOperand::Value(_) | MirOperand::Constant(_) => {
                let temp = self.push_temp(ty, span);
                self.assign(MirPlace::temp(temp), operand, ty, span);
                MirOperand::Temp(temp)
            }
        }
    }

    /// Resolve a swizzle pattern literal to source lane indices. The pattern
    /// length and lane bounds depend on the receiver width, so the expander
    /// checks them once the width is a literal.
    fn vector_swizzle_pattern(&mut self, pattern_expr: &HirExpression, expr: &HirExpression) -> Option<Vec<u8>> {
        let Some(interner) = self.context.interner else {
            self.errors
                .push(MirError::unsupported(expr.span, "vector swizzle interner before MIR lowering"));
            return None;
        };
        let HirExpressionKind::Literal(HirLiteral::String(pattern)) = &pattern_expr.kind else {
            self.errors.push(MirError::unsupported(
                pattern_expr.span,
                "vector swizzle literal before MIR lowering",
            ));
            return None;
        };
        let lanes = interner
            .resolve(*pattern)
            .chars()
            .map(vector_swizzle_lane)
            .collect::<Option<Vec<_>>>();
        if lanes.is_none() {
            self.errors.push(MirError::unsupported(
                pattern_expr.span,
                "vector swizzle lane before MIR lowering",
            ));
        }
        lanes
    }

    fn lower_matrix_method(
        &mut self,
        receiver: &HirExpression,
        method_name: &str,
        args: &[HirCallArg],
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        // WHY: the coordinate methods (`accipe` / `apud` /
        // `ponde`) are receiver-generic registry rows that lower through the
        // `TensorGet` / `TensorSet` collection op, not the register-matrix
        // method path below (which rejects non-literal shapes and >4x4 before
        // it ever reads the method name). Fall through to the generic
        // method-intrinsic dispatch.
        // `longitudo` (the element count, Q-X1) takes the same generic route
        // through the `Length` collection op.
        if matches!(method_name, "accipe" | "apud" | "ponde" | "longitudo") {
            return None;
        }
        let receiver_ty = receiver.ty?;
        let Type::Matrix(element_ty, shape) = self.normalized_type(receiver_ty).clone() else {
            return None;
        };
        let Some((rows, columns)) = radix_semantic::literal_matrix_shape(shape, self.types) else {
            self.errors.push(MirError::unsupported(
                expr.span,
                "matrix method with non-literal shape before MIR lowering",
            ));
            return None;
        };
        // Reject oversized register matrices before emission (CPU subset: ≤4×4).
        if rows > 4 || columns > 4 {
            self.errors.push(MirError::unsupported(
                expr.span,
                "matrix register method exceeds 4x4 CPU subset",
            ));
            return None;
        }
        let lhs = self.lower_expr_value(receiver)?;
        let matrix_ty = MirType::semantic(receiver_ty);
        let lhs = self.value_base_place(lhs, matrix_ty, receiver.span);
        let elem_ty = MirType::semantic(element_ty);

        if method_name == "inversa" {
            if !args.is_empty() {
                return None;
            }
            let result_ty = self.expr_ty(expr)?;
            return Some(self.lower_matrix_inversa(&lhs, rows, columns, elem_ty, matrix_ty, result_ty, expr));
        }
        if method_name == "transpone" {
            if !args.is_empty() {
                return None;
            }
            let result_ty = self.expr_ty(expr)?;
            return Some(self.lower_matrix_transpone(&lhs, rows, columns, result_ty, expr));
        }

        let [arg] = args else {
            return None;
        };
        let rhs = self.lower_expr_value(&arg.expr)?;
        let rhs_ty = arg.expr.ty.map_or(matrix_ty, MirType::semantic);
        let rhs = self.value_base_place(rhs, rhs_ty, arg.expr.span);
        if method_name == "matmul" {
            // Full matmul needs the peer's literal [K,N] shape: the result is
            // [M,N], never the receiver's own [M,K] (which would silently drop
            // RHS columns for non-square products).
            let Some(peer_ty) = arg.expr.ty else {
                self.errors.push(MirError::unsupported(
                    expr.span,
                    "matrix matmul peer without a type before MIR lowering",
                ));
                return None;
            };
            let Type::Matrix(_, peer_shape) = self.normalized_type(peer_ty).clone() else {
                self.errors.push(MirError::unsupported(
                    expr.span,
                    "matrix matmul peer must be a matrix before MIR lowering",
                ));
                return None;
            };
            let Some((rhs_rows, rhs_columns)) = radix_semantic::literal_matrix_shape(peer_shape, self.types) else {
                self.errors.push(MirError::unsupported(
                    expr.span,
                    "matrix matmul peer with non-literal shape before MIR lowering",
                ));
                return None;
            };
            return Some(self.lower_matrix_matmul(&lhs, &rhs, rows, columns, rhs_rows, rhs_columns, elem_ty, expr));
        }
        if method_name == "applica" {
            // Peer is vector width = columns; result is vector width = rows.
            let arg_ty = arg.expr.ty?;
            let Type::Vector(_, width) = self.normalized_type(arg_ty).clone() else {
                self.errors.push(MirError::unsupported(
                    expr.span,
                    "matrix applica requires vector peer before MIR lowering",
                ));
                return None;
            };
            #[expect(
                clippy::cast_possible_truncation,
                reason = "matrix peer vector width literal is bounded to 1..=4 by the pattern"
            )]
            let peer_width = if let IndexExpr::Literal(w @ 1..=4) = self.types.get_index(width) {
                *w as u8
            } else {
                self.errors.push(MirError::unsupported(
                    expr.span,
                    "matrix applica vector width before MIR lowering",
                ));
                return None;
            };
            if peer_width != columns {
                self.errors.push(MirError::unsupported(
                    expr.span,
                    "matrix applica vector width must match matrix columns",
                ));
                return None;
            }
            let result_ty = self.expr_ty(expr)?;
            return Some(self.lower_matrix_applica(&lhs, &rhs, rows, columns, elem_ty, result_ty, expr));
        }
        let op = matrix_method_kind(method_name)?;
        Some(self.lower_matrix_elementwise_op(lhs, rhs, (rows, columns), op, (elem_ty, matrix_ty), expr))
    }

    /// Flatten nested `[[…], …]` lista literal into row-major matrix cells.
    fn lower_matrix_from_nested_lista(
        &mut self,
        elements: &[HirArrayElement],
        rows: u8,
        columns: u8,
        ty: MirType,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        if rows > 4 || columns > 4 {
            self.errors.push(MirError::unsupported(
                expr.span,
                "matrix register materialize exceeds 4x4 CPU subset",
            ));
            return None;
        }
        if elements.len() != usize::from(rows) {
            self.errors.push(MirError::unsupported(
                expr.span,
                "matrix materialize row count mismatch before MIR lowering",
            ));
            return None;
        }
        let mut fields = Vec::with_capacity(usize::from(rows) * usize::from(columns));
        for row in elements {
            let HirArrayElement::Expr(row_expr) = row else {
                self.errors.push(MirError::unsupported(
                    expr.span,
                    "matrix materialize forbids spreads before MIR lowering",
                ));
                return None;
            };
            let HirExpressionKind::Array(cells) = &row_expr.kind else {
                self.errors.push(MirError::unsupported(
                    expr.span,
                    "matrix materialize requires nested lista rows before MIR lowering",
                ));
                return None;
            };
            if cells.len() != usize::from(columns) {
                self.errors.push(MirError::unsupported(
                    expr.span,
                    "matrix materialize column count mismatch before MIR lowering",
                ));
                return None;
            }
            for cell in cells {
                let HirArrayElement::Expr(cell_expr) = cell else {
                    self.errors.push(MirError::unsupported(
                        expr.span,
                        "matrix materialize forbids spreads before MIR lowering",
                    ));
                    return None;
                };
                fields.push(MirAggregateItem::Operand(self.lower_expr_value(cell_expr)?));
            }
        }
        Some(self.construct_temp(MirAggregateKind::Matrix, MirAggregateFields::Ordered(fields), ty, expr.span))
    }

    /// Expand vector·matrix (row-vector convention) into lane×cell mul/add and
    /// a vector construct: result lane `j` = sum over `k` of `v[k] * m[k, j]`.
    fn lower_vector_matrix_dot(
        &mut self,
        lhs: &HirExpression,
        rhs: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let (lhs_ty_id, rhs_ty_id) = (lhs.ty?, rhs.ty?);
        let Type::Vector(_, width) = self.normalized_type(lhs_ty_id).clone() else {
            self.errors
                .push(MirError::unsupported(expr.span, "vector·matrix requires a vector receiver"));
            return None;
        };
        let Type::Matrix(element_ty, shape) = self.normalized_type(rhs_ty_id).clone() else {
            self.errors
                .push(MirError::unsupported(expr.span, "vector·matrix requires a matrix peer"));
            return None;
        };
        let Some(lanes) = radix_semantic::literal_vector_width(width, self.types) else {
            self.errors.push(MirError::unsupported(
                expr.span,
                "vector·matrix vector width before MIR lowering",
            ));
            return None;
        };
        let Some((rows, columns)) = radix_semantic::literal_matrix_shape(shape, self.types) else {
            self.errors.push(MirError::unsupported(
                expr.span,
                "vector·matrix with non-literal matrix shape before MIR lowering",
            ));
            return None;
        };
        if rows > 4 || columns > 4 {
            self.errors
                .push(MirError::unsupported(expr.span, "vector·matrix exceeds 4x4 CPU subset"));
            return None;
        }
        if lanes != rows {
            self.errors.push(MirError::unsupported(
                expr.span,
                "vector·matrix vector width must match matrix rows",
            ));
            return None;
        }
        let vector = self.lower_expr_value(lhs)?;
        let vector = self.value_base_place(vector, MirType::semantic(lhs_ty_id), lhs.span);
        let matrix = self.lower_expr_value(rhs)?;
        let matrix = self.value_base_place(matrix, MirType::semantic(rhs_ty_id), rhs.span);
        let elem_ty = MirType::semantic(element_ty);
        let result_ty = self.expr_ty(expr)?;
        let mut fields = Vec::with_capacity(usize::from(columns));
        for column in 0..columns {
            let mut acc = self.assign_temp(
                MirValueKind::Binary {
                    op: MirBinOp::Mul,
                    lhs: vector_lane_operand(&vector, 0),
                    rhs: matrix_cell_operand(&matrix, 0, column),
                },
                elem_ty,
                expr.span,
            );
            for row in 1..rows {
                let product = self.assign_temp(
                    MirValueKind::Binary {
                        op: MirBinOp::Mul,
                        lhs: vector_lane_operand(&vector, row),
                        rhs: matrix_cell_operand(&matrix, row, column),
                    },
                    elem_ty,
                    expr.span,
                );
                acc = self.assign_temp(
                    MirValueKind::Binary { op: MirBinOp::Add, lhs: acc, rhs: product },
                    elem_ty,
                    expr.span,
                );
            }
            fields.push(MirAggregateItem::Operand(acc));
        }
        Some(self.construct_temp(
            MirAggregateKind::Vector,
            MirAggregateFields::Ordered(fields),
            result_ty,
            expr.span,
        ))
    }

    /// Expand matrix×vector product into cell×lane mul/add and a vector construct.
    fn lower_matrix_applica(
        &mut self,
        matrix: &MirPlace,
        vector: &MirPlace,
        rows: u8,
        columns: u8,
        elem_ty: MirType,
        vector_ty: MirType,
        expr: &HirExpression,
    ) -> MirOperand {
        let mut fields = Vec::with_capacity(usize::from(rows));
        for row in 0..rows {
            let mut acc = self.assign_temp(
                MirValueKind::Binary {
                    op: MirBinOp::Mul,
                    lhs: matrix_cell_operand(matrix, row, 0),
                    rhs: vector_lane_operand(vector, 0),
                },
                elem_ty,
                expr.span,
            );
            for column in 1..columns {
                let product = self.assign_temp(
                    MirValueKind::Binary {
                        op: MirBinOp::Mul,
                        lhs: matrix_cell_operand(matrix, row, column),
                        rhs: vector_lane_operand(vector, column),
                    },
                    elem_ty,
                    expr.span,
                );
                acc = self.assign_temp(
                    MirValueKind::Binary { op: MirBinOp::Add, lhs: acc, rhs: product },
                    elem_ty,
                    expr.span,
                );
            }
            fields.push(MirAggregateItem::Operand(acc));
        }
        self.construct_temp(
            MirAggregateKind::Vector,
            MirAggregateFields::Ordered(fields),
            vector_ty,
            expr.span,
        )
    }

    /// Matrix transpose: `[R, C] → [C, R]`. Result cell `(c, r)` reads
    /// receiver cell `(r, c)` — a pure operand permutation, no arithmetic.
    fn lower_matrix_transpone(
        &mut self,
        matrix: &MirPlace,
        rows: u8,
        columns: u8,
        result_ty: MirType,
        expr: &HirExpression,
    ) -> MirOperand {
        let mut fields = Vec::with_capacity(usize::from(rows) * usize::from(columns));
        for column in 0..columns {
            for row in 0..rows {
                fields.push(MirAggregateItem::Operand(matrix_cell_operand(matrix, row, column)));
            }
        }
        self.construct_temp(
            MirAggregateKind::Matrix,
            MirAggregateFields::Ordered(fields),
            result_ty,
            expr.span,
        )
    }

    /// Square matrix inverse with singular → nihil (Option).
    ///
    /// - 2×2: full inverse via determinant
    /// - 4×4: affine upper-3×3 inverse matching Triga `matrix4_inversa_affinis`
    ///
    /// Other ranks reject before emission.
    fn lower_matrix_inversa(
        &mut self,
        matrix: &MirPlace,
        rows: u8,
        columns: u8,
        elem_ty: MirType,
        matrix_ty: MirType,
        option_ty: MirType,
        expr: &HirExpression,
    ) -> MirOperand {
        if rows != columns {
            self.errors
                .push(MirError::unsupported(expr.span, "matrix inversa requires square shape"));
            return self.assign_temp(MirValueKind::Option(MirOptionOp::None), option_ty, expr.span);
        }
        match rows {
            2 => self.lower_matrix_inversa_2x2(matrix, elem_ty, matrix_ty, option_ty, expr),
            4 => self.lower_matrix_inversa_affine_4x4(matrix, elem_ty, matrix_ty, option_ty, expr),
            _ => {
                self.errors.push(MirError::unsupported(
                    expr.span,
                    "matrix inversa CPU subset admits only 2x2 and affine 4x4",
                ));
                self.assign_temp(MirValueKind::Option(MirOptionOp::None), option_ty, expr.span)
            }
        }
    }

    fn lower_matrix_inversa_2x2(
        &mut self,
        matrix: &MirPlace,
        elem_ty: MirType,
        matrix_ty: MirType,
        option_ty: MirType,
        expr: &HirExpression,
    ) -> MirOperand {
        let a = matrix_cell_operand(matrix, 0, 0);
        let b = matrix_cell_operand(matrix, 0, 1);
        let c = matrix_cell_operand(matrix, 1, 0);
        let d = matrix_cell_operand(matrix, 1, 1);
        let ad = self.assign_temp(
            MirValueKind::Binary { op: MirBinOp::Mul, lhs: a.clone(), rhs: d.clone() },
            elem_ty,
            expr.span,
        );
        let bc = self.assign_temp(
            MirValueKind::Binary { op: MirBinOp::Mul, lhs: b.clone(), rhs: c.clone() },
            elem_ty,
            expr.span,
        );
        let det = self.assign_temp(MirValueKind::Binary { op: MirBinOp::Sub, lhs: ad, rhs: bc }, elem_ty, expr.span);
        self.lower_matrix_inversa_with_det(det, elem_ty, option_ty, expr, |this, inv_det| {
            this.lower_matrix_2x2_inverse_with_det(a, b, c, d, inv_det, elem_ty, matrix_ty, expr)
        })
    }

    /// Affine 4×4 inverse (upper 3×3 + translation), Triga column-vector layout
    /// expressed with row-major language cells.
    fn lower_matrix_inversa_affine_4x4(
        &mut self,
        matrix: &MirPlace,
        elem_ty: MirType,
        matrix_ty: MirType,
        option_ty: MirType,
        expr: &HirExpression,
    ) -> MirOperand {
        // Upper-left 3×3 cells (row-major language matrix).
        let a00 = matrix_cell_operand(matrix, 0, 0);
        let a01 = matrix_cell_operand(matrix, 0, 1);
        let a02 = matrix_cell_operand(matrix, 0, 2);
        let a10 = matrix_cell_operand(matrix, 1, 0);
        let a11 = matrix_cell_operand(matrix, 1, 1);
        let a12 = matrix_cell_operand(matrix, 1, 2);
        let a20 = matrix_cell_operand(matrix, 2, 0);
        let a21 = matrix_cell_operand(matrix, 2, 1);
        let a22 = matrix_cell_operand(matrix, 2, 2);
        let tx = matrix_cell_operand(matrix, 0, 3);
        let ty = matrix_cell_operand(matrix, 1, 3);
        let tz = matrix_cell_operand(matrix, 2, 3);

        // det = a00*(a11*a22-a12*a21) - a01*(a10*a22-a12*a20) + a02*(a10*a21-a11*a20)
        let t0 = self.mul_sub_mul(&a11, &a22, &a12, &a21, elem_ty, expr.span);
        let t1 = self.mul_sub_mul(&a10, &a22, &a12, &a20, elem_ty, expr.span);
        let t2 = self.mul_sub_mul(&a10, &a21, &a11, &a20, elem_ty, expr.span);
        let p0 = self.assign_temp(
            MirValueKind::Binary { op: MirBinOp::Mul, lhs: a00.clone(), rhs: t0 },
            elem_ty,
            expr.span,
        );
        let p1 = self.assign_temp(
            MirValueKind::Binary { op: MirBinOp::Mul, lhs: a01.clone(), rhs: t1 },
            elem_ty,
            expr.span,
        );
        let p2 = self.assign_temp(
            MirValueKind::Binary { op: MirBinOp::Mul, lhs: a02.clone(), rhs: t2 },
            elem_ty,
            expr.span,
        );
        let det01 = self.assign_temp(MirValueKind::Binary { op: MirBinOp::Sub, lhs: p0, rhs: p1 }, elem_ty, expr.span);
        let det = self.assign_temp(
            MirValueKind::Binary { op: MirBinOp::Add, lhs: det01, rhs: p2 },
            elem_ty,
            expr.span,
        );

        self.lower_matrix_inversa_with_det(det, elem_ty, option_ty, expr, |this, inv_det| {
            this.lower_matrix_3x3_inverse_with_det(
                &a00, &a01, &a02, &a10, &a11, &a12, &a20, &a21, &a22, &tx, &ty, &tz, &inv_det, elem_ty, matrix_ty, expr,
            )
        })
    }

    /// Shared singular test: |det| ≤ 1e-6 → nihil; else build inverse via `build`.
    fn lower_matrix_inversa_with_det<F>(
        &mut self,
        det: MirOperand,
        elem_ty: MirType,
        option_ty: MirType,
        expr: &HirExpression,
        build: F,
    ) -> MirOperand
    where
        F: FnOnce(&mut Self, MirOperand) -> MirOperand,
    {
        let eps = MirOperand::Constant(MirConstant::Float(0.000_001));
        let neg_eps = MirOperand::Constant(MirConstant::Float(-0.000_001));
        let gt = self.assign_temp(
            MirValueKind::Binary { op: MirBinOp::Gt, lhs: det.clone(), rhs: eps },
            MirType::semantic(self.types.primitive(radix_semantic::Primitive::Bivalens)),
            expr.span,
        );
        let lt = self.assign_temp(
            MirValueKind::Binary { op: MirBinOp::Lt, lhs: det.clone(), rhs: neg_eps },
            MirType::semantic(self.types.primitive(radix_semantic::Primitive::Bivalens)),
            expr.span,
        );
        let invertible = self.assign_temp(
            MirValueKind::Binary { op: MirBinOp::Or, lhs: gt, rhs: lt },
            MirType::semantic(self.types.primitive(radix_semantic::Primitive::Bivalens)),
            expr.span,
        );

        let result = self.push_temp(option_ty, expr.span);
        let ok_id = self.fresh_block(expr.span);
        let nil_id = self.fresh_block(expr.span);
        let join_id = self.fresh_block(expr.span);
        self.terminate_current(
            MirTerminatorKind::Branch { condition: invertible, then_block: ok_id, else_block: nil_id },
            expr.span,
        );

        self.switch_to(ok_id);
        {
            let one = MirOperand::Constant(MirConstant::Float(1.0));
            let inv_det = self.assign_temp(
                MirValueKind::Binary { op: MirBinOp::Div, lhs: one, rhs: det },
                elem_ty,
                expr.span,
            );
            let inverse = build(self, inv_det);
            let some = self.assign_temp(MirValueKind::Option(MirOptionOp::Some(inverse)), option_ty, expr.span);
            let value = self.new_value(MirValueKind::Operand(some), option_ty, expr.span);
            self.append_stmt(MirStatement {
                kind: MirStatementKind::Assign { place: MirPlace::temp(result), value },
                span: expr.span,
            });
            self.terminate_current(MirTerminatorKind::Goto(join_id), expr.span);
        }

        self.switch_to(nil_id);
        {
            let none = self.assign_temp(MirValueKind::Option(MirOptionOp::None), option_ty, expr.span);
            let value = self.new_value(MirValueKind::Operand(none), option_ty, expr.span);
            self.append_stmt(MirStatement {
                kind: MirStatementKind::Assign { place: MirPlace::temp(result), value },
                span: expr.span,
            });
            self.terminate_current(MirTerminatorKind::Goto(join_id), expr.span);
        }

        self.switch_to(join_id);
        MirOperand::Temp(result)
    }

    /// Build the 2×2 matrix inverse given `inv_det` = 1/det.
    ///
    /// For matrix [[a, b], [c, d]], the inverse is:
    /// `[[ d, -b], [-c,  a]] * inv_det`
    fn lower_matrix_2x2_inverse_with_det(
        &mut self,
        a: MirOperand,
        b: MirOperand,
        c: MirOperand,
        d: MirOperand,
        inv_det: MirOperand,
        elem_ty: MirType,
        matrix_ty: MirType,
        expr: &HirExpression,
    ) -> MirOperand {
        let d_s = self.assign_temp(
            MirValueKind::Binary { op: MirBinOp::Mul, lhs: d, rhs: inv_det.clone() },
            elem_ty,
            expr.span,
        );
        let neg_b = self.assign_temp(MirValueKind::Unary { op: MirUnOp::Neg, operand: b }, elem_ty, expr.span);
        let neg_b_s = self.assign_temp(
            MirValueKind::Binary { op: MirBinOp::Mul, lhs: neg_b, rhs: inv_det.clone() },
            elem_ty,
            expr.span,
        );
        let neg_c = self.assign_temp(MirValueKind::Unary { op: MirUnOp::Neg, operand: c }, elem_ty, expr.span);
        let neg_c_scaled = self.assign_temp(
            MirValueKind::Binary { op: MirBinOp::Mul, lhs: neg_c, rhs: inv_det.clone() },
            elem_ty,
            expr.span,
        );
        let a_s = self.assign_temp(
            MirValueKind::Binary { op: MirBinOp::Mul, lhs: a, rhs: inv_det },
            elem_ty,
            expr.span,
        );
        self.construct_temp(
            MirAggregateKind::Matrix,
            MirAggregateFields::Ordered(vec![
                MirAggregateItem::Operand(d_s),
                MirAggregateItem::Operand(neg_b_s),
                MirAggregateItem::Operand(neg_c_scaled),
                MirAggregateItem::Operand(a_s),
            ]),
            matrix_ty,
            expr.span,
        )
    }

    /// Build the 4×4 affine inverse given `inv_det` = 1/det of the upper-left 3×3.
    ///
    /// Computes the 3×3 inverse (i00–i22), then the negated rotated translation
    /// (ntx–ntz), and assembles a 4×4 matrix in Triga column-vector layout.
    fn lower_matrix_3x3_inverse_with_det(
        &mut self,
        a00: &MirOperand,
        a01: &MirOperand,
        a02: &MirOperand,
        a10: &MirOperand,
        a11: &MirOperand,
        a12: &MirOperand,
        a20: &MirOperand,
        a21: &MirOperand,
        a22: &MirOperand,
        tx: &MirOperand,
        ty: &MirOperand,
        tz: &MirOperand,
        inv_det: &MirOperand,
        elem_ty: MirType,
        matrix_ty: MirType,
        expr: &HirExpression,
    ) -> MirOperand {
        let i00 = self.scaled_mul_sub_mul(a11, a22, a12, a21, inv_det, elem_ty, expr.span);
        let i01 = self.scaled_mul_sub_mul(a02, a21, a01, a22, inv_det, elem_ty, expr.span);
        let i02 = self.scaled_mul_sub_mul(a01, a12, a02, a11, inv_det, elem_ty, expr.span);
        let i10 = self.scaled_mul_sub_mul(a12, a20, a10, a22, inv_det, elem_ty, expr.span);
        let i11 = self.scaled_mul_sub_mul(a00, a22, a02, a20, inv_det, elem_ty, expr.span);
        let i12 = self.scaled_mul_sub_mul(a02, a10, a00, a12, inv_det, elem_ty, expr.span);
        let i20 = self.scaled_mul_sub_mul(a10, a21, a11, a20, inv_det, elem_ty, expr.span);
        let i21 = self.scaled_mul_sub_mul(a01, a20, a00, a21, inv_det, elem_ty, expr.span);
        let i22 = self.scaled_mul_sub_mul(a00, a11, a01, a10, inv_det, elem_ty, expr.span);

        // translation' = -R^{-1} * t
        let ntx = self.neg_dot3(&i00, &i01, &i02, tx, ty, tz, elem_ty, expr.span);
        let nty = self.neg_dot3(&i10, &i11, &i12, tx, ty, tz, elem_ty, expr.span);
        let ntz = self.neg_dot3(&i20, &i21, &i22, tx, ty, tz, elem_ty, expr.span);
        let zero = self.assign_temp(
            MirValueKind::Operand(MirOperand::Constant(MirConstant::Float(0.0))),
            elem_ty,
            expr.span,
        );
        let one = self.assign_temp(
            MirValueKind::Operand(MirOperand::Constant(MirConstant::Float(1.0))),
            elem_ty,
            expr.span,
        );
        self.construct_temp(
            MirAggregateKind::Matrix,
            MirAggregateFields::Ordered(vec![
                MirAggregateItem::Operand(i00),
                MirAggregateItem::Operand(i01),
                MirAggregateItem::Operand(i02),
                MirAggregateItem::Operand(ntx),
                MirAggregateItem::Operand(i10),
                MirAggregateItem::Operand(i11),
                MirAggregateItem::Operand(i12),
                MirAggregateItem::Operand(nty),
                MirAggregateItem::Operand(i20),
                MirAggregateItem::Operand(i21),
                MirAggregateItem::Operand(i22),
                MirAggregateItem::Operand(ntz),
                MirAggregateItem::Operand(zero.clone()),
                MirAggregateItem::Operand(zero.clone()),
                MirAggregateItem::Operand(zero),
                MirAggregateItem::Operand(one),
            ]),
            matrix_ty,
            expr.span,
        )
    }

    fn mul_sub_mul(
        &mut self,
        a: &MirOperand,
        b: &MirOperand,
        c: &MirOperand,
        d: &MirOperand,
        elem_ty: MirType,
        span: radix_lexer::Span,
    ) -> MirOperand {
        let ab = self.assign_temp(
            MirValueKind::Binary { op: MirBinOp::Mul, lhs: a.clone(), rhs: b.clone() },
            elem_ty,
            span,
        );
        let cd = self.assign_temp(
            MirValueKind::Binary { op: MirBinOp::Mul, lhs: c.clone(), rhs: d.clone() },
            elem_ty,
            span,
        );
        self.assign_temp(MirValueKind::Binary { op: MirBinOp::Sub, lhs: ab, rhs: cd }, elem_ty, span)
    }

    fn scaled_mul_sub_mul(
        &mut self,
        a: &MirOperand,
        b: &MirOperand,
        c: &MirOperand,
        d: &MirOperand,
        scale: &MirOperand,
        elem_ty: MirType,
        span: radix_lexer::Span,
    ) -> MirOperand {
        let raw = self.mul_sub_mul(a, b, c, d, elem_ty, span);
        self.assign_temp(
            MirValueKind::Binary { op: MirBinOp::Mul, lhs: raw, rhs: scale.clone() },
            elem_ty,
            span,
        )
    }

    fn neg_dot3(
        &mut self,
        r0: &MirOperand,
        r1: &MirOperand,
        r2: &MirOperand,
        x: &MirOperand,
        y: &MirOperand,
        z: &MirOperand,
        elem_ty: MirType,
        span: radix_lexer::Span,
    ) -> MirOperand {
        let p0 = self.assign_temp(
            MirValueKind::Binary { op: MirBinOp::Mul, lhs: r0.clone(), rhs: x.clone() },
            elem_ty,
            span,
        );
        let p1 = self.assign_temp(
            MirValueKind::Binary { op: MirBinOp::Mul, lhs: r1.clone(), rhs: y.clone() },
            elem_ty,
            span,
        );
        let p2 = self.assign_temp(
            MirValueKind::Binary { op: MirBinOp::Mul, lhs: r2.clone(), rhs: z.clone() },
            elem_ty,
            span,
        );
        let s01 = self.assign_temp(MirValueKind::Binary { op: MirBinOp::Add, lhs: p0, rhs: p1 }, elem_ty, span);
        let sum = self.assign_temp(MirValueKind::Binary { op: MirBinOp::Add, lhs: s01, rhs: p2 }, elem_ty, span);
        self.assign_temp(MirValueKind::Unary { op: MirUnOp::Neg, operand: sum }, elem_ty, span)
    }

    /// Expand matrix product into typed cell multiplies and adds (row-major cells).
    ///
    /// The receiver is [M,K] and the peer is [K,N]; the result is the full
    /// [M,N] grid contracted over K. A peer whose row count differs from the
    /// receiver's K fails closed — columns are never truncated.
    fn lower_matrix_matmul(
        &mut self,
        lhs: &MirPlace,
        rhs: &MirPlace,
        rows: u8,
        columns: u8,
        rhs_rows: u8,
        rhs_columns: u8,
        elem_ty: MirType,
        expr: &HirExpression,
    ) -> MirOperand {
        let Some(result_ty) = self.expr_ty(expr) else {
            return self.assign_temp(
                MirValueKind::Operand(MirOperand::Constant(MirConstant::Float(0.0))),
                elem_ty,
                expr.span,
            );
        };
        if rhs_rows != columns {
            self.errors.push(MirError::unsupported(
                expr.span,
                "matrix matmul inner dimension mismatch before MIR lowering",
            ));
            return self.assign_temp(
                MirValueKind::Operand(MirOperand::Constant(MirConstant::Float(0.0))),
                elem_ty,
                expr.span,
            );
        }
        // The declared result type must be exactly [rows, rhs_columns]; any
        // other shape means the semantic layer disagrees with the K
        // contraction, which the MIR lane fails closed on rather than
        // truncating or stretching the grid.
        let declared = match self.types.get(result_ty.semantic_id()) {
            Type::Matrix(_, shape) => radix_semantic::literal_matrix_shape(*shape, self.types),
            _ => None,
        };
        if !matches!(declared, Some((r, c)) if r == rows && c == rhs_columns) {
            self.errors.push(MirError::unsupported(
                expr.span,
                "matrix matmul result shape before MIR lowering",
            ));
            return self.assign_temp(
                MirValueKind::Operand(MirOperand::Constant(MirConstant::Float(0.0))),
                elem_ty,
                expr.span,
            );
        }
        if rows > 4 || rhs_columns > 4 {
            self.errors
                .push(MirError::unsupported(expr.span, "matrix matmul result exceeds 4x4 CPU subset"));
            return self.assign_temp(
                MirValueKind::Operand(MirOperand::Constant(MirConstant::Float(0.0))),
                elem_ty,
                expr.span,
            );
        }
        let checked = self.register_lanes_are_checked(expr);
        let mut fields = Vec::with_capacity(usize::from(rows) * usize::from(rhs_columns));
        for row in 0..rows {
            for column in 0..rhs_columns {
                let mut acc = self.assign_temp(
                    MirValueKind::Binary {
                        op: MirBinOp::Mul,
                        lhs: matrix_cell_operand(lhs, row, 0),
                        rhs: matrix_cell_operand(rhs, 0, column),
                    },
                    elem_ty,
                    expr.span,
                );
                for k in 1..columns {
                    let product = self.assign_temp(
                        MirValueKind::Binary {
                            op: MirBinOp::Mul,
                            lhs: matrix_cell_operand(lhs, row, k),
                            rhs: matrix_cell_operand(rhs, k, column),
                        },
                        elem_ty,
                        expr.span,
                    );
                    acc = self.assign_temp(
                        MirValueKind::Binary { op: MirBinOp::Add, lhs: acc, rhs: product },
                        elem_ty,
                        expr.span,
                    );
                }
                let mut lanes = super::register_lift::MatrixLanes { builder: self, span: expr.span };
                fields.push(MirAggregateItem::Operand(stored_lane(&mut lanes, acc, elem_ty, checked)));
            }
        }
        self.construct_temp(
            MirAggregateKind::Matrix,
            MirAggregateFields::Ordered(fields),
            result_ty,
            expr.span,
        )
    }

    fn lower_matrix_elementwise_op(
        &mut self,
        lhs: MirPlace,
        rhs: MirPlace,
        (rows, columns): (u8, u8),
        op: MirLaneOp,
        (elem_ty, matrix_ty): (MirType, MirType),
        expr: &HirExpression,
    ) -> MirOperand {
        // Each cell is exact and stored under the element type at the
        // operation (ruling 19(g)), as for the scalar lift.
        let lane_op = LaneOp {
            op,
            checked: self.register_lanes_are_checked(expr),
            lane_tys: (elem_ty, elem_ty),
            element: elem_ty,
            types: self.types,
        };
        self.matrix_lane_cells(
            &lane_op,
            (rows, columns),
            (&LaneSource::Cells(lhs), &LaneSource::Cells(rhs)),
            matrix_ty,
            expr.span,
        )
    }

    pub(super) fn value_base_place(&mut self, operand: MirOperand, ty: MirType, span: Span) -> MirPlace {
        match operand {
            MirOperand::Place(place) => place,
            MirOperand::Temp(id) => MirPlace::temp(id),
            MirOperand::Value(_) | MirOperand::Constant(_) => {
                let temp = self.push_temp(ty, span);
                self.assign(MirPlace::temp(temp), operand, ty, span);
                MirPlace::temp(temp)
            }
        }
    }

    /// Lower the `GI3-1` `SiLU` activation as existing tensor elementwise MIR.
    ///
    /// `silu(x) = x / (1 + exp(-x))`: `TensorNeg`, tensor Exp, `TensorAdd`, and
    /// `TensorDiv` are the complete composition. Keeping this expansion here
    /// means the registry admits the method without inventing `TensorSilu`.
    fn lower_tensor_silu(&mut self, receiver: &HirExpression, expr: &HirExpression) -> Option<MirOperand> {
        let tensor_ty = self.expr_ty(expr)?;
        let x = self.lower_expr_value(receiver)?;
        let negated = self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::TensorNeg),
            vec![x.clone()],
            tensor_ty,
            expr.span,
        );
        let exponent =
            self.assign_temp(MirValueKind::Unary { op: MirUnOp::Exp, operand: negated }, tensor_ty, expr.span);
        let denominator = self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::TensorAdd),
            vec![exponent, MirOperand::Constant(MirConstant::Float(1.0))],
            tensor_ty,
            expr.span,
        );
        Some(self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::TensorDiv),
            vec![x, denominator],
            tensor_ty,
            expr.span,
        ))
    }

    /// Lower `potentia`, the one numeric method twin with no operator.
    ///
    /// WHY: every other numeric twin (`addita`, `adde`, `negativa`, ...) is
    /// rewritten to its operator or assignment during typecheck (ss1), so it
    /// reaches lowering as an ordinary node. `potentia` composes from existing
    /// MIR ops so the runner never sees a late provider method call.
    fn lower_numeric_operator_method(
        &mut self,
        receiver: &HirExpression,
        method_name: &str,
        args: &[HirCallArg],
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        if method_name != "potentia" {
            return None;
        }
        let receiver_ty = receiver.ty?;
        let normalized = self.normalized_type(receiver_ty);
        let entry = radix_intrinsics::lookup_method_for_type(normalized, method_name, args.len())?;
        if !matches!(entry.receiver, radix_intrinsics::IntrinsicReceiver::Numeric) {
            return None;
        }
        self.lower_potentia(receiver, receiver_ty, args, expr)
    }

    /// Lower `potentia` by receiver family.
    fn lower_potentia(
        &mut self,
        receiver: &HirExpression,
        receiver_ty: TypeId,
        args: &[HirCallArg],
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        match self.normalized_type(receiver_ty) {
            Type::Primitive(Primitive::Numerus) | Type::SizedNumeric(Primitive::Numerus, _) => {
                self.lower_integer_potentia(receiver, receiver_ty, args, expr)
            }
            Type::Primitive(Primitive::Fractus) | Type::SizedNumeric(Primitive::Fractus, _) => {
                self.lower_fractus_potentia(receiver, args, expr)
            }
            _ => None,
        }
    }

    /// Lower integer `potentia` with exponentiation by repeated squaring.
    ///
    /// Shape: `accumulator = 1`; while `exponent > 0`, multiply the
    /// accumulator for an odd exponent, halve the exponent, then square the
    /// base only when another iteration remains. This is O(log exponent) in
    /// multiplications, preserves negative-base signs without a float round
    /// trip, and keeps overflow under checked integer MIR arithmetic. A
    /// negative exponent traps before the loop (ruling 28).
    fn lower_integer_potentia(
        &mut self,
        receiver: &HirExpression,
        receiver_ty: TypeId,
        args: &[HirCallArg],
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let ty = self.expr_ty(expr)?;
        let [arg] = args else {
            return None;
        };
        let decimal = self.is_decimal_receiver(receiver_ty);
        let numerus_ty = MirType::semantic(self.types.primitive(Primitive::Numerus));
        let base = self.lower_expr_value(receiver)?;
        let argument = self.lower_expr_value(&arg.expr)?;
        let argument_ty = self.expr_ty(&arg.expr)?;
        let destination = self.push_temp(ty, expr.span);
        let accumulator = self.next_local_id();
        self.locals
            .push(MirLocalDecl { id: accumulator, name: None, ty, mutable: true, span: expr.span });
        let base_local = self.next_local_id();
        self.locals
            .push(MirLocalDecl { id: base_local, name: None, ty, mutable: true, span: expr.span });
        // The exponent keeps its own type: the result type is the range of
        // `base^exponent` and can be narrower than the exponent's type. A
        // decimal (`d64`) receiver instead runs the exponent at plain
        // `numerus`, because the by-squaring loop's Mod/Div literals are
        // small integer counts and a bare `Int(2)` under a width-scaled
        // carrier is 2e-8 — halving by it would grow the exponent until the
        // carrier overflowed. The exponent is a small count and d64 can
        // never exceed numerus, so the one-time conversion is exact.
        let (exponent_ty, exponent) = if decimal {
            // Materialize the argument at its declared decimal type first:
            // a bare literal lowers to an untyped `Int` constant whose MIR
            // operand type is plain `numerus`, so the `Convert` source must
            // be a decimal-typed temp for the runtime conversion route.
            let argument_temp = self.assign_new_temp(MirValueKind::Operand(argument), argument_ty, expr.span);
            let converted = self.runtime_call_value(
                MirIntrinsic::Convert(MirConversion {
                    store: None,
                    flavor: MirConversionFlavor::Runtime,
                    source_ty: argument_ty,
                    target_ty: numerus_ty,
                    params: Vec::new(),
                    recovery: None,
                    struct_defaults: Vec::new(),
                }),
                vec![MirOperand::Temp(argument_temp)],
                numerus_ty,
                expr.span,
            );
            (numerus_ty, converted)
        } else {
            (argument_ty, argument)
        };
        let exponent_local = self.next_local_id();
        self.locals.push(MirLocalDecl {
            id: exponent_local,
            name: None,
            ty: exponent_ty,
            mutable: true,
            span: expr.span,
        });
        let accumulator_place = MirPlace::local(accumulator);
        let base_place = MirPlace::local(base_local);
        let exponent_place = MirPlace::local(exponent_local);
        // The accumulator starts at one. A bare `Int(1)` stored into a
        // decimal slot is the raw scale-8 carrier `1e-8`, so the decimal
        // receiver starts from a `numerus` 1 through the runtime decimal
        // conversion instead.
        let accumulator_start = if decimal {
            self.lower_decimal_one(ty, numerus_ty, expr.span)
        } else {
            MirOperand::Constant(MirConstant::Int(1))
        };
        self.assign(accumulator_place.clone(), accumulator_start, ty, expr.span);
        self.assign(base_place.clone(), base, ty, expr.span);
        self.assign(exponent_place.clone(), exponent, exponent_ty, expr.span);

        let done =
            self.emit_potentia_loop(&accumulator_place, &base_place, &exponent_place, exponent_ty, ty, expr.span);
        self.switch_to(done);
        self.assign(MirPlace::temp(destination), MirOperand::Place(accumulator_place), ty, expr.span);
        Some(MirOperand::Temp(destination))
    }

    /// Whether a `potentia` receiver is a width-scaled decimal (`d64`).
    fn is_decimal_receiver(&self, receiver_ty: TypeId) -> bool {
        matches!(
            self.normalized_type(receiver_ty),
            Type::SizedNumeric(Primitive::Numerus, width) if width.is_decimal()
        )
    }

    /// The exact decimal `1.0` as an operand of `target_ty`.
    ///
    /// WHY: a bare `MirConstant::Int(1)` stored into a decimal slot is the raw
    /// scale-8 carrier `1e-8`, not `1.0`, so `potentia`'s accumulator must
    /// start from a `numerus` 1 put through the runtime decimal conversion.
    fn lower_decimal_one(&mut self, target_ty: MirType, numerus_ty: MirType, span: Span) -> MirOperand {
        self.runtime_call_value(
            MirIntrinsic::Convert(MirConversion {
                store: None,
                flavor: MirConversionFlavor::Runtime,
                source_ty: numerus_ty,
                target_ty,
                params: Vec::new(),
                recovery: None,
                struct_defaults: Vec::new(),
            }),
            vec![MirOperand::Constant(MirConstant::Int(1))],
            target_ty,
            span,
        )
    }

    /// Emit the exponentiation-by-squaring loop for integer `potentia`. The
    /// accumulator, base and exponent locals already hold their start values;
    /// the loop multiplies the accumulator on an odd exponent, halves the
    /// exponent, and squares the base only when another iteration remains.
    /// Returns the `done` block the caller joins after the loop.
    fn emit_potentia_loop(
        &mut self,
        accumulator_place: &MirPlace,
        base_place: &MirPlace,
        exponent_place: &MirPlace,
        exponent_ty: MirType,
        ty: MirType,
        span: Span,
    ) -> MirBlockId {
        let bool_ty = MirType::semantic(self.types.primitive(Primitive::Bivalens));
        let loop_check = self.fresh_block(span);
        let body = self.fresh_block(span);
        let multiply = self.fresh_block(span);
        let shift = self.fresh_block(span);
        let square = self.fresh_block(span);
        let done = self.fresh_block(span);
        self.trap_negative_potentia_exponent(exponent_place, loop_check, span);

        self.switch_to(loop_check);
        let has_exponent = self.binary_temp(
            MirBinOp::Gt,
            MirOperand::Place(exponent_place.clone()),
            MirOperand::Constant(MirConstant::Int(0)),
            bool_ty,
            span,
        );
        self.terminate_current(
            MirTerminatorKind::Branch { condition: has_exponent, then_block: body, else_block: done },
            span,
        );

        self.switch_to(body);
        let odd_part = self.binary_temp(
            MirBinOp::Mod,
            MirOperand::Place(exponent_place.clone()),
            MirOperand::Constant(MirConstant::Int(2)),
            exponent_ty,
            span,
        );
        let is_odd = self.binary_temp(
            MirBinOp::NotEq,
            odd_part,
            MirOperand::Constant(MirConstant::Int(0)),
            bool_ty,
            span,
        );
        self.terminate_current(
            MirTerminatorKind::Branch { condition: is_odd, then_block: multiply, else_block: shift },
            span,
        );

        self.switch_to(multiply);
        let product = self.binary_temp(
            MirBinOp::Mul,
            MirOperand::Place(accumulator_place.clone()),
            MirOperand::Place(base_place.clone()),
            ty,
            span,
        );
        self.assign(accumulator_place.clone(), product, ty, span);
        self.terminate_current(MirTerminatorKind::Goto(shift), span);

        self.switch_to(shift);
        let half = self.binary_temp(
            MirBinOp::Div,
            MirOperand::Place(exponent_place.clone()),
            MirOperand::Constant(MirConstant::Int(2)),
            exponent_ty,
            span,
        );
        self.assign(exponent_place.clone(), half, exponent_ty, span);
        let has_more = self.binary_temp(
            MirBinOp::Gt,
            MirOperand::Place(exponent_place.clone()),
            MirOperand::Constant(MirConstant::Int(0)),
            bool_ty,
            span,
        );
        self.terminate_current(
            MirTerminatorKind::Branch { condition: has_more, then_block: square, else_block: loop_check },
            span,
        );

        self.switch_to(square);
        let squared = self.binary_temp(
            MirBinOp::Mul,
            MirOperand::Place(base_place.clone()),
            MirOperand::Place(base_place.clone()),
            ty,
            span,
        );
        self.assign(base_place.clone(), squared, ty, span);
        self.terminate_current(MirTerminatorKind::Goto(loop_check), span);
        done
    }

    /// Ruling 28: a negative integer exponent is an arithmetic trap (the class
    /// of a zero divisor), never `⇥`. Branches to `proceed` for `exponent >= 0`.
    fn trap_negative_potentia_exponent(&mut self, exponent: &MirPlace, proceed: MirBlockId, span: Span) {
        let bool_ty = MirType::semantic(self.types.primitive(Primitive::Bivalens));
        let negative = self.binary_temp(
            MirBinOp::Lt,
            MirOperand::Place(exponent.clone()),
            MirOperand::Constant(MirConstant::Int(0)),
            bool_ty,
            span,
        );
        let trap = self.fresh_block(span);
        self.terminate_current(
            MirTerminatorKind::Branch { condition: negative, then_block: trap, else_block: proceed },
            span,
        );
        self.switch_to(trap);
        let message = MirOperand::Constant(MirConstant::String(self.context.diagnostic_potentia_negative_exponent));
        let numquam = MirType::semantic(self.types.primitive(Primitive::Numquam));
        self.append_stmt(MirStatement {
            kind: MirStatementKind::RuntimeCall {
                destination: None,
                call: MirRuntimeCall { intrinsic: MirIntrinsic::Panic, args: vec![message], return_ty: numquam },
            },
            span,
        });
        self.terminate_current(MirTerminatorKind::Unreachable, span);
    }

    /// Lower `lista.ordina()` as an in-place update using the existing sorted
    /// collection result operation.
    fn lower_lista_ordina(
        &mut self,
        receiver: &HirExpression,
        method_name: &str,
        args: &[HirCallArg],
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        if method_name != "ordina" || !args.is_empty() {
            return None;
        }
        // In-place `ordina` requires a `mut` receiver (SEM020), so the receiver
        // type usually carries the access marker; look through it.
        let payload_ty = self.method_receiver_payload_id(receiver.ty?);
        if !matches!(self.normalized_type(payload_ty), Type::Array(_)) {
            return None;
        }

        let collection_ty = self.expr_ty(receiver)?;
        let (place, ty) = self.lower_assignment_place_with_fallback(receiver, collection_ty)?;
        let sorted = self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::Sort),
            vec![MirOperand::Place(place.clone())],
            ty,
            expr.span,
        );
        self.assign(place, sorted, ty, expr.span);
        Some(MirOperand::Constant(MirConstant::Unit))
    }

    pub(super) fn lower_direct_call_args(
        &mut self,
        callee: DefId,
        args: &[HirCallArg],
        span: Span,
    ) -> Option<Vec<MirOperand>> {
        if args.len() == 1
            && args[0].spread
            && let Some(params) = self.context.function_params.get(&callee)
            && params.len() > 1
            && params.iter().all(|param| !param.optional)
        {
            return self.lower_spread_call_args(&args[0].expr, params.len(), span);
        }

        if let Some(params) = self.context.callable_param_lower_meta(callee) {
            let mut lowered = Vec::with_capacity(params.len());
            for (index, param) in params.iter().enumerate() {
                if let Some(arg) = args.get(index) {
                    lowered.push(self.lower_call_arg_value(&arg.expr)?);
                } else if let Some(default) = param.default {
                    lowered.push(self.lower_call_arg_value(default)?);
                } else if param.optional {
                    lowered.push(self.omitted_optional_operand(param.ty, span));
                } else {
                    self.errors.push(MirError::unsupported(
                        span,
                        "call missing required argument before MIR lowering completed",
                    ));
                    return None;
                }
            }
            return Some(lowered);
        }

        // Linked callees receive their DefIds after analysis, so their parameter
        // metadata travels through options (like the synthetic-callee fallbacks
        // for `function_generator` and `function_errors` in `lower_call`).
        // Metadata is only authoritative when it can account for the provided
        // arguments; shorter metadata falls through to the generic branch rather
        // than silently dropping arguments past `params.len()`.
        if let Some(params) = self.context.imported_function_params.get(&callee).cloned()
            && params.len() >= args.len()
        {
            let mut lowered = Vec::with_capacity(params.len());
            for (index, param) in params.iter().enumerate() {
                if let Some(arg) = args.get(index) {
                    lowered.push(self.lower_call_arg_value(&arg.expr)?);
                } else if let Some(default) = &param.default {
                    lowered.push(self.lower_call_arg_value(default)?);
                } else if param.optional {
                    lowered.push(self.omitted_optional_operand(None, span));
                } else {
                    self.errors.push(MirError::unsupported(
                        span,
                        "call missing required argument before MIR lowering completed",
                    ));
                    return None;
                }
            }
            return Some(lowered);
        }

        let mut lowered = Vec::with_capacity(args.len());
        for arg in args {
            lowered.push(self.lower_call_arg_value(&arg.expr)?);
        }
        Some(lowered)
    }

    fn lower_spread_call_args(&mut self, spread: &HirExpression, arity: usize, span: Span) -> Option<Vec<MirOperand>> {
        let collection = self.lower_expr_value(spread)?;
        let collection_ty = self.expr_ty(spread)?;
        let temp = self.push_temp(collection_ty, span);
        self.assign(MirPlace::temp(temp), collection, collection_ty, span);
        let mut args = Vec::with_capacity(arity);
        for index in 0..arity {
            #[expect(
                clippy::cast_possible_wrap,
                reason = "call arity is bounded by source program size, far below i64::MAX"
            )]
            let index_operand = MirOperand::Constant(MirConstant::Int(index as i64));
            args.push(MirOperand::Place(MirPlace {
                base: radix_mir::MirPlaceBase::Temp(temp),
                projections: vec![MirProjection::Index(index_operand)],
            }));
        }
        Some(args)
    }

    /// Contract method reached through the enclosing callable's bound on the
    /// receiver's type parameter (D1.1).
    fn bound_method_target(&self, receiver: &HirExpression, method: Symbol) -> Option<DefId> {
        let receiver_ty = receiver.ty?;
        let (_, payload) = self.types.peel_access_marker(receiver_ty);
        let Type::Param(param) = self.types.get(payload) else {
            return None;
        };
        self.bound_methods.get(&(*param, method)).copied()
    }

    fn genus_method_target(&self, receiver: &HirExpression, method: Symbol) -> Option<MethodTarget> {
        let receiver_ty = receiver.ty?;
        let def_id = self.method_receiver_struct_def(receiver_ty)?;
        self.context.method_targets.get(&(def_id, method)).copied()
    }

    /// Lower a direct call, enum variant constructor, or failable call.
    ///
    /// Path callees resolve to definition calls, provider imports, or variant
    /// constructors. Locals holding callable values lower through
    /// [`MirCallee::Value`].
    pub(super) fn lower_call(
        &mut self,
        callee: &HirExpression,
        type_args: &[TypeId],
        args: &[HirCallArg],
        conversion: Option<DefId>,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        if let HirExpressionKind::OptionalChain(object, HirOptionalChainKind::Member(method)) = &callee.kind
            && let Some(object_ty) = object.ty
            && let Type::Option(inner) = *self.types.get(self.method_receiver_payload_id(object_ty))
            && let Some(def) = self.method_receiver_struct_def(inner)
            && let Some(target) = self.context.method_targets.get(&(def, *method)).copied()
        {
            // `a?.method(args)`: chain base becomes arg 0 of the genus method,
            // receiver-first exactly like the genus branch of lower_method_call_on_receiver.
            let base = self.lower_expr_value(object)?;
            let ty = self.expr_ty(expr)?;
            let operands_effect_free = args.iter().all(|arg| arg.expr.is_effect_free_operand());
            return self.lower_option_chain_guarded(
                &base,
                object.ty,
                operands_effect_free,
                ty,
                expr.span,
                |this, captured_base| {
                    let mut call_args = Vec::with_capacity(args.len() + 1);
                    call_args.push(captured_base.clone());
                    call_args.extend(this.lower_hir_call_args(args)?);
                    this.emit_call_shape_check(expr.id, &call_args, 1, expr.span)?;
                    Some(MirOptionChainLink::Call {
                        callee: MirCallee::definition_with_type_args(target.def_id, Vec::new()),
                        args: call_args,
                    })
                },
            );
        }

        // Callable value: a local binding or non-path expression. Positional args,
        // indirect call via MirCallee::Value.
        let value_callee = match &callee.kind {
            HirExpressionKind::Path(def_id)
                if self.bindings.contains_key(def_id) || self.captured_bindings.contains_key(def_id) =>
            {
                Some(self.lower_path(*def_id, callee)?)
            }
            HirExpressionKind::Path(_) => None,
            _ => Some(self.lower_expr_value(callee)?),
        };
        if let Some(callee_operand) = value_callee {
            if !type_args.is_empty() {
                self.errors
                    .push(MirError::unsupported(expr.span, "call-site type arguments on callable values"));
                return None;
            }
            let lowered_args = self.lower_positional_call_args(args)?;
            self.emit_call_shape_check(expr.id, &lowered_args, 0, expr.span)?;
            let ty = self.expr_ty(expr)?;
            if let Some(error_ty) = self.callable_error_ty(callee) {
                return self.lower_failable_call(
                    self.closure_callee_for_operand(&callee_operand)
                        .map_or_else(|| MirCallee::Value(callee_operand.clone()), MirCallee::Closure),
                    lowered_args,
                    ty,
                    error_ty,
                    conversion,
                    expr.span,
                );
            }
            return Some(self.call_value_operand(callee_operand, lowered_args, ty, expr.span));
        }

        // Direct definition call: spread/default-aware arg lowering, then dispatch
        // to provider import, variant constructor, or definition body.
        let HirExpressionKind::Path(def_id) = &callee.kind else {
            self.errors.push(MirError::unsupported(
                expr.span,
                "call through a callee that is not a path or value callee",
            ));
            return None;
        };

        if let Some(builtin) = self.context.gpu_builtins.get(def_id).copied() {
            if !args.is_empty() {
                self.errors
                    .push(MirError::unsupported(expr.span, "GPU builtin call with arguments"));
                return None;
            }
            self.emit_call_shape_check(expr.id, &[], 0, expr.span)?;
            let ty = self.expr_ty(expr)?;
            return Some(self.runtime_call_value(MirIntrinsic::GpuBuiltin(builtin), Vec::new(), ty, expr.span));
        }

        let lowered_args = self.lower_direct_call_args(*def_id, args, expr.span)?;
        let ty = self.expr_ty(expr)?;
        let callee_async = self
            .context
            .function_async
            .get(def_id)
            .copied()
            .unwrap_or(false)
            || self.callable_is_async(callee);
        let ty = if callee_async { self.async_payload_ty(ty) } else { ty };

        if let Some(import) = self.context.provider_imports.get(def_id).cloned() {
            self.emit_call_shape_check(expr.id, &lowered_args, 0, expr.span)?;
            return Some(self.runtime_call_value(
                MirIntrinsic::Provider(MirProvider { kind: import.kind, module: import.module, name: import.item }),
                lowered_args,
                ty,
                expr.span,
            ));
        }

        if self.context.variant_parents.contains_key(def_id) {
            self.emit_call_shape_check(expr.id, &lowered_args, 0, expr.span)?;
            let fields = self.variant_payload(*def_id, lowered_args, expr.span);
            let ty = self.variant_value_ty(expr)?;
            return Some(self.construct_temp(MirAggregateKind::EnumVariant(*def_id), fields, ty, expr.span));
        }

        let explicit_type_args = type_args.iter().copied().map(MirType::semantic).collect();

        // Linked generator export: a `fiunt` function rewritten to a
        // synthetic definition id is absent from this unit's
        // `function_generator` registry (the id is allocated after
        // analysis). The callee's HIR type is the only remaining source of
        // generator-ness; without the cursor-stream intrinsic a plain `Call`
        // destination (the cursor carrier) mismatches the callee's declared
        // element return type at MIR validation.
        if !self
            .context
            .function_generator
            .get(def_id)
            .copied()
            .unwrap_or(false)
            && self.callable_is_generator(callee)
        {
            self.emit_call_shape_check(expr.id, &lowered_args, 0, expr.span)?;
            return Some(self.runtime_call_value(
                MirIntrinsic::CursorStream(radix_mir::MirCursorStream { callee: *def_id, stepped: false }),
                lowered_args,
                ty,
                expr.span,
            ));
        }

        // Package-MIR failable-call fallback: a callee rewritten to a
        // synthetic definition id (the faber linker's `Path(synthetic)` for a
        // linked library export) is absent from this unit's `function_errors`
        // registry — the id is allocated after analysis. The callee's HIR
        // type is then the only source of truth for failable-ness. Without
        // the try-call edge the cape handler is never referenced and its
        // `si`-chain blocks are orphaned (PKG001 pkg001-mir-fix). Known
        // generator/cursor-stream defs stay on their own route.
        if !self.context.function_errors.contains_key(def_id)
            && !self
                .context
                .function_generator
                .get(def_id)
                .copied()
                .unwrap_or(false)
            && !self.callable_is_generator(callee)
            && let Some(error_ty) = self
                .callable_error_ty(callee)
                .or_else(|| self.callable_async_error_ty(callee))
        {
            let callee = MirCallee::definition_with_type_args(*def_id, explicit_type_args);
            return self.lower_failable_call(callee, lowered_args, ty, error_ty, conversion, expr.span);
        }

        self.lower_definition_call(*def_id, explicit_type_args, lowered_args, conversion, 0, expr)
    }

    /// A call argument. An integer constant carries the type its HIR literal
    /// was given (`f(200)` into a `u8` parameter is a `u8`, `f(3.0)` into a
    /// `d64` parameter is a `d64` carrier), not the untyped
    /// `numerus` of the bare MIR constant, so the callee sees the argument at
    /// the parameter's type without a validator exemption (d11-6 ruling 12a).
    pub(super) fn lower_call_arg_value(&mut self, arg: &HirExpression) -> Option<MirOperand> {
        let value = self.lower_expr_value(arg)?;
        if !matches!(value, MirOperand::Constant(MirConstant::Int(_) | MirConstant::UInt(_))) {
            return Some(value);
        }
        let Some(ty) = arg.ty else {
            return Some(value);
        };
        let sized = match self.normalized_type(self.types.peel_access_marker(ty).1) {
            // A decimal literal is its scaled carrier: a bare `Int` constant
            // would read as plain `numerus` at the call (F9 ruling 30).
            // Deliberate: `inf` is not a sized scalar constant slot (heap bignum, U4).
            Type::SizedNumeric(Primitive::Numerus, width) => width.integer_range().is_some() || width.is_decimal(),
            // A `wrapping` / `saturating` parameter takes the constant at its
            // own word type (ruling 12: a constant in a slot takes its type).
            Type::ModularWord(_) | Type::SaturatingWord(_) | Type::RangedInt { .. } => true,
            _ => false,
        };
        Some(if sized {
            self.assign_temp(MirValueKind::Operand(value), MirType::semantic(ty), arg.span)
        } else {
            value
        })
    }

    pub(super) fn lower_positional_call_args(&mut self, args: &[HirCallArg]) -> Option<Vec<MirOperand>> {
        let mut lowered = Vec::with_capacity(args.len());
        for arg in args {
            lowered.push(self.lower_call_arg_value(&arg.expr)?);
        }
        Some(lowered)
    }

    fn lower_propagated_error(
        &mut self,
        error_temp: MirTempId,
        destination_ty: MirType,
        conversion: Option<DefId>,
        span: Span,
    ) -> MirOperand {
        let Some(arm) = conversion else {
            return MirOperand::Temp(error_temp);
        };
        let converted = self.push_temp(destination_ty, span);
        // EDGE: a conversion row that declares its own `⇥` channel is
        // failable; applying it with an ordinary `Call` fails MIR validation
        // (a failable callee needs a `try_call` edge). The row's own failure
        // composes into the active sink — semantic analysis proved the row's
        // error channel fits it (`check_registered_row_channel`) — so the
        // row's error block returns the row error, and the caller terminates
        // the returned ok-block with the converted value.
        let Some(row_err_ty) = self.context.function_errors.get(&arm).copied() else {
            self.append_stmt(MirStatement {
                kind: MirStatementKind::Call {
                    destination: Some(MirPlace::temp(converted)),
                    callee: MirCallee::definition(arm),
                    args: vec![MirOperand::Temp(error_temp)],
                },
                span,
            });
            return MirOperand::Temp(converted);
        };
        let ok_block = self.fresh_block(span);
        let error_block = self.fresh_block(span);
        let row_error_temp = self.push_temp(row_err_ty, span);
        self.terminate_current(
            MirTerminatorKind::TryCall {
                destination: Some(MirPlace::temp(converted)),
                callee: MirCallee::definition(arm),
                args: vec![MirOperand::Temp(error_temp)],
                ok_block,
                error_place: MirPlace::temp(row_error_temp),
                error_block,
            },
            span,
        );
        self.switch_to(error_block);
        self.terminate_current(MirTerminatorKind::ReturnError(MirOperand::Temp(row_error_temp)), span);
        self.switch_to(ok_block);
        MirOperand::Temp(converted)
    }

    /// The error conversion applied when a failable call's error propagates to
    /// the enclosing function's alternate-exit terminator.
    ///
    /// A conversion recorded on the HIR call site by semantic analysis wins.
    /// When none was recorded — the package-MIR synthetic-path rewrite of
    /// namespace calls drops the call-site field — recover the coherence
    /// registry's enrolled row for the call's exact ordered pair, the same
    /// function the semantic layer would have attached. Types that already
    /// agree need no conversion. Lookup is direct only: no transitive
    /// composition, and a pair with no enrolled row fails closed — the raw
    /// value is kept and MIR validation rejects the `ReturnError`.
    fn declared_error_conversion(
        &self,
        source_ty: MirType,
        destination_ty: MirType,
        call_site: Option<DefId>,
    ) -> Option<DefId> {
        if call_site.is_some()
            || self
                .types
                .is_assignable(source_ty.semantic_id(), destination_ty.semantic_id())
        {
            return call_site;
        }
        self.context
            .conversion_rows
            .get(&ConversionPair::new(source_ty.semantic_id(), destination_ty.semantic_id()))
            .copied()
    }

    /// The registered `valor → E` row that materializes a bare `sermo ↦ T`
    /// failure into the enclosing `⇥ E` channel, when the conversion source is
    /// a conversation handle and the registry enrolls one. The row's call is
    /// wired on the failable conversion's error edge by [`Self::lower_failable_call`].
    fn sermo_failure_row(&self, source_ty: TypeId) -> Option<DefId> {
        let sermo_def = self.context.sermo_def?;
        match self.normalized_type(source_ty) {
            Type::Struct(def_id) if *def_id == sermo_def => {}
            Type::Applied(base, _) if matches!(self.types.get(*base), Type::Struct(def_id) if *def_id == sermo_def) => {
            }
            _ => return None,
        }
        let destination = self.error_ty?;
        let value_ty = self.types.primitive(Primitive::Valor);
        self.context
            .conversion_rows
            .get(&ConversionPair::new(value_ty, destination.semantic_id()))
            .copied()
    }

    // consumes type_args and args into MIR statements
    pub(super) fn lower_definition_call(
        &mut self,
        def_id: DefId,
        explicit_type_args: Vec<MirType>,
        lowered_args: Vec<MirOperand>,
        conversion: Option<DefId>,
        callee_parameter_offset: u32,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        self.emit_call_shape_check(expr.id, &lowered_args, callee_parameter_offset, expr.span)?;
        let ty = self.expr_ty(expr)?;
        let ty = if self.definition_call_is_async(def_id, expr) {
            self.async_payload_ty(ty)
        } else {
            ty
        };
        let callee = MirCallee::definition_with_type_args(def_id, explicit_type_args);

        if self
            .context
            .function_generator
            .get(&def_id)
            .copied()
            .unwrap_or(false)
        {
            return Some(self.runtime_call_value(
                MirIntrinsic::CursorStream(radix_mir::MirCursorStream { callee: def_id, stepped: false }),
                lowered_args,
                ty,
                expr.span,
            ));
        }

        if let Some(err_ty) = self.context.function_errors.get(&def_id).copied() {
            return self.lower_failable_call(callee, lowered_args, ty, err_ty, conversion, expr.span);
        }

        if self.is_vacuum(ty) {
            self.append_stmt(MirStatement {
                kind: MirStatementKind::Call { destination: None, callee: callee.clone(), args: lowered_args },
                span: expr.span,
            });
            return Some(MirOperand::Constant(MirConstant::Unit));
        }

        let destination = self.push_temp(ty, expr.span);
        self.append_stmt(MirStatement {
            kind: MirStatementKind::Call { destination: Some(MirPlace::temp(destination)), callee, args: lowered_args },
            span: expr.span,
        });
        Some(MirOperand::Temp(destination))
    }

    pub(super) fn callable_error_ty(&self, callee: &HirExpression) -> Option<MirType> {
        fn error_ty(types: &TypeTable, ty: TypeId) -> Option<MirType> {
            match types.get(ty) {
                Type::Func(signature) => signature.err.map(MirType::semantic),
                Type::Alias(_, inner) => error_ty(types, *inner),
                _ => None,
            }
        }
        callee.ty.and_then(|ty| error_ty(self.types, ty))
    }

    /// The error half of an awaited `@ future` callee whose declared `⇥ E`
    /// channel was folded into its wrapped `promissum<T ⇥ E>` return.
    ///
    /// A linked library function exported through the file interface has its
    /// `⇥ E` channel re-wrapped by `import_callable_return_shape`, which clears
    /// the interface `FuncSig.err`; [`Self::callable_error_ty`] therefore
    /// cannot see it. Recovering `E` from the wrapped return keeps the awaited
    /// linked call on the `TryCall` edge its sync twin already uses. Keyed on
    /// the callee's async fact: a non-async callable may declare a `promissum`
    /// return without being failable.
    fn callable_async_error_ty(&self, callee: &HirExpression) -> Option<MirType> {
        fn async_error(types: &TypeTable, ty: TypeId) -> Option<MirType> {
            match types.get(ty) {
                Type::Func(signature) if signature.is_async => match types.get(signature.ret) {
                    Type::PromissumFailable(_, err) => Some(MirType::semantic(*err)),
                    _ => None,
                },
                Type::Alias(_, inner) => async_error(types, *inner),
                _ => None,
            }
        }
        callee.ty.and_then(|ty| async_error(self.types, ty))
    }

    pub(super) fn callable_is_generator(&self, callee: &HirExpression) -> bool {
        fn is_generator(types: &TypeTable, ty: TypeId) -> bool {
            match types.get(ty) {
                Type::Func(signature) => signature.is_generator,
                Type::Alias(_, inner) => is_generator(types, *inner),
                _ => false,
            }
        }
        callee.ty.is_some_and(|ty| is_generator(self.types, ty))
    }

    pub(super) fn callable_is_async(&self, callee: &HirExpression) -> bool {
        fn is_async(types: &TypeTable, ty: TypeId) -> bool {
            match types.get(ty) {
                Type::Func(signature) => signature.is_async,
                Type::Alias(_, inner) => is_async(types, *inner),
                _ => false,
            }
        }
        callee.ty.is_some_and(|ty| is_async(self.types, ty))
    }

    /// The success payload of an awaited `@ future` call result: a
    /// `promissum<T>`/`promissum<T ⇥ E>` call yields `T` (`async_surface.rs`).
    fn async_payload_ty(&self, ty: MirType) -> MirType {
        match self.normalized_type(ty.semantic_id()) {
            Type::Promissum(inner) | Type::PromissumFailable(inner, _) => MirType::semantic(*inner),
            _ => ty,
        }
    }

    /// Whether the definition call named by `def_id` resolves to an async
    /// callee. In-unit facts live in the async registry; a linked or provider
    /// callee is resolved through its HIR signature.
    fn definition_call_is_async(&self, def_id: DefId, expr: &HirExpression) -> bool {
        self.context
            .function_async
            .get(&def_id)
            .copied()
            .unwrap_or(false)
            || match &expr.kind {
                HirExpressionKind::Call(callee, ..) => self.callable_is_async(callee),
                _ => false,
            }
    }

    // consumes callee and args into MIR statements
    pub(super) fn lower_failable_call(
        &mut self,
        callee: MirCallee,
        lowered_args: Vec<MirOperand>,
        ty: MirType,
        err_ty: MirType,
        conversion: Option<DefId>,
        span: Span,
    ) -> Option<MirOperand> {
        // UT — the `capta` capture boundary: the failable call lowers to the
        // existing `TryCall` whose error block wraps the payload into the
        // trap carrier (union-typed destination) and joins. A trapped error
        // never reaches `ReturnError`.
        if let Some(trap) = self.take_innermost_trap() {
            let ok_block = self.fresh_block(span);
            let error_block = self.fresh_block(span);
            let error_temp = self.push_temp(err_ty, span);
            let error_place = MirPlace::temp(error_temp);
            if self.is_vacuum(ty) {
                self.terminate_current(
                    MirTerminatorKind::TryCall {
                        destination: None,
                        callee,
                        args: lowered_args,
                        ok_block,
                        error_place,
                        error_block,
                    },
                    span,
                );
                self.switch_to(error_block);
                self.assign(trap.destination, MirOperand::Temp(error_temp), err_ty, span);
                self.terminate_current(MirTerminatorKind::Goto(trap.join_block), span);
                self.switch_to(ok_block);
                return Some(MirOperand::Constant(MirConstant::Unit));
            }

            let destination = self.push_temp(ty, span);
            self.terminate_current(
                MirTerminatorKind::TryCall {
                    destination: Some(MirPlace::temp(destination)),
                    callee,
                    args: lowered_args,
                    ok_block,
                    error_place,
                    error_block,
                },
                span,
            );
            self.switch_to(error_block);
            self.assign(trap.destination, MirOperand::Temp(error_temp), err_ty, span);
            self.terminate_current(MirTerminatorKind::Goto(trap.join_block), span);
            self.switch_to(ok_block);
            return Some(MirOperand::Temp(destination));
        }

        if let Some(handler) = self.handlers.last().cloned() {
            let ok_block = self.fresh_block(span);
            if self.is_vacuum(ty) {
                self.terminate_current(
                    MirTerminatorKind::TryCall {
                        destination: None,
                        callee,
                        args: lowered_args,
                        ok_block,
                        error_place: handler.error_place,
                        error_block: handler.error_block,
                    },
                    span,
                );
                self.switch_to(ok_block);
                return Some(MirOperand::Constant(MirConstant::Unit));
            }

            let destination = self.push_temp(ty, span);
            self.terminate_current(
                MirTerminatorKind::TryCall {
                    destination: Some(MirPlace::temp(destination)),
                    callee,
                    args: lowered_args,
                    ok_block,
                    error_place: handler.error_place,
                    error_block: handler.error_block,
                },
                span,
            );
            self.switch_to(ok_block);
            return Some(MirOperand::Temp(destination));
        }

        Some(self.lower_failable_call_declared(callee, lowered_args, ty, err_ty, conversion, span))
    }

    /// Lower a failable call that reaches the declared `⇥` error channel (no
    /// active trap or `cape` handler): the failure propagates through
    /// `ReturnError`, or panics when the unit declares no error channel.
    fn lower_failable_call_declared(
        &mut self,
        callee: MirCallee,
        lowered_args: Vec<MirOperand>,
        ty: MirType,
        err_ty: MirType,
        conversion: Option<DefId>,
        span: Span,
    ) -> MirOperand {
        let Some(destination_ty) = self.error_ty else {
            return self.lower_unhandled_call_panic(callee, lowered_args, ty, err_ty, span);
        };
        let conversion = self.declared_error_conversion(err_ty, destination_ty, conversion);

        let ok_block = self.fresh_block(span);
        let error_block = self.fresh_block(span);
        let error_temp = self.push_temp(err_ty, span);
        if self.is_vacuum(ty) {
            self.terminate_current(
                MirTerminatorKind::TryCall {
                    destination: None,
                    callee,
                    args: lowered_args,
                    ok_block,
                    error_place: MirPlace::temp(error_temp),
                    error_block,
                },
                span,
            );
            self.switch_to(error_block);
            let propagated = self.lower_propagated_error(error_temp, destination_ty, conversion, span);
            self.terminate_current(MirTerminatorKind::ReturnError(propagated), span);
            self.switch_to(ok_block);
            return MirOperand::Constant(MirConstant::Unit);
        }

        let destination = self.push_temp(ty, span);
        self.terminate_current(
            MirTerminatorKind::TryCall {
                destination: Some(MirPlace::temp(destination)),
                callee,
                args: lowered_args,
                ok_block,
                error_place: MirPlace::temp(error_temp),
                error_block,
            },
            span,
        );
        self.switch_to(error_block);
        let propagated = self.lower_propagated_error(error_temp, destination_ty, conversion, span);
        self.terminate_current(MirTerminatorKind::ReturnError(propagated), span);
        self.switch_to(ok_block);
        MirOperand::Temp(destination)
    }

    /// D17.8: a failable call with no trap, `cape` handler or declared `⇥`
    /// channel is only legal directly in `main {}` (typecheck rejects it
    /// everywhere else), where the failure is a panic. The error value is the
    /// panic message, exactly as an unhandled failable conversion reports.
    fn lower_unhandled_call_panic(
        &mut self,
        callee: MirCallee,
        lowered_args: Vec<MirOperand>,
        ty: MirType,
        err_ty: MirType,
        span: Span,
    ) -> MirOperand {
        let ok_block = self.fresh_block(span);
        let error_block = self.fresh_block(span);
        let error_temp = self.push_temp(err_ty, span);
        let destination = (!self.is_vacuum(ty)).then(|| self.push_temp(ty, span));
        self.terminate_current(
            MirTerminatorKind::TryCall {
                destination: destination.map(MirPlace::temp),
                callee,
                args: lowered_args,
                ok_block,
                error_place: MirPlace::temp(error_temp),
                error_block,
            },
            span,
        );
        self.switch_to(error_block);
        let numquam = MirType::semantic(self.types.primitive(Primitive::Numquam));
        self.append_stmt(MirStatement {
            kind: MirStatementKind::RuntimeCall {
                destination: None,
                call: MirRuntimeCall {
                    intrinsic: MirIntrinsic::Panic,
                    args: vec![MirOperand::Temp(error_temp)],
                    return_ty: numquam,
                },
            },
            span,
        });
        self.terminate_current(MirTerminatorKind::Unreachable, span);
        self.switch_to(ok_block);
        destination.map_or(MirOperand::Constant(MirConstant::Unit), MirOperand::Temp)
    }

    /// Select the strict read carrier for a declared literal-bound receiver.
    ///
    /// MIR values do not carry the declared receiver type at runtime. Using a
    /// non-optional return type on the existing read op is the lower-time
    /// checked/lookup variant: the runner traps for strict reads and keeps
    /// `Option` lookup behavior for dynamic receivers.
    pub(super) fn checked_read_result_type(
        &self,
        op: MirCollectionOp,
        receiver_ty: Option<TypeId>,
        expr_ty: MirType,
    ) -> Option<MirType> {
        let receiver_ty = receiver_ty?;
        let result = self.types.nullable_inner(expr_ty.semantic_id())?;
        let checked = match op {
            MirCollectionOp::Index => self.declared_literal_bound(receiver_ty),
            MirCollectionOp::TensorGet => self.declared_literal_tensor_shape(receiver_ty),
            _ => false,
        };
        checked.then_some(MirType::semantic(result))
    }

    /// The declared index-read carrier when the expression type kept an
    /// unresolved inference variable inside its nullable carrier.
    ///
    /// WHY: expected-type narrowing on an `out.append(xs.get(i))` argument
    /// inside a generic genus body can leave the `get` call's HIR type as an
    /// optional carrier with an inference hole instead of the receiver's
    /// element, and the checker's finalize pass only refuses a hole at the
    /// carrier's head, so a nested one reaches lowering. The intrinsic's
    /// result is not in doubt there — an index read returns the receiver's
    /// element lifted into its nullable carrier — so fill the hole from the
    /// receiver rather than emitting a MIR type the validator must reject.
    /// Resolution stays immutable; a missing interned carrier keeps the old
    /// fail-soft behavior.
    fn index_read_result_type(&self, receiver_ty: Option<TypeId>, expr_ty: MirType) -> Option<MirType> {
        let receiver_ty = receiver_ty?;
        let Type::Option(inner) = self.types.get(expr_ty.semantic_id()) else {
            return None;
        };
        if !matches!(self.types.get(*inner), Type::Infer(_) | Type::InferUnion(_)) {
            return None;
        }
        let element = self.collection_element_ty(receiver_ty)?;
        if matches!(self.types.get(element.semantic_id()), Type::Infer(_) | Type::InferUnion(_)) {
            return None;
        }
        let option = self
            .types
            .find_equal(&Type::Option(element.semantic_id()))?;
        Some(MirType::semantic(option))
    }

    fn declared_literal_bound(&self, ty: TypeId) -> bool {
        let ty = self.normalized_type(ty);
        let capacity = match ty {
            Type::BoundedArray { capacity, .. }
            | Type::BoundedTextus { capacity }
            | Type::BoundedAscii { capacity }
            | Type::BoundedOcteti { capacity } => *capacity,
            _ => return false,
        };
        matches!(self.types.get_index(capacity), radix_types::IndexExpr::Literal(_))
    }

    fn declared_literal_tensor_shape(&self, ty: TypeId) -> bool {
        let Type::Tensor(_, shape) = self.normalized_type(ty) else {
            return false;
        };
        match self.types.get_index(*shape) {
            radix_types::IndexExpr::Literal(_) => true,
            radix_types::IndexExpr::Tuple(dims) => dims
                .iter()
                .all(|dim| matches!(self.types.get_index(*dim), radix_types::IndexExpr::Literal(_))),
            radix_types::IndexExpr::Param(_)
            | radix_types::IndexExpr::Runtime(_)
            | radix_types::IndexExpr::Affine { .. }
            | radix_types::IndexExpr::Divide { .. }
            | radix_types::IndexExpr::Infer(_)
            | radix_types::IndexExpr::Unspecified => false,
        }
    }

    /// Resolve a HIR method call into the small set of collection intrinsics
    /// currently represented in MIR.
    fn collection_method_op(
        &mut self,
        receiver: &HirExpression,
        method: Symbol,
        args: &[HirCallArg],
        span: Span,
    ) -> Option<MirCollectionOp> {
        let Some(receiver_ty) = receiver.ty else {
            self.errors
                .push(MirError::missing_type(span, "collection method receiver"));
            return None;
        };
        let method_name = self.resolve_method_name(method)?;
        // The type-expression constructor (`tf32[2, 3]`) synthesizes a `crea`
        // call on a vacua receiver (`lower_construct`; the check lane rewrites
        // its symbol to the canonical spelling). The retired receiver row no
        // longer dispatches it, so it lowers to the same TensorCreate op here.
        if matches!(receiver.kind, HirExpressionKind::Vacua) && method_name == "crea" && args.len() == 2 {
            return Some(MirCollectionOp::TensorCreate);
        }
        let payload = self.types.get(self.method_receiver_payload_id(receiver_ty));
        // A ledger method dispatches through its map twin's Tabula rows: the
        // carrier shape is the same and only the value kind distinguishes it.
        let entry = match payload {
            Type::Ledger(key, value) => {
                radix_intrinsics::lookup_method_for_type(&Type::Map(*key, *value), method_name, args.len())?
            }
            other => radix_intrinsics::lookup_method_for_type(other, method_name, args.len())?,
        };
        radix_intrinsics::mir_collection_op(entry)
    }

    pub(super) fn resolve_method_name(&self, method: Symbol) -> Option<&str> {
        self.context
            .interner
            .map(|interner| interner.resolve(method))
    }

    /// Lower `mori`/panic into a runtime call followed by an unreachable
    /// terminator.
    pub(super) fn lower_mori(&mut self, value: &HirExpression, span: Span) -> Option<MirOperand> {
        let value = self.lower_expr_value(value)?;
        let numquam = MirType::semantic(self.types.primitive(Primitive::Numquam));
        self.append_stmt(MirStatement {
            kind: MirStatementKind::RuntimeCall {
                destination: None,
                call: radix_mir::MirRuntimeCall {
                    intrinsic: radix_mir::MirIntrinsic::Panic,
                    args: vec![value],
                    return_ty: numquam,
                },
            },
            span,
        });
        self.terminate_current(MirTerminatorKind::Unreachable, span);
        None
    }

    /// Lower `lege` into a target-neutral stdin read intrinsic.
    ///
    /// Q-E5 (ruled 2026-10-15): `read` returns all of stdin to EOF; `read
    /// line` one line without its terminator.
    pub(super) fn lower_read_line(&mut self, expr: &HirExpression) -> Option<MirOperand> {
        let ty = self.expr_ty(expr)?;
        let intrinsic = match expr.kind {
            HirExpressionKind::ReadLine(false) => MirIntrinsic::ReadAll,
            _ => MirIntrinsic::ReadLine,
        };
        Some(self.runtime_call_value(intrinsic, Vec::new(), ty, expr.span))
    }

    /// Lower diagnostic output expressions to diagnostic intrinsics.
    pub(super) fn lower_scribe(
        &mut self,
        kind: HirScribeKind,
        args: &[HirExpression],
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let intrinsic = MirIntrinsic::Diagnostic(mir_diagnostic_kind(kind));
        let return_ty = self.expr_ty(expr)?;
        let template = args
            .len()
            .checked_sub(1)
            .and_then(|index| self.context.diagnostic_format_templates.get(index))
            .copied();
        let has_literal_template = args
            .first()
            .is_some_and(|arg| matches!(arg.kind, HirExpressionKind::Literal(HirLiteral::String(_))));
        if let Some(template) = template
            && has_literal_template
            && args.len() > 1
            && args.len() <= 3
        {
            let mut lowered_args = Vec::with_capacity(args.len());
            for arg in args {
                lowered_args.push(self.lower_expr_value(arg)?);
            }
            let text_ty = MirType::semantic(self.types.primitive(Primitive::Textus));
            let formatted =
                self.runtime_call_value(MirIntrinsic::FormatString { template }, lowered_args, text_ty, expr.span);
            self.runtime_call_value(intrinsic, vec![formatted], return_ty, expr.span);
            return Some(MirOperand::Constant(MirConstant::Unit));
        }
        // L23 (d57baa50): the multi-arg grouping carries the HIR-Rust lane's
        // space-join for EVERY arg count — one diagnostic call with the joined
        // payload instead of one call per argument. The previous gate limited
        // the grouping to `> 3` scalar arguments, so `nota a, b` emitted two
        // separate lines while the Rust oracle prints one space-joined line.
        // The grouping is shared lowering: every MIR consumer (LLVM host,
        // runner, wasm) renders the joined textus the same way.
        let text_ty = MirType::semantic(self.types.primitive(Primitive::Textus));
        if args.len() > 1
            && args.iter().all(|arg| {
                arg.ty
                    .is_some_and(|ty| self.scribe_arg_renders_to_text(MirType::semantic(ty), text_ty))
            })
        {
            let separator = MirOperand::Constant(MirConstant::String(self.context.diagnostic_separator));
            let mut rendered = None;
            for arg in args {
                let arg_ty = self.expr_ty(arg)?;
                let value = self.lower_expr_value(arg)?;
                let value = self.render_scribe_value_text(value, arg_ty, text_ty, expr.span);
                rendered = Some(match rendered {
                    None => value,
                    Some(current) => {
                        let separated = self.assign_temp(
                            MirValueKind::Binary { op: MirBinOp::Add, lhs: current, rhs: separator.clone() },
                            text_ty,
                            expr.span,
                        );
                        self.assign_temp(
                            MirValueKind::Binary { op: MirBinOp::Add, lhs: separated, rhs: value },
                            text_ty,
                            expr.span,
                        )
                    }
                });
            }
            let rendered = rendered?;
            self.runtime_call_value(intrinsic, vec![rendered], return_ty, expr.span);
            return Some(MirOperand::Constant(MirConstant::Unit));
        }
        for arg in args {
            let arg = self.lower_expr_value(arg)?;
            self.runtime_call_value(intrinsic.clone(), vec![arg], return_ty, expr.span);
        }
        Some(MirOperand::Constant(MirConstant::Unit))
    }

    /// Whether a scribe argument type can render to textus through shared MIR
    /// operations (the grouping predicate). Mirrors the HIR-Rust lane's
    /// per-argument display: textus passes through, bare numeric primitives
    /// convert or format (`display_fractus` semantics for fractus), opaque
    /// collections format in their Debug shape, and option values render
    /// their payload or `nihil` via an explicit presence branch. Sized
    /// numerics, bN bit words and the modular/saturating/policy-float cells
    /// render through the single-opaque-arg format carrier, which every MIR
    /// consumer renders at the operand's own declared width, so the grouped
    /// line prints the same text the per-argument path would. Types without
    /// a shared render (regex, structs, nested options) keep the
    /// per-argument diagnostic path below, which retains its specialized
    /// carriers.
    fn scribe_arg_renders_to_text(&self, arg_ty: MirType, text_ty: MirType) -> bool {
        if arg_ty == text_ty {
            return true;
        }
        match self.normalized_type(arg_ty.semantic_id()) {
            // L28 (ab91f49f): sized `fractus<f32>` and `instans` values now
            // render through shared MIR operations too — f32 via the
            // FormatString f32 display carrier (`display_fractus` semantics)
            // and instans via its Rust-oracle Debug-shape opaque format
            // carrier — so multi-arg notas carrying them join like the
            // HIR-Rust lane instead of falling to the per-argument path.
            // This is the FLAGGED continuation of the L23 grouping predicate
            // (the rows L23 explicitly left open).
            Type::Primitive(
                Primitive::Bivalens
                | Primitive::Numerus
                | Primitive::Fractus
                | Primitive::Octeti
                | Primitive::Valor
                | Primitive::Instans,
            )
            | Type::Array(_)
            // An inline collection literal types as the exact-length witness
            // (`BoundedArray`), while a declared binding gets plain `Array`;
            // both render through the same Debug-shape opaque carrier, so the
            // grouping predicate must admit the literal shape too. Otherwise
            // `nota 1, [2]` splits into one diagnostic call per argument.
            | Type::BoundedArray { .. } | Type::RangedArray { .. }
            | Type::Map(_, _)
            | Type::Set(_)
            | Type::SizedNumeric(_, _)
            | Type::Bits(_)
            | Type::ModularWord(_)
            | Type::SaturatingWord(_)
            | Type::RangedInt { .. }
            | Type::PolicyFloat(_, _)
            | Type::SizedInstans(_) => true,
            // Option payloads render as a bare value (`display_option`
            // semantics); nested options are not grouped.
            Type::Option(inner) => {
                let inner_ty = MirType::semantic(*inner);
                inner_ty != arg_ty && self.scribe_arg_renders_to_text(inner_ty, text_ty)
            }
            _ => false,
        }
    }

    /// Render one scribe argument to a textus operand. Only called for types
    /// [`Self::scribe_arg_renders_to_text`] admits, so it never falls back.
    fn render_scribe_value_text(&mut self, value: MirOperand, ty: MirType, text_ty: MirType, span: Span) -> MirOperand {
        if ty == text_ty {
            return value;
        }
        match self.normalized_type(ty.semantic_id()) {
            Type::Primitive(Primitive::Textus) => value,
            // L28 (ab91f49f): `fractus<f32>` renders through the same
            // `display_fractus` FormatString carrier (the emitter/runtime f32
            // display ABI); `instans` renders its Rust-oracle Debug shape
            // through the opaque format carrier (the runtime's arena lookup
            // renders the handle like `__faber_rt_v1_instans_display`).
            // Opaque collections (lista, octeti, valor, tabula, copia) format
            // through the single-opaque-arg format carrier too, which every
            // MIR consumer renders in the Rust-oracle Debug shape.
            Type::Primitive(
                Primitive::Bivalens | Primitive::Fractus | Primitive::Instans | Primitive::Octeti | Primitive::Valor,
            )
            | Type::SizedNumeric(_, _)
            | Type::Bits(_)
            | Type::ModularWord(_)
            | Type::SaturatingWord(_)
            | Type::RangedInt { .. }
            | Type::PolicyFloat(_, _)
            | Type::SizedInstans(_)
            | Type::Array(_)
            | Type::BoundedArray { .. }
            | Type::RangedArray { .. }
            | Type::Map(_, _)
            | Type::Set(_) => self.runtime_call_value(
                MirIntrinsic::FormatString { template: self.context.diagnostic_format_templates[0] },
                vec![value],
                text_ty,
                span,
            ),
            Type::Primitive(Primitive::Numerus) => self.runtime_call_value(
                MirIntrinsic::Convert(MirConversion {
                    store: None,
                    flavor: MirConversionFlavor::Runtime,
                    source_ty: ty,
                    target_ty: text_ty,
                    params: Vec::new(),
                    recovery: None,
                    struct_defaults: Vec::new(),
                }),
                vec![value],
                text_ty,
                span,
            ),
            Type::Option(inner) => {
                let inner_ty = MirType::semantic(*inner);
                self.lower_option_scribe_text(value, inner_ty, text_ty, span)
            }
            _ => {
                // Unreachable for the grouped path (the pre-check admits only
                // renderable types); keep the MIR type-valid defensively.
                self.errors.push(MirError::unsupported(
                    span,
                    "scribe argument type outside the grouped display set",
                ));
                MirOperand::Constant(MirConstant::String(self.context.empty_textus))
            }
        }
    }

    /// Render an option (`T ∪ nihil`) scribe argument as textus: branch on the
    /// payload's presence, unwrap and render the payload when present, and
    /// substitute the literal `nihil` text when absent — mirroring the Rust
    /// oracle's `display_option` semantics.
    fn lower_option_scribe_text(
        &mut self,
        option: MirOperand,
        inner_ty: MirType,
        text_ty: MirType,
        span: Span,
    ) -> MirOperand {
        let bivalens = MirType::semantic(self.types.primitive(Primitive::Bivalens));
        let present = self.assign_temp(MirValueKind::Option(MirOptionOp::IsNotNil(option.clone())), bivalens, span);
        let result = self.push_temp(text_ty, span);
        let some_id = self.fresh_block(span);
        let nil_id = self.fresh_block(span);
        let join_id = self.fresh_block(span);
        self.terminate_current(
            MirTerminatorKind::Branch { condition: present, then_block: some_id, else_block: nil_id },
            span,
        );

        self.switch_to(some_id);
        {
            let payload = self.assign_temp(
                MirValueKind::Option(MirOptionOp::Unwrap { value: option, mode: MirOptionUnwrapMode::Assume }),
                inner_ty,
                span,
            );
            let rendered = self.render_scribe_value_text(payload, inner_ty, text_ty, span);
            let value = self.new_value(MirValueKind::Operand(rendered), text_ty, span);
            self.append_stmt(MirStatement {
                kind: MirStatementKind::Assign { place: MirPlace::temp(result), value },
                span,
            });
            self.terminate_current(MirTerminatorKind::Goto(join_id), span);
        }

        self.switch_to(nil_id);
        {
            let nihil = MirOperand::Constant(MirConstant::String(self.context.diagnostic_nihil));
            let value = self.new_value(MirValueKind::Operand(nihil), text_ty, span);
            self.append_stmt(MirStatement {
                kind: MirStatementKind::Assign { place: MirPlace::temp(result), value },
                span,
            });
            self.terminate_current(MirTerminatorKind::Goto(join_id), span);
        }

        self.switch_to(join_id);
        MirOperand::Temp(result)
    }

    /// Lower `adfirma` into a target-neutral assertion runtime intrinsic.
    ///
    /// The condition has already been typechecked as `bivalens`; MIR validation
    /// rechecks that invariant so backend probes can trust the first operand.
    pub(super) fn lower_adfirma(
        &mut self,
        cond: &HirExpression,
        message: Option<&HirExpression>,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let mut args = vec![self.lower_expr_value(cond)?];
        if let Some(message) = message {
            args.push(self.lower_expr_value(message)?);
        }
        let return_ty = self.expr_ty(expr)?;
        Some(self.runtime_call_value(MirIntrinsic::Assert, args, return_ty, expr.span))
    }

    /// Lower a string-template application to a format-string intrinsic.
    pub(super) fn lower_scriptum(
        &mut self,
        template: Symbol,
        args: &[HirExpression],
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let mut lowered_args = Vec::with_capacity(args.len());
        for arg in args {
            lowered_args.push(self.lower_expr_value(arg)?);
        }
        let ty = self.expr_ty(expr)?;
        Some(self.runtime_call_value(MirIntrinsic::FormatString { template }, lowered_args, ty, expr.span))
    }

    /// Lower `value ¶ "spec"` (D2.1) to the one-argument format runtime call.
    ///
    /// Typecheck validated the spec against the value's type; the executor
    /// re-parses the same literal text with the shared spec parser.
    pub(super) fn lower_format(
        &mut self,
        value: &HirExpression,
        spec: Symbol,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let value = self.lower_expr_value(value)?;
        let ty = self.expr_ty(expr)?;
        Some(self.runtime_call_value(MirIntrinsic::FormatValue { spec }, vec![value], ty, expr.span))
    }

    /// Lower runtime conversions, including optional inline recovery expressions.
    ///
    /// Aggregate `verte` construction is handled in `aggregate.rs`; this path
    /// represents conversion work that remains a runtime intrinsic.
    pub(super) fn lower_conversio(
        &mut self,
        source: &HirExpression,
        target: TypeId,
        params: &[Symbol],
        recovery: Option<&HirExpression>,
        implicit: Option<radix_hir::HirStoreSite>,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        if implicit.is_some()
            && let Some(folded) = self.fold_word_store_constant(source, target)
        {
            return Some(folded);
        }
        // A `copy`/`mut`/`own`/`ref` binding reads typed under its access
        // marker, but its MIR local carries the payload; the conversion
        // source is the payload, as the validator compares it to the operand.
        let source_ty = MirType::semantic(
            self.types
                .peel_access_marker(self.expr_ty(source)?.semantic_id())
                .1,
        );
        let source_type_id = source_ty.semantic_id();
        let ty = self.expr_ty(expr)?;
        let target = self.materialized_conversio_target(target, ty);
        if recovery.is_none()
            && matches!(self.normalized_type(source_type_id), Type::Intervallum(..))
            && matches!(self.normalized_type(target), Type::Array(_) | Type::Tensor(_, _))
        {
            let source = self.lower_expr_value(source)?;
            return Some(self.runtime_call_value(
                MirIntrinsic::Collection(MirCollectionOp::IntervalMaterialize),
                vec![source],
                ty,
                expr.span,
            ));
        }
        // `tensor<E, [N]> ↦ vector<E, size N>`: the tensor flattens to its element
        // lista and lane `k` reads slot `k`. A tensor source is not a literal,
        // so it never reaches the `lista` literal arm below.
        if recovery.is_none()
            && matches!(self.normalized_type(target), Type::Vector(_, _))
            && matches!(self.normalized_type(source_type_id), Type::Tensor(_, _))
        {
            return self.lower_tensor_to_vector_conversio(source, target, ty, expr.span);
        }
        // `tensor<E, [R, C]> ↦ matrix<E, [R, C]>`: flatten the tensor to its
        // element lista, then build the matrix row-major — cell `(r, c)` reads
        // flat slot `r * C + c`, the same order `MatrixValue` keeps and
        // `lower_matrix_transpone` builds. The opposite crossing
        // (`matrix ↦ tensor`) stays on the runtime conversio, which already
        // converts a register matrix like its flat row-major cell list.
        if recovery.is_none()
            && matches!(self.normalized_type(target), Type::Matrix(_, _))
            && matches!(self.normalized_type(source_type_id), Type::Tensor(_, _))
        {
            return self.lower_tensor_to_matrix_conversio(source, target, ty, expr.span);
        }
        if recovery.is_none()
            && matches!(self.normalized_type(target), Type::Vector(_, _))
            && let HirExpressionKind::Array(elements) = &source.kind
        {
            return self.lower_array(elements, expr, MirAggregateKind::Vector);
        }
        if recovery.is_none()
            && let Type::Matrix(_, shape) = self.normalized_type(target).clone()
            && let Some((rows, columns)) = radix_semantic::literal_matrix_shape(shape, self.types)
            && let HirExpressionKind::Array(elements) = &source.kind
        {
            return self.lower_matrix_from_nested_lista(elements, rows, columns, ty, expr);
        }
        // Route C (AEU-5): bounded numeric lista snapshots into a padded
        // tensor. The trailing `TensorFromFlat` fill operand is a real value —
        // the caller's `⊥` fill or the numeric zero — never a `MaybeUninit`
        // read; a `⊥` on the flat path below stays a failure recovery, so the
        // padded arm runs first and even with a recovery present.
        if self
            .padded_snapshot_elem(source, source_type_id, target)
            .is_some()
        {
            return self.lower_bounded_lista_to_padded_tensor_conversio(
                source,
                source_type_id,
                target,
                ty,
                recovery,
                expr.span,
            );
        }
        if recovery.is_none() && validate_lista_to_tensor_source(source_type_id, target, self.types).is_ok() {
            return self.lower_lista_to_tensor_conversio(source, target, ty, expr.span);
        }
        if recovery.is_none() && analyze_rectangular_lista_literal(source, target, self.types).is_ok() {
            return self.lower_rectangular_lista_literal_to_tensor_conversio(source, target, ty, expr.span);
        }

        // `octeti ↦ tensor<E, [.., size n]>`: the extents are values, not
        // type-table literals. The shape travels as a second operand, built
        // from the declared target (whose extents name the bindings); the
        // conversion itself targets the expression's own type, which is the
        // tensor type validation compares the result against.
        let declared_target = target;
        let runtime_extent_tensor = self.is_octeti_runtime_extent_tensor(source_type_id, declared_target);
        let target = if runtime_extent_tensor {
            ty.semantic_id()
        } else {
            declared_target
        };
        let conversion_source_ty = self.lowered_conversio_source_ty(source, source_type_id, target);
        let source = self.lower_conversio_source(source, target)?;
        let shape = if runtime_extent_tensor {
            Some(self.lower_octeti_tensor_shape(source.clone(), declared_target, expr.span)?)
        } else {
            None
        };
        let struct_defaults = self.valor_genus_struct_defaults(source_type_id, target, expr.span)?;
        let conversion_block = self.current;
        let lowered_recovery = match recovery {
            Some(recovery) => Some(self.lower_conversio_recovery(recovery, expr.span)?),
            None => None,
        };
        // An implicit store (d11-6 ruling 5) traps: it is never routed into an
        // enclosing `fac … cape` or `⇥` channel, only an authored `↦` is.
        if lowered_recovery.is_none()
            && implicit.is_none()
            && let Some(handler) = self.handlers.last().cloned()
        {
            return Some(self.lower_handled_conversio(HandledConversio {
                source,
                shape,
                source_ty: conversion_source_ty,
                target,
                params,
                return_ty: ty,
                handler,
                span: expr.span,
            }));
        }
        // A bare failable `sermo ↦ T` materialization failure crosses as the
        // leaf's `{kind, path}` record (`valor`). When the declared `⇥ E`
        // channel cannot carry that record but the registry enrolls a
        // `valor → E` row, the conversion runs in a synthetic `⇥ valor`
        // helper and the failable-call edge applies the row, exactly as a
        // failable call's declared-channel row does (CTR-02). An active
        // `capta` trap or `cape` handler above owns the edge and keeps the
        // bare conversion.
        if lowered_recovery.is_none()
            && implicit.is_none()
            && self.traps.is_empty()
            && let Some(row) = self.sermo_failure_row(source_type_id)
        {
            let param_tys: Vec<MirType> = std::iter::once(conversion_source_ty)
                .chain(shape.as_ref().map(|(_, ty)| *ty))
                .collect();
            let args: Vec<MirOperand> = std::iter::once(source)
                .chain(shape.map(|(operand, _)| operand))
                .collect();
            let value_ty = MirType::semantic(self.types.primitive(Primitive::Valor));
            let helper = self.push_conversio_helper(&param_tys, target, params, ty, value_ty, expr.span);
            return self.lower_failable_call(MirCallee::Function(helper), args, ty, value_ty, Some(row), expr.span);
        }
        let result = self.runtime_call_value(
            MirIntrinsic::Convert(MirConversion {
                store: implicit,
                flavor: MirConversionFlavor::Runtime,
                source_ty: conversion_source_ty,
                target_ty: MirType::semantic(target),
                params: params.to_vec(),
                recovery: lowered_recovery
                    .as_ref()
                    .map(|(operand, _)| operand.clone()),
                struct_defaults,
            }),
            std::iter::once(source)
                .chain(shape.map(|(operand, _)| operand))
                .collect(),
            ty,
            expr.span,
        );
        // A deferred recovery region stores the fallback into the conversion's
        // destination and both arms flow into one join, so every consumer
        // after the conversion reads a single joined temp.
        self.wire_conversio_recovery_tail(lowered_recovery.as_ref(), conversion_block, &result, ty, expr.span);
        Some(result)
    }

    /// Wire a deferred conversio recovery region (if any) into the CFG after the
    /// conversion statement. A sealed-block destination is reported and the
    /// result is left unwired.
    fn wire_conversio_recovery_tail(
        &mut self,
        lowered_recovery: Option<&(MirOperand, Option<MirBlockId>)>,
        conversion_block: Option<MirBlockId>,
        result: &MirOperand,
        ty: MirType,
        span: Span,
    ) {
        if let Some((recovery_value, Some(tail_block))) = lowered_recovery {
            let Some(conversion_block) = conversion_block else {
                self.errors
                    .push(MirError::unsupported(span, "conversio recovery after a sealed MIR block"));
                return;
            };
            self.wire_conversio_recovery_block(conversion_block, *tail_block, recovery_value.clone(), result, ty, span);
        }
    }

    /// An await-binding whose awaited value narrows into the declared slot
    /// (d11-6 §3(a) row 2). The awaited call already yields the success
    /// payload, so the payload is converted into the slot with the same
    /// checked `Convert` an implicit store node uses: it traps, and is never
    /// routed into an enclosing `fac … cape` or `⇥` channel.
    pub(super) fn lower_await_store(
        &mut self,
        init: &HirExpression,
        destination: MirPlace,
        slot_ty: MirType,
        site: radix_hir::HirStoreSite,
        span: Span,
    ) -> Option<()> {
        let init_ty = self.expr_ty(init)?.semantic_id();
        let awaited = match self.normalized_type(init_ty) {
            Type::Promissum(inner) | Type::PromissumFailable(inner, _) => *inner,
            _ => init_ty,
        };
        let future = self.lower_expr_value(init)?;
        // Materialize the payload into an awaited-typed temp, as `reddet`
        // does, so the conversion validates against the payload type.
        let source = self.assign_temp(MirValueKind::Operand(future), MirType::semantic(awaited), span);
        self.runtime_call_to_place(
            Some(destination),
            MirIntrinsic::Convert(MirConversion {
                store: Some(site),
                flavor: MirConversionFlavor::Runtime,
                source_ty: MirType::semantic(awaited),
                target_ty: slot_ty,
                params: Vec::new(),
                recovery: None,
                struct_defaults: Vec::new(),
            }),
            vec![source],
            slot_ty,
            span,
        );
        Some(())
    }

    /// A constant stored with `←` into a `wrapping` or `saturating` slot
    /// follows the slot's policy (d11-6 ruling 21, amended): the value is
    /// reduced or clamped here, at compile time, instead of by a runtime
    /// conversion that would range-check.
    fn fold_word_store_constant(&self, source: &HirExpression, target: TypeId) -> Option<MirOperand> {
        let value = radix_semantic::passes::const_int_value(source)?;
        // A ranged cell folds into its own range; a trapping one is judged by
        // the checker (the constant fits or the store is refused), so it never
        // reaches here.
        let (width, saturating, lo, hi) = match self.normalized_type(target) {
            Type::ModularWord(width) => {
                let (lo, hi) = width.integer_range()?;
                (*width, false, lo, hi)
            }
            Type::SaturatingWord(width) => {
                let (lo, hi) = width.integer_range()?;
                (*width, true, lo, hi)
            }
            Type::RangedInt { policy: radix_types::IntPolicy::Wrapping, width, lo, hi } => (*width, false, *lo, *hi),
            Type::RangedInt { policy: radix_types::IntPolicy::Saturating, width, lo, hi } => (*width, true, *lo, *hi),
            _ => return None,
        };
        let folded = if saturating {
            value.clamp(lo, hi)
        } else {
            lo + (value - lo).rem_euclid(hi - lo + 1)
        };
        // The constant is surfaced in the carrier of the width: unsigned
        // widths hold `UInt`, signed widths `Int`.
        Some(MirOperand::Constant(if width.integer_range()?.0 >= 0 {
            MirConstant::UInt(u64::try_from(folded).ok()?)
        } else {
            MirConstant::Int(i64::try_from(folded).ok()?)
        }))
    }

    /// Finalize the conversion target when semantic hole solving only updates
    /// the HIR result type. A surface `ascii<_, Radix>` target can retain its
    /// inference index in `HirConversioTarget`, while `expr.ty` already carries
    /// the literal capacity witnessed by the source. MIR validation requires
    /// the conversion target and runtime-call result to agree, so reuse that
    /// materialized result type for the bounded-ascii pack family only.
    fn materialized_conversio_target(&self, target: TypeId, return_ty: MirType) -> TypeId {
        let Type::BoundedAscii { capacity } = self.normalized_type(target) else {
            return target;
        };
        if matches!(self.types.get_index(*capacity), IndexExpr::Literal(_)) {
            return target;
        }

        let result = return_ty.semantic_id();
        let Type::BoundedAscii { capacity } = self.normalized_type(result) else {
            return target;
        };
        if matches!(self.types.get_index(*capacity), IndexExpr::Literal(_)) {
            result
        } else {
            target
        }
    }

    fn lower_conversio_source(&mut self, source: &HirExpression, target: TypeId) -> Option<MirOperand> {
        if let HirExpressionKind::Literal(HirLiteral::JsonValor(value)) = &source.kind
            && matches!(self.normalized_type(target), Type::Primitive(Primitive::Valor))
        {
            return self.lower_json_valor_literal(value, Some(target), source.span);
        }
        let operand = self.lower_expr_value(source)?;
        if let MirOperand::Place(place) = &operand
            && place.projections.is_empty()
            && let MirPlaceBase::Local(local) = place.base
            && self
                .lane_region
                .and_then(|region| region.coordinate_locals)
                .is_some_and(|(start, end)| local.0 >= start.0 && local.0 < end.0)
            && let Some(source_ty) = source.ty
            && matches!(
                self.types.get(source_ty),
                Type::SizedNumeric(Primitive::Numerus, radix_types::NumericWidth::I32)
            )
        {
            // Admission proves the coordinate fits i32. Only conversions need
            // this checked source view; ordinary indexing keeps the int binder.
            return Some(self.assign_temp(MirValueKind::Operand(operand), MirType::semantic(source_ty), source.span));
        }
        Some(operand)
    }

    /// CTO-3 — defer a SIDE-EFFECTING `⊥` default. A fallback containing a
    /// call must run ONLY when the conversion fails (fallback-not-run-on-
    /// success), so it lowers into a RECOVERY BLOCK: a real CFG block the
    /// conversion failure enters, carrying the whole expression (nested calls
    /// and conditionals included). Pure recovery expressions (literals, paths,
    /// call-free compound values) stay eagerly lowered — evaluating them is
    /// side-effect-free, so the success path stays byte-identical for them.
    ///
    /// Returns the recovery expression's value and, for the deferred shape,
    /// the region's tail block (where the fallback value is complete). The
    /// region is left open: the caller emits the conversion statement, then
    /// `wire_conversio_recovery_block` stores the value into the conversion's
    /// destination from the tail and joins both arms.
    fn lower_conversio_recovery(
        &mut self,
        recovery: &HirExpression,
        span: Span,
    ) -> Option<(MirOperand, Option<MirBlockId>)> {
        if !recovery_contains_call(recovery) {
            return Some((self.lower_expr_value(recovery)?, None));
        }
        let Some(main_block) = self.current else {
            self.errors
                .push(MirError::unsupported(span, "conversio recovery after a sealed MIR block"));
            return None;
        };
        let recovery_block = self.fresh_block(span);
        self.switch_to(recovery_block);
        let operand = self.lower_expr_value(recovery)?;
        let Some(tail_block) = self.current else {
            self.errors
                .push(MirError::unsupported(span, "conversio recovery after a sealed MIR block"));
            return None;
        };
        self.switch_to(main_block);
        Some((operand, Some(tail_block)))
    }

    /// Wire a deferred recovery region into the CFG after the conversion
    /// statement: the region tail stores the fallback value into the
    /// conversion's destination temp and jumps to a fresh join, and the
    /// conversion block jumps to the same join, so both arms rejoin in one
    /// block. The failure edge itself is carried by the statement's
    /// `recovery` operand (the runner, LLVM and Wasm read it); the MIR shows
    /// the recovery region as ordinary Goto-closed blocks instead of an
    /// `unreachable` orphan.
    fn wire_conversio_recovery_block(
        &mut self,
        conversion_block: MirBlockId,
        tail_block: MirBlockId,
        recovery_value: MirOperand,
        destination: &MirOperand,
        ty: MirType,
        span: Span,
    ) {
        let join_block = self.fresh_block(span);
        self.switch_to(tail_block);
        if let MirOperand::Temp(temp) = destination {
            let value = self.new_value(MirValueKind::Operand(recovery_value), ty, span);
            self.append_stmt(MirStatement {
                kind: MirStatementKind::Assign { place: MirPlace::temp(*temp), value },
                span,
            });
        }
        self.terminate_current(MirTerminatorKind::Goto(join_block), span);
        self.switch_to(conversion_block);
        self.terminate_current(MirTerminatorKind::Goto(join_block), span);
        self.switch_to(join_block);
    }

    fn lowered_conversio_source_ty(&self, source: &HirExpression, source_type_id: TypeId, target: TypeId) -> MirType {
        if matches!(
            (&source.kind, self.normalized_type(target)),
            (
                HirExpressionKind::Literal(HirLiteral::JsonValor(_)),
                Type::Primitive(Primitive::Valor)
            )
        ) {
            return MirType::semantic(target);
        }
        MirType::semantic(source_type_id)
    }

    pub(super) fn lower_ad_expr(
        &mut self,
        route: Symbol,
        opener: Option<&HirExpression>,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let return_ty = self.expr_ty(expr)?;
        let route_operand = MirOperand::Constant(MirConstant::String(route));
        let sermo = self.runtime_call_value(MirIntrinsic::SermoOpen, vec![route_operand], return_ty, expr.span);
        let Some(opener) = opener else {
            return Some(sermo);
        };

        let opener_operand = self.lower_expr_value(opener)?;
        let opener_ty = self.expr_ty(opener)?;
        let valor_ty = self.types.primitive(Primitive::Valor);
        let opener_valor = self.runtime_call_value(
            MirIntrinsic::Convert(MirConversion {
                store: None,
                flavor: MirConversionFlavor::Runtime,
                source_ty: opener_ty,
                target_ty: MirType::semantic(valor_ty),
                params: Vec::new(),
                recovery: None,
                struct_defaults: Vec::new(),
            }),
            vec![opener_operand],
            MirType::semantic(valor_ty),
            opener.span,
        );
        self.runtime_call_to_place(
            None,
            MirIntrinsic::SermoSetOpener,
            vec![sermo.clone(), opener_valor],
            MirType::semantic(self.types.primitive(Primitive::Vacuum)),
            expr.span,
        );
        Some(sermo)
    }

    fn valor_genus_struct_defaults(
        &mut self,
        source: TypeId,
        target: TypeId,
        span: Span,
    ) -> Option<Vec<MirStructFieldDefaults>> {
        if !matches!(self.types.get(source), Type::Primitive(Primitive::Valor)) {
            return Some(Vec::new());
        }
        let mut seen = FxHashSet::default();
        let mut defaults = Vec::new();
        self.collect_type_struct_defaults(target, &mut seen, &mut defaults, span)?;
        Some(defaults)
    }

    fn collect_type_struct_defaults(
        &mut self,
        ty: TypeId,
        seen: &mut FxHashSet<DefId>,
        defaults: &mut Vec<MirStructFieldDefaults>,
        span: Span,
    ) -> Option<()> {
        match self.types.get(ty).clone() {
            Type::Struct(def_id) => self.collect_struct_defaults(def_id, seen, defaults, span),
            Type::Array(inner)
            | Type::Map(_, inner)
            | Type::Set(inner)
            | Type::Tensor(inner, _)
            | Type::Option(inner) => self.collect_type_struct_defaults(inner, seen, defaults, span),
            _ => Some(()),
        }
    }

    fn collect_struct_defaults(
        &mut self,
        def_id: DefId,
        seen: &mut FxHashSet<DefId>,
        defaults: &mut Vec<MirStructFieldDefaults>,
        span: Span,
    ) -> Option<()> {
        if !seen.insert(def_id) {
            return Some(());
        }
        let fields = self.materialize_struct_fields(def_id, &[], aggregate::StructFieldFill::Construction, span)?;
        if !fields.is_empty() {
            defaults.push(MirStructFieldDefaults { def_id, fields });
        }
        let field_defs = self
            .context
            .structs
            .get(&def_id)
            .cloned()
            .unwrap_or_default();
        for field in field_defs {
            self.collect_type_struct_defaults(field.ty, seen, defaults, span)?;
        }
        Some(())
    }

    fn lower_lista_to_tensor_conversio(
        &mut self,
        source: &HirExpression,
        target: TypeId,
        return_ty: MirType,
        span: Span,
    ) -> Option<MirOperand> {
        let flat = self.lower_expr_value(source)?;
        if self.tensor_from_flat_is_handled(target) {
            let flat_ty = self.expr_ty(source)?;
            return self.lower_handled_tensor_from_flat(flat, flat_ty, target, return_ty, span);
        }
        self.lower_tensor_from_flat_conversio(flat, target, return_ty, span)
    }

    /// Whether a recovery-free `lista ↦ tensor` conversion runs inside a
    /// `fac … cape` handler or a `capta` boundary, against a target whose
    /// extents are all literal. Its count check is a recoverable failure (a
    /// length that does not fill the shape is a failed conversion), and the
    /// bare `TensorFromFlat` call has no error edge, so under a handler it goes
    /// through a failable helper (see [`Self::lower_handled_tensor_from_flat`]).
    /// A symbolic or run-time shape keeps the bare call: its operand reads
    /// package-finalized shape sizes in the caller, which a helper would hide
    /// from the finalizer. Device bodies keep the bare op.
    fn tensor_from_flat_is_handled(&self, target: TypeId) -> bool {
        self.device_role.is_none()
            && (!self.handlers.is_empty() || !self.traps.is_empty())
            && matches!(
                resolve_lista_to_tensor_shape(target, self.types),
                Some(ListaToTensorShape::Literal(_))
            )
    }

    /// `TensorFromFlat` inside a handler: the same seam as a bounded append
    /// (`is_bounded_append_under_handler`). The count check runs in a
    /// `(flat, shape) ⇥ textus` helper, so its mismatch leaves the helper
    /// frame as the `textus` error and the caller's `TryCall` routes it to the
    /// enclosing `cape` block or `capta` boundary.
    fn lower_handled_tensor_from_flat(
        &mut self,
        flat: MirOperand,
        flat_ty: MirType,
        target: TypeId,
        return_ty: MirType,
        span: Span,
    ) -> Option<MirOperand> {
        let shape = self.lower_lista_to_tensor_shape(flat.clone(), target, span)?;
        let shape_ty = MirType::semantic(lista_numerus_type(self.types)?);
        let textus = MirType::semantic(self.types.primitive(Primitive::Textus));
        let helper = self.push_tensor_from_flat_helper(flat_ty, shape_ty, return_ty, textus, span);
        self.lower_failable_call(MirCallee::Function(helper), vec![flat, shape], return_ty, textus, None, span)
    }

    /// The helper body, for `(flat, shape) ⇥ textus → tensor`: one
    /// `TensorFromFlat` over a placeholder receiver, the form the bare
    /// conversion builds.
    fn push_tensor_from_flat_helper(
        &mut self,
        flat_ty: MirType,
        shape_ty: MirType,
        return_ty: MirType,
        error_ty: MirType,
        span: Span,
    ) -> MirFunctionId {
        let key = SyntheticHelperKey::TensorFromFlat { flat_ty, shape_ty, return_ty, error_ty };
        if let Some(&id) = self.synthetic_helpers.get(&key) {
            return id;
        }
        #[expect(
            clippy::cast_possible_truncation,
            reason = "MirFunctionId is u32 by design; synthetic function count is bounded by u32::MAX"
        )]
        let id = MirFunctionId(self.parent_function_id.0 + 1 + self.synthetic_functions.len() as u32);
        let mut nested = FunctionBuilder::for_function(
            self.types,
            Some(error_ty),
            Self::clone_context(&self.context),
            id,
            self.device_role,
            None,
        );
        let mut locals = Vec::with_capacity(2);
        for ty in [flat_ty, shape_ty] {
            let local = nested.next_local_id();
            nested
                .params
                .push(MirParam { local, name: None, ty, mode: MirParamMode::Owned, span });
            nested
                .locals
                .push(MirLocalDecl { id: local, name: None, ty, mutable: false, span });
            locals.push(local);
        }
        let entry = nested.fresh_block(span);
        nested.switch_to(entry);
        let seed = nested.construct_temp(
            MirAggregateKind::Tensor,
            MirAggregateFields::Ordered(Vec::new()),
            return_ty,
            span,
        );
        let tensor = nested.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::TensorFromFlat),
            vec![
                seed,
                MirOperand::Place(MirPlace::local(locals[0])),
                MirOperand::Place(MirPlace::local(locals[1])),
            ],
            return_ty,
            span,
        );
        nested.terminate_current(MirTerminatorKind::Return(Some(tensor)), span);
        let id = Self::finish_synthetic_function(
            &mut self.errors,
            &mut self.synthetic_functions,
            id,
            nested,
            return_ty,
            error_ty,
            span,
        );
        self.synthetic_helpers.insert(key, id);
        id
    }

    /// `tensor<E, [N]> ↦ vector<E, size N>`: flatten the tensor to its element
    /// lista, then build lane `k` from slot `k` of that lista in order.
    fn lower_tensor_to_vector_conversio(
        &mut self,
        source: &HirExpression,
        target: TypeId,
        return_ty: MirType,
        span: Span,
    ) -> Option<MirOperand> {
        let Type::Vector(element, width) = self.normalized_type(target).clone() else {
            return None;
        };
        let Some(lanes) = radix_semantic::literal_vector_width(width, self.types) else {
            self.errors
                .push(MirError::unsupported(span, "vector width before MIR lowering"));
            return None;
        };
        let Some(flat_ty) = self.types.find_array(element) else {
            self.errors
                .push(MirError::unsupported(span, "vector element lista type before MIR lowering"));
            return None;
        };
        let flat_ty = MirType::semantic(flat_ty);
        let tensor = self.lower_expr_value(source)?;
        let flat = self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::TensorFlatten),
            vec![tensor],
            flat_ty,
            span,
        );
        let flat = self.value_base_place(flat, flat_ty, span);
        let fields = (0..lanes)
            .map(|lane| {
                MirAggregateItem::Operand(projected_operand(
                    &flat,
                    MirProjection::Index(MirOperand::Constant(MirConstant::Int(i64::from(lane)))),
                ))
            })
            .collect();
        Some(self.construct_temp(MirAggregateKind::Vector, MirAggregateFields::Ordered(fields), return_ty, span))
    }

    /// `tensor<E, [R, C]> ↦ matrix<E, [R, C]>`: flatten the tensor to its
    /// element lista, then build the matrix row-major — cell `(r, c)` reads
    /// flat slot `r * C + c`, the same order the runner's `MatrixValue` keeps
    /// and `lower_matrix_transpone` builds.
    fn lower_tensor_to_matrix_conversio(
        &mut self,
        source: &HirExpression,
        target: TypeId,
        return_ty: MirType,
        span: Span,
    ) -> Option<MirOperand> {
        let Type::Matrix(element, shape) = self.normalized_type(target).clone() else {
            return None;
        };
        let Some((rows, columns)) = radix_semantic::literal_matrix_shape(shape, self.types) else {
            self.errors
                .push(MirError::unsupported(span, "matrix shape before MIR lowering"));
            return None;
        };
        let Some(flat_ty) = self.types.find_array(element) else {
            self.errors
                .push(MirError::unsupported(span, "matrix element lista type before MIR lowering"));
            return None;
        };
        let flat_ty = MirType::semantic(flat_ty);
        let tensor = self.lower_expr_value(source)?;
        let flat = self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::TensorFlatten),
            vec![tensor],
            flat_ty,
            span,
        );
        let flat = self.value_base_place(flat, flat_ty, span);
        let mut fields = Vec::with_capacity(usize::from(rows) * usize::from(columns));
        for row in 0..rows {
            for column in 0..columns {
                let index = i64::from(row) * i64::from(columns) + i64::from(column);
                fields.push(MirAggregateItem::Operand(projected_operand(
                    &flat,
                    MirProjection::Index(MirOperand::Constant(MirConstant::Int(index))),
                )));
            }
        }
        Some(self.construct_temp(MirAggregateKind::Matrix, MirAggregateFields::Ordered(fields), return_ty, span))
    }

    /// The fill element of a Route C padded snapshot, or `None` when the
    /// conversion is not one. A bounded source of the tensor's own element
    /// qualifies by type; a list *literal* whose untyped constants adopt the
    /// tensor element (`[1.0, 2.0] ↦ tf32[2]`) qualifies by shape, because
    /// typecheck already admitted it through the same facts
    /// (`padded_literal_facts`) and refused every literal that does not adopt.
    fn padded_snapshot_elem(&self, source: &HirExpression, source_ty: TypeId, target: TypeId) -> Option<TypeId> {
        if let Some((elem, _)) = resolve_bounded_lista_padded_shape(source_ty, target, self.types) {
            return Some(elem);
        }
        if !matches!(source.kind, HirExpressionKind::Array(_)) {
            return None;
        }
        padded_literal_facts(source_ty, target, self.types).map(|(elem, _, _)| elem)
    }

    /// Route C padded snapshot: `lista<T, magnitudo N> ↦ tensor<T, [N]>` with the
    /// caller's fill (a `⊥` value, element-typed) or the numeric zero. The
    /// runtime `TensorFromFlat` pads slots `len..N` with that real value; the
    /// bounded carrier's dead slots are never read.
    fn lower_bounded_lista_to_padded_tensor_conversio(
        &mut self,
        source: &HirExpression,
        source_ty: TypeId,
        target: TypeId,
        return_ty: MirType,
        recovery: Option<&HirExpression>,
        span: Span,
    ) -> Option<MirOperand> {
        let elem = self.padded_snapshot_elem(source, source_ty, target)?;
        let flat = self.lower_expr_value(source)?;
        let fill = match recovery {
            Some(fill) => self.lower_expr_value(fill)?,
            None => MirOperand::Constant(numeric_zero_constant(elem, self.types)),
        };
        let shape = self.lower_lista_to_tensor_shape(flat.clone(), target, span)?;
        let seed = self.construct_temp(
            MirAggregateKind::Tensor,
            MirAggregateFields::Ordered(Vec::new()),
            return_ty,
            span,
        );
        Some(self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::TensorFromFlat),
            vec![seed, flat, shape, fill],
            return_ty,
            span,
        ))
    }

    fn lower_rectangular_lista_literal_to_tensor_conversio(
        &mut self,
        source: &HirExpression,
        target: TypeId,
        return_ty: MirType,
        span: Span,
    ) -> Option<MirOperand> {
        let rectangular = analyze_rectangular_lista_literal(source, target, self.types).ok()?;
        let flat_ty = self.types.find_array(rectangular.leaf_elem).or_else(|| {
            self.errors.push(MirError::unsupported(
                span,
                "lista element type missing before rectangular lista literal tensor lowering",
            ));
            None
        })?;
        let fields = rectangular
            .flat_leaves
            .into_iter()
            .map(|leaf| self.lower_expr_value(leaf).map(MirAggregateItem::Operand))
            .collect::<Option<Vec<_>>>()?;
        let flat = self.construct_temp(
            MirAggregateKind::Array,
            MirAggregateFields::Ordered(fields),
            MirType::semantic(flat_ty),
            span,
        );
        self.lower_tensor_from_flat_conversio(flat, target, return_ty, span)
    }

    fn lower_tensor_from_flat_conversio(
        &mut self,
        flat: MirOperand,
        target: TypeId,
        return_ty: MirType,
        span: Span,
    ) -> Option<MirOperand> {
        let shape = self.lower_lista_to_tensor_shape(flat.clone(), target, span)?;
        let seed = self.construct_temp(
            MirAggregateKind::Tensor,
            MirAggregateFields::Ordered(Vec::new()),
            return_ty,
            span,
        );
        Some(self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::TensorFromFlat),
            vec![seed, flat, shape],
            return_ty,
            span,
        ))
    }

    /// Whether `source_ty ↦ target` is the byte-string-to-tensor row with an
    /// extent that is bound at run time (`size n`). The runner reads a literal
    /// shape from the target type but cannot read a run-time one from it.
    fn is_octeti_runtime_extent_tensor(&self, source_ty: TypeId, target: TypeId) -> bool {
        let Type::Tensor(_, shape) = self.normalized_type(target) else {
            return false;
        };
        let octeti_source = match self.normalized_type(source_ty) {
            Type::Primitive(Primitive::Octeti) | Type::BoundedOcteti { .. } => true,
            Type::Array(element) => matches!(self.normalized_type(*element), Type::Bits(radix_types::BitsWidth::B8)),
            _ => false,
        };
        octeti_source && self.body_index_contains_runtime_extent(*shape, &mut FxHashSet::default(), true)
    }

    /// The shape operand of [`Self::is_octeti_runtime_extent_tensor`]: the
    /// same `lista<numerus>` the `TensorFromFlat` conversion carries, paired
    /// with its type so a handled-conversio helper can declare the parameter.
    fn lower_octeti_tensor_shape(
        &mut self,
        source: MirOperand,
        target: TypeId,
        span: Span,
    ) -> Option<(MirOperand, MirType)> {
        // The octeti row is the one lowering that reads a witnessed `size`
        // parameter's tensor-axis witness at run time: the
        // byte source carries no shape and the runner cannot read a run-time
        // shape from the target type. Only this path resolves witnessed
        // parameters as run-time extents.
        let shape = self.lower_lista_to_tensor_shape_with_expression(source, target, None, span, true)?;
        let shape_ty = MirType::semantic(lista_numerus_type(self.types)?);
        Some((shape, shape_ty))
    }

    /// The lista-source shape lowering shared by the `lista ↦ tensor` rows.
    /// A declared `Runtime` extent lowers to a run-time `TensorShapeSize`
    /// read; every symbolic axis — including a witnessed `size` parameter —
    /// stays a `ShapeSize` term that package finalization substitutes for a
    /// generic pin (GSF-2), so a witnessed parameter must not preempt it
    /// with a run-time witness read the finalizer cannot fold. The octeti
    /// row, which also reads witnessed parameters, goes through
    /// [`Self::lower_octeti_tensor_shape`] instead.
    fn lower_lista_to_tensor_shape(&mut self, flat: MirOperand, target: TypeId, span: Span) -> Option<MirOperand> {
        self.lower_lista_to_tensor_shape_with_expression(flat, target, None, span, false)
    }

    /// A target figura that is a single `list<int>` value name is a runtime
    /// value shape (`data ↦ tensor<f32, shape>` with no static witness): the
    /// shape operand is that list's value itself, and the runtime count check
    /// validates the flat length against the value's product. Scalar extents
    /// keep the per-axis reading below.
    fn lower_value_shape_operand(&self, target: TypeId) -> Option<MirOperand> {
        let Type::Tensor(_, shape) = self.normalized_type(target) else {
            return None;
        };
        // The figura bracket always wraps a tuple, so the single-value form
        // is `Tuple([extent])`.
        let shape = match self.types.get_index(*shape) {
            IndexExpr::Tuple(dims) if dims.len() == 1 => *dims.first()?,
            _ => *shape,
        };
        let IndexExpr::Runtime(RuntimeExtentId::ScalarBinding(def_id)) = self.types.get_index(shape) else {
            return None;
        };
        let binding = self.bindings.get(def_id)?;
        if !radix_types::numeric_width::is_integer_index_vector(self.types, binding.ty.semantic_id()) {
            return None;
        }
        Some(MirOperand::Place(MirPlace::local(binding.local)))
    }

    fn lower_lista_to_tensor_shape_with_expression(
        &mut self,
        flat: MirOperand,
        target: TypeId,
        expression_value: Option<&(u32, MirOperand)>,
        span: Span,
        witnessed_size_params: bool,
    ) -> Option<MirOperand> {
        let shape_type = lista_numerus_type(self.types).or_else(|| {
            self.errors.push(MirError::unsupported(
                span,
                "lista<numerus> shape type missing before lista-to-tensor MIR lowering",
            ));
            None
        })?;
        let shape_ty = MirType::semantic(shape_type);

        if let Some(shape_operand) = self.lower_value_shape_operand(target) {
            return Some(shape_operand);
        }

        let fields = match resolve_lista_to_tensor_shape(target, self.types)? {
            ListaToTensorShape::Literal(dims) => dims
                .into_iter()
                .map(|dim| MirAggregateItem::Operand(MirOperand::Constant(MirConstant::Int(dim))))
                .collect(),
            // GSF-2: a declared symbolic target (`[T, D]`) carries one
            // shape-size scalar per axis. Finalization substitutes each to
            // its concrete `fractus` value; the numerus conversion keeps the
            // `lista<numerus>` shape operand type-correct through validation
            // and runner execution — the flat length is never the shape.
            ListaToTensorShape::Symbolic { shape, rank } => {
                let dimensions = match self.types.get_index(shape) {
                    IndexExpr::Tuple(dimensions) => dimensions.clone(),
                    _ => vec![shape],
                };
                (0..rank)
                    .map(|axis| {
                        let axis_id = i64::try_from(axis).ok()?;
                        let dimension = *dimensions.get(axis)?;
                        let direct = match self.types.get_index(dimension) {
                            IndexExpr::Literal(value) => {
                                Some(MirOperand::Constant(MirConstant::Int(i64::try_from(*value).ok()?)))
                            }
                            IndexExpr::Runtime(RuntimeExtentId::ScalarBinding(binding)) => {
                                if let Some(place) = self.captured_bindings.get(binding) {
                                    Some(MirOperand::Place(place.clone()))
                                } else {
                                    self.bindings
                                        .get(binding)
                                        .map(|binding| MirOperand::Place(MirPlace::local(binding.local)))
                                }
                            }
                            _ => None,
                        };
                        let dim = if let Some(direct) = direct {
                            direct
                        } else {
                            let scalar = if self.body_index_contains_runtime_extent(
                                dimension,
                                &mut FxHashSet::default(),
                                witnessed_size_params,
                            ) {
                                let mut shape_arguments = Vec::new();
                                let expression = match self.lower_shape_expr(
                                    dimension,
                                    &mut shape_arguments,
                                    expression_value,
                                    ShapeRelationBoundary::FunctionEntry,
                                    0,
                                ) {
                                    Ok(expression) => expression,
                                    Err(message) => {
                                        self.errors.push(MirError::unsupported(span, message));
                                        return None;
                                    }
                                };
                                self.runtime_call_value(
                                    MirIntrinsic::TensorShapeSize(expression),
                                    shape_arguments,
                                    MirType::semantic(self.types.primitive(Primitive::Fractus)),
                                    span,
                                )
                            } else {
                                MirOperand::Constant(MirConstant::ShapeSize { shape, axis: Some(axis_id) })
                            };
                            let numerus = self.types.primitive(Primitive::Numerus);
                            self.runtime_call_value(
                                MirIntrinsic::Convert(MirConversion {
                                    store: None,
                                    flavor: MirConversionFlavor::Runtime,
                                    source_ty: MirType::semantic(self.types.primitive(Primitive::Fractus)),
                                    target_ty: MirType::semantic(numerus),
                                    params: Vec::new(),
                                    recovery: None,
                                    struct_defaults: Vec::new(),
                                }),
                                vec![scalar],
                                MirType::semantic(numerus),
                                span,
                            )
                        };
                        Some(MirAggregateItem::Operand(dim))
                    })
                    .collect::<Option<Vec<_>>>()?
            }
            ListaToTensorShape::InferOneD => {
                let len = self.runtime_call_value(
                    MirIntrinsic::Collection(MirCollectionOp::Length),
                    vec![flat],
                    MirType::semantic(self.types.primitive(Primitive::Numerus)),
                    span,
                );
                vec![MirAggregateItem::Operand(len)]
            }
        };

        Some(self.construct_temp(MirAggregateKind::Array, MirAggregateFields::Ordered(fields), shape_ty, span))
    }

    /// Lower the zero-arg generic `grid.formata<T>()` tensor view onto the
    /// runtime `tensor_reshape` call (Q-X10). The call's result type is the
    /// complete tensor type named by the type argument, so the receiver's
    /// literal extents lower to the `lista<numerus>` shape operand the runner
    /// expects; the one-arg `forma(shape)` method shares the op and keeps its
    /// existing path.
    fn lower_tensor_formata(&mut self, receiver: &HirExpression, expr: &HirExpression) -> Option<MirOperand> {
        let receiver_value = self.lower_expr_value(receiver)?;
        let expr_ty = self.expr_ty(expr)?;
        let shape = self.lower_lista_to_tensor_shape_with_expression(
            receiver_value.clone(),
            expr_ty.semantic_id(),
            None,
            expr.span,
            false,
        )?;
        Some(self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::TensorReshape),
            vec![receiver_value, shape],
            expr_ty,
            expr.span,
        ))
    }

    fn lower_handled_conversio(&mut self, request: HandledConversio<'_>) -> MirOperand {
        let HandledConversio { source, shape, source_ty, target, params, return_ty, handler, span } = request;
        let param_tys: Vec<MirType> = std::iter::once(source_ty)
            .chain(shape.as_ref().map(|(_, ty)| *ty))
            .collect();
        let args: Vec<MirOperand> = std::iter::once(source)
            .chain(shape.map(|(operand, _)| operand))
            .collect();
        let callee = self.push_conversio_helper(&param_tys, target, params, return_ty, handler.error_ty, span);
        let ok_block = self.fresh_block(span);
        if self.is_vacuum(return_ty) {
            self.terminate_current(
                MirTerminatorKind::TryCall {
                    destination: None,
                    callee: MirCallee::Function(callee),
                    args,
                    ok_block,
                    error_place: handler.error_place,
                    error_block: handler.error_block,
                },
                span,
            );
            self.switch_to(ok_block);
            return MirOperand::Constant(MirConstant::Unit);
        }

        let destination = self.push_temp(return_ty, span);
        self.terminate_current(
            MirTerminatorKind::TryCall {
                destination: Some(MirPlace::temp(destination)),
                callee: MirCallee::Function(callee),
                args,
                ok_block,
                error_place: handler.error_place,
                error_block: handler.error_block,
            },
            span,
        );
        self.switch_to(ok_block);
        MirOperand::Temp(destination)
    }

    /// A synthetic function that runs one `Convert` and returns its result.
    /// `param_tys` is the source type, then the shape type when the target
    /// has run-time extents; each is a parameter of the helper.
    fn push_conversio_helper(
        &mut self,
        param_tys: &[MirType],
        target: TypeId,
        params: &[Symbol],
        return_ty: MirType,
        error_ty: MirType,
        span: Span,
    ) -> MirFunctionId {
        let key = SyntheticHelperKey::Conversio {
            params: param_tys.to_vec(),
            target,
            conversion_params: params.to_vec(),
            return_ty,
            error_ty,
        };
        if let Some(&id) = self.synthetic_helpers.get(&key) {
            return id;
        }
        #[expect(
            clippy::cast_possible_truncation,
            reason = "MirFunctionId is u32 by design; synthetic function count is bounded by u32::MAX"
        )]
        let id = MirFunctionId(self.parent_function_id.0 + 1 + self.synthetic_functions.len() as u32);
        let mut nested = FunctionBuilder::for_function(
            self.types,
            Some(error_ty),
            Self::clone_context(&self.context),
            id,
            self.device_role,
            None,
        );
        let mut args = Vec::with_capacity(param_tys.len());
        for ty in param_tys {
            let param = nested.next_local_id();
            nested
                .params
                .push(MirParam { local: param, name: None, ty: *ty, mode: MirParamMode::Owned, span });
            nested
                .locals
                .push(MirLocalDecl { id: param, name: None, ty: *ty, mutable: false, span });
            args.push(MirOperand::Place(MirPlace::local(param)));
        }
        let source_ty = param_tys[0];

        let entry = nested.fresh_block(span);
        nested.switch_to(entry);
        let destination = nested.push_temp(return_ty, span);
        nested.runtime_call_to_place(
            Some(MirPlace::temp(destination)),
            MirIntrinsic::Convert(MirConversion {
                store: None,
                flavor: MirConversionFlavor::Runtime,
                source_ty,
                target_ty: MirType::semantic(target),
                params: params.to_vec(),
                recovery: None,
                struct_defaults: Vec::new(),
            }),
            args,
            return_ty,
            span,
        );
        nested.terminate_current(MirTerminatorKind::Return(Some(MirOperand::Temp(destination))), span);
        let blocks = nested.finish_blocks();
        self.errors.extend(nested.errors);
        self.synthetic_functions.push(MirFunction {
            id,
            source: None,
            name: None,
            params: nested.params,
            locals: nested.locals,
            temps: nested.temps,
            blocks,
            return_ty,
            error_ty: Some(error_ty),
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span,
        });
        self.synthetic_helpers.insert(key, id);
        id
    }

    /// The tensor type of a checked `ponde` / `accipe` receiver, under any
    /// `ref` / `mut` / `own` / `copy` access marker. The helper's parameter
    /// mode already carries the access; the call passes the bare tensor, so a
    /// marker left on the parameter type fails MIR verification.
    fn checked_tensor_receiver_ty(&mut self, receiver: &HirExpression) -> Option<MirType> {
        let ty = self.expr_ty(receiver)?;
        Some(MirType::semantic(self.method_receiver_payload_id(ty.semantic_id())))
    }

    /// Whether this `append` targets a fixed-capacity list while a `cape`
    /// handler or `capta` boundary is open. Device bodies keep the bare op.
    fn is_bounded_append_under_handler(&self, receiver: &HirExpression) -> bool {
        let Some(receiver_ty) = receiver.ty else {
            return false;
        };
        self.device_role.is_none()
            && (!self.handlers.is_empty() || !self.traps.is_empty())
            && matches!(
                self.types.get(self.method_receiver_payload_id(receiver_ty)),
                Type::BoundedArray { .. }
            )
    }

    /// The length range a push or removal method call is checked against:
    /// `(limit, is_push)` for `appende` / `immerge` on a range with an upper
    /// end (the maximum) and for `decapita` / `detrahe` on any range (the
    /// minimum). A `lo…∞` push has no limit. Device bodies keep the bare op.
    fn ranged_list_limit(&self, receiver: &HirExpression, op: MirCollectionOp) -> Option<u64> {
        if self.device_role.is_some() {
            return None;
        }
        let Type::RangedArray { min, max, .. } = self
            .types
            .get(self.method_receiver_payload_id(receiver.ty?))
        else {
            return None;
        };
        match op {
            MirCollectionOp::Append => *max,
            MirCollectionOp::RemoveFirst | MirCollectionOp::RemoveLast => Some(*min),
            _ => None,
        }
    }

    /// The checked helper of a length-ranged list push or removal, for
    /// `(in receiver[, item]) ⇥ textus`:
    ///
    /// ```text
    /// entry: len = Length(recv)
    ///        push: len >= max ? fail : ok          removal: len <= min ? fail : ok
    /// ok:    return Append(recv, item)  /  return RemoveFirst|RemoveLast(recv)
    /// fail:  ReturnError("list length range: push past the maximum" /
    ///                    "list length range: removal below the minimum")
    /// ```
    ///
    /// Only a checked op reaches the runtime, so a failed push or removal
    /// leaves the list untouched.
    #[expect(clippy::too_many_arguments, reason = "one helper-key field per argument")]
    fn push_ranged_list_helper(
        &mut self,
        op: MirCollectionOp,
        receiver_ty: MirType,
        item_ty: Option<MirType>,
        limit: u64,
        return_ty: MirType,
        error_ty: MirType,
        span: Span,
    ) -> MirFunctionId {
        let key = SyntheticHelperKey::RangedList { op, receiver_ty, item_ty, limit, return_ty, error_ty };
        if let Some(&id) = self.synthetic_helpers.get(&key) {
            return id;
        }
        let numerus = MirType::semantic(self.types.primitive(Primitive::Numerus));
        let bivalens = MirType::semantic(self.types.primitive(Primitive::Bivalens));
        #[expect(
            clippy::cast_possible_truncation,
            reason = "MirFunctionId is u32 by design; synthetic function count is bounded by u32::MAX"
        )]
        let id = MirFunctionId(self.parent_function_id.0 + 1 + self.synthetic_functions.len() as u32);
        let mut nested = FunctionBuilder::for_function(
            self.types,
            Some(error_ty),
            Self::clone_context(&self.context),
            id,
            self.device_role,
            None,
        );
        let mut locals = Vec::with_capacity(2);
        let params = std::iter::once((receiver_ty, MirParamMode::MutRef))
            .chain(item_ty.map(|item_ty| (item_ty, MirParamMode::Owned)));
        for (ty, mode) in params {
            let local = nested.next_local_id();
            nested
                .params
                .push(MirParam { local, name: None, ty, mode, span });
            nested
                .locals
                .push(MirLocalDecl { id: local, name: None, ty, mutable: false, span });
            locals.push(local);
        }
        let operands: Vec<MirOperand> = locals
            .iter()
            .map(|local| MirOperand::Place(MirPlace::local(*local)))
            .collect();
        let is_push = op == MirCollectionOp::Append;
        let limit_operand = MirOperand::Constant(MirConstant::Int(i64::try_from(limit).unwrap_or(i64::MAX)));

        let entry = nested.fresh_block(span);
        let ok = nested.fresh_block(span);
        let fail = nested.fresh_block(span);
        nested.switch_to(entry);
        let length = nested.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::Length),
            vec![operands[0].clone()],
            numerus,
            span,
        );
        let violated = nested.binary_temp(
            if is_push { MirBinOp::GtEq } else { MirBinOp::LtEq },
            length,
            limit_operand,
            bivalens,
            span,
        );
        nested.terminate_current(
            MirTerminatorKind::Branch { condition: violated, then_block: fail, else_block: ok },
            span,
        );

        nested.switch_to(ok);
        if is_push {
            nested.runtime_call_to_place(None, MirIntrinsic::Collection(op), operands, return_ty, span);
            nested.terminate_current(MirTerminatorKind::Return(None), span);
        } else {
            let removed = nested.runtime_call_value(MirIntrinsic::Collection(op), operands, return_ty, span);
            nested.terminate_current(MirTerminatorKind::Return(Some(removed)), span);
        }

        nested.switch_to(fail);
        let message = if is_push {
            nested.context.diagnostic_ranged_push_past_maximum
        } else {
            nested.context.diagnostic_ranged_remove_below_minimum
        };
        nested.terminate_current(
            MirTerminatorKind::ReturnError(MirOperand::Constant(MirConstant::String(message))),
            span,
        );

        let id = Self::finish_synthetic_function(
            &mut self.errors,
            &mut self.synthetic_functions,
            id,
            nested,
            return_ty,
            error_ty,
            span,
        );
        self.synthetic_helpers.insert(key, id);
        id
    }

    /// The helper body, for `(in receiver, item) ⇥ textus`: one `Append`.
    /// Its capacity failure leaves the helper frame as the `textus` error, so
    /// the caller's `TryCall` routes it to the enclosing handler.
    fn push_bounded_append_helper(
        &mut self,
        receiver_ty: MirType,
        item_ty: MirType,
        return_ty: MirType,
        error_ty: MirType,
        span: Span,
    ) -> MirFunctionId {
        let key = SyntheticHelperKey::BoundedAppend { receiver_ty, item_ty, return_ty, error_ty };
        if let Some(&id) = self.synthetic_helpers.get(&key) {
            return id;
        }
        #[expect(
            clippy::cast_possible_truncation,
            reason = "MirFunctionId is u32 by design; synthetic function count is bounded by u32::MAX"
        )]
        let id = MirFunctionId(self.parent_function_id.0 + 1 + self.synthetic_functions.len() as u32);
        let mut nested = FunctionBuilder::for_function(
            self.types,
            Some(error_ty),
            Self::clone_context(&self.context),
            id,
            self.device_role,
            None,
        );
        let mut locals = Vec::with_capacity(2);
        for (ty, mode) in [(receiver_ty, MirParamMode::MutRef), (item_ty, MirParamMode::Owned)] {
            let local = nested.next_local_id();
            nested
                .params
                .push(MirParam { local, name: None, ty, mode, span });
            nested
                .locals
                .push(MirLocalDecl { id: local, name: None, ty, mutable: false, span });
            locals.push(local);
        }
        let entry = nested.fresh_block(span);
        nested.switch_to(entry);
        nested.runtime_call_to_place(
            None,
            MirIntrinsic::Collection(MirCollectionOp::Append),
            vec![
                MirOperand::Place(MirPlace::local(locals[0])),
                MirOperand::Place(MirPlace::local(locals[1])),
            ],
            return_ty,
            span,
        );
        nested.terminate_current(MirTerminatorKind::Return(None), span);
        let blocks = nested.finish_blocks();
        self.errors.extend(nested.errors);
        self.synthetic_functions.push(MirFunction {
            id,
            source: None,
            name: None,
            params: nested.params,
            locals: nested.locals,
            temps: nested.temps,
            blocks,
            return_ty,
            error_ty: Some(error_ty),
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span,
        });
        self.synthetic_helpers.insert(key, id);
        id
    }

    /// K11: whether this tensor `ponde` / `accipe` method call is the checked
    /// (failable) form. Device bodies keep the bare op; so does any call the
    /// registry row does not mark failable, and a call whose literal index the
    /// checker proved in range (it cannot fail, so the call has no error
    /// channel and a `trap` around it carries no `textus` member).
    fn is_checked_tensor_method(&self, receiver: &HirExpression, method_name: &str, args: &[HirCallArg]) -> bool {
        self.device_role.is_none() && self.is_failable_tensor_method(receiver, method_name, args)
    }

    /// Whether this tensor `ponde` / `accipe` method call is failable at all:
    /// the registry row marks it failable and the checker did not prove the
    /// index in range. Ignores whether the body is a kernel.
    pub(super) fn is_failable_tensor_method(
        &self,
        receiver: &HirExpression,
        method_name: &str,
        args: &[HirCallArg],
    ) -> bool {
        let Some(receiver_ty) = receiver.ty else {
            return false;
        };
        let payload_id = self.method_receiver_payload_id(receiver_ty);
        let payload = self.types.get(payload_id);
        let Some(entry) = radix_intrinsics::lookup_method_for_type(payload, method_name, args.len()) else {
            return false;
        };
        if !entry.failable {
            return false;
        }
        let proven_in_range = entry
            .range_checked_arg
            .and_then(|position| args.get(position))
            .is_some_and(|arg| self.literal_index_in_static_bounds(payload_id, &arg.expr));
        !proven_in_range
    }

    /// The checker's static range proof (`classify_static_index_range`,
    /// literal-only): every coordinate is an integer literal inside the
    /// tensor's literal extents. The checker types such a call as non-failable,
    /// so lowering must not give it an error channel either.
    fn literal_index_in_static_bounds(&self, payload: TypeId, index: &HirExpression) -> bool {
        // The checker's static range proof (`classify_static_index_range`)
        // covers a `matrix` receiver on the same coordinate contract, so the
        // literal-in-bounds proof must agree for a matrix too; otherwise a
        // checker-infallible call would be given an error channel here.
        let (Type::Tensor(_, shape) | Type::Matrix(_, shape)) = self.types.get(payload) else {
            return false;
        };
        let bounds: Option<Vec<u64>> = match self.types.get_index(*shape) {
            IndexExpr::Literal(value) => Some(vec![*value]),
            IndexExpr::Tuple(dims) => dims
                .iter()
                .map(|dim| match self.types.get_index(*dim) {
                    IndexExpr::Literal(value) => Some(*value),
                    _ => None,
                })
                .collect(),
            _ => None,
        };
        let Some(bounds) = bounds else {
            return false;
        };
        let literal = |expr: &HirExpression| -> Option<i128> {
            match &expr.kind {
                HirExpressionKind::Literal(radix_hir::HirLiteral::Int(value)) => Some(i128::from(*value)),
                HirExpressionKind::Unary(radix_hir::HirUnOp::Neg, inner) => match &inner.kind {
                    HirExpressionKind::Literal(radix_hir::HirLiteral::Int(value)) => i128::from(*value).checked_neg(),
                    _ => None,
                },
                _ => None,
            }
        };
        let coordinates: Option<Vec<i128>> = match &index.kind {
            HirExpressionKind::Array(elements) => elements
                .iter()
                .map(|element| match element {
                    radix_hir::HirArrayElement::Expr(inner) => literal(inner),
                    _ => None,
                })
                .collect(),
            _ => literal(index).map(|value| vec![value]),
        };
        let Some(coordinates) = coordinates else {
            return false;
        };
        coordinates.len() == bounds.len()
            && coordinates
                .iter()
                .zip(&bounds)
                .all(|(value, bound)| u128::try_from(*value).is_ok_and(|value| value < u128::from(*bound)))
    }

    /// K11: lower the checked `ponde` store / `accipe` read as a failable call
    /// to a synthesized helper, exactly as handled conversio helpers are. The
    /// helper compares the index against the tensor shape in MIR; `TensorSet`
    /// and `TensorGet` keep their single (fatal) meaning.
    fn lower_checked_tensor_access(
        &mut self,
        op: MirCollectionOp,
        lowered_args: Vec<MirOperand>,
        param_tys: &[MirType],
        ty: MirType,
        span: Span,
    ) -> Option<MirOperand> {
        let textus = MirType::semantic(self.types.primitive(Primitive::Textus));
        let helper = self.push_checked_tensor_helper(op, param_tys, ty, textus, span)?;
        self.lower_failable_call(MirCallee::Function(helper), lowered_args, ty, textus, None, span)
    }

    /// The `at` helper, for `(in receiver, index-or-key) ⇥ textus` (need
    /// 20a8cc01): a list or text index outside `0‥length`, or a map key that is
    /// not present, leaves the helper as the error textus; otherwise the strict
    /// element `Index` read.
    fn push_checked_at_helper(
        &mut self,
        keyed: bool,
        param_tys: [MirType; 2],
        return_ty: MirType,
        error_ty: MirType,
        span: Span,
    ) -> MirFunctionId {
        let key = SyntheticHelperKey::CheckedAt { keyed, params: param_tys, return_ty, error_ty };
        if let Some(&id) = self.synthetic_helpers.get(&key) {
            return id;
        }
        let numerus = MirType::semantic(self.types.primitive(Primitive::Numerus));
        let bivalens = MirType::semantic(self.types.primitive(Primitive::Bivalens));
        #[expect(
            clippy::cast_possible_truncation,
            reason = "MirFunctionId is u32 by design; synthetic function count is bounded by u32::MAX"
        )]
        let id = MirFunctionId(self.parent_function_id.0 + 1 + self.synthetic_functions.len() as u32);
        let mut nested = FunctionBuilder::for_function(
            self.types,
            Some(error_ty),
            Self::clone_context(&self.context),
            id,
            self.device_role,
            None,
        );
        let mut locals = Vec::with_capacity(2);
        for (ty, mode) in param_tys
            .into_iter()
            .zip([MirParamMode::Ref, MirParamMode::Owned])
        {
            let local = nested.next_local_id();
            nested
                .params
                .push(MirParam { local, name: None, ty, mode, span });
            nested
                .locals
                .push(MirLocalDecl { id: local, name: None, ty, mutable: false, span });
            locals.push(local);
        }
        let receiver = MirOperand::Place(MirPlace::local(locals[0]));
        let index = MirOperand::Place(MirPlace::local(locals[1]));

        let entry = nested.fresh_block(span);
        let ok = nested.fresh_block(span);
        let fail = nested.fresh_block(span);
        nested.switch_to(entry);
        if keyed {
            let present = nested.runtime_call_value(
                MirIntrinsic::Collection(MirCollectionOp::Contains),
                vec![receiver.clone(), index.clone()],
                bivalens,
                span,
            );
            nested.terminate_current(
                MirTerminatorKind::Branch { condition: present, then_block: ok, else_block: fail },
                span,
            );
        } else {
            let upper = nested.fresh_block(span);
            let length = nested.runtime_call_value(
                MirIntrinsic::Collection(MirCollectionOp::Length),
                vec![receiver.clone()],
                numerus,
                span,
            );
            let negative = nested.binary_temp(
                MirBinOp::Lt,
                index.clone(),
                MirOperand::Constant(MirConstant::Int(0)),
                bivalens,
                span,
            );
            nested.terminate_current(
                MirTerminatorKind::Branch { condition: negative, then_block: fail, else_block: upper },
                span,
            );
            nested.switch_to(upper);
            let past_end = nested.binary_temp(MirBinOp::GtEq, index.clone(), length, bivalens, span);
            nested.terminate_current(
                MirTerminatorKind::Branch { condition: past_end, then_block: fail, else_block: ok },
                span,
            );
        }

        nested.switch_to(ok);
        let element = nested.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::Index),
            vec![receiver, index],
            return_ty,
            span,
        );
        nested.terminate_current(MirTerminatorKind::Return(Some(element)), span);

        nested.switch_to(fail);
        let message = MirOperand::Constant(MirConstant::String(nested.context.diagnostic_at_out_of_range));
        nested.terminate_current(MirTerminatorKind::ReturnError(message), span);

        let id = Self::finish_synthetic_function(
            &mut self.errors,
            &mut self.synthetic_functions,
            id,
            nested,
            return_ty,
            error_ty,
            span,
        );
        self.synthetic_helpers.insert(key, id);
        id
    }

    /// Whether this method's receiver is a `lista` — a run-time `Array` or a
    /// fixed-capacity `BoundedArray` — and never a `textus`, whose `sectio`
    /// keeps trimming quietly.
    /// The receiver type a checked window helper takes: a bounded cord has
    /// already been widened to `textus` by the text-receiver copy above.
    fn checked_window_receiver_ty(&mut self, receiver: &HirExpression) -> Option<MirType> {
        if self.is_textus_receiver(receiver) {
            return Some(MirType::semantic(self.types.primitive(Primitive::Textus)));
        }
        self.checked_tensor_receiver_ty(receiver)
    }

    fn is_textus_receiver(&self, receiver: &HirExpression) -> bool {
        receiver.ty.is_some_and(|ty| {
            matches!(
                self.types.get(self.method_receiver_payload_id(ty)),
                Type::Primitive(Primitive::Textus) | Type::BoundedTextus { .. } | Type::BoundedAscii { .. }
            )
        })
    }

    fn is_lista_receiver(&self, receiver: &HirExpression) -> bool {
        receiver.ty.is_some_and(|ty| {
            matches!(
                self.types.get(self.method_receiver_payload_id(ty)),
                Type::Array(_) | Type::BoundedArray { .. } | Type::RangedArray { .. }
            )
        })
    }

    /// The helper body, for `(in receiver, start, end) ⇥ textus` (`Slice`) and
    /// `(in receiver, count) ⇥ textus` (`Take`):
    ///
    /// ```text
    /// entry: len = Length(recv)
    ///        start < 0 ? fail : next
    /// next:  start > end ? fail : next        (Slice only)
    /// next:  end > len ? fail : ok
    /// ok:    return Slice(recv, start, end)
    /// fail:  ReturnError(FormatString("slice_out_of_range § § §", start, end, len))
    /// ```
    ///
    /// `Take` checks `0 <= count <= len` and reports `count` and `len`. The
    /// payload is the fault kind plus the numbers as data, never prose. Only a
    /// guarded trim reaches the runtime op, so the strict call keeps its quiet
    /// trimming semantics without the source surface ever trimming.
    fn push_checked_trim_helper(
        &mut self,
        op: MirCollectionOp,
        param_tys: &[MirType],
        return_ty: MirType,
        error_ty: MirType,
        span: Span,
    ) -> MirFunctionId {
        let key = SyntheticHelperKey::CheckedTrim { op, params: param_tys.to_vec(), return_ty, error_ty };
        if let Some(&id) = self.synthetic_helpers.get(&key) {
            return id;
        }
        let numerus = MirType::semantic(self.types.primitive(Primitive::Numerus));
        let bivalens = MirType::semantic(self.types.primitive(Primitive::Bivalens));
        #[expect(
            clippy::cast_possible_truncation,
            reason = "MirFunctionId is u32 by design; synthetic function count is bounded by u32::MAX"
        )]
        let id = MirFunctionId(self.parent_function_id.0 + 1 + self.synthetic_functions.len() as u32);
        let mut nested = FunctionBuilder::for_function(
            self.types,
            Some(error_ty),
            Self::clone_context(&self.context),
            id,
            self.device_role,
            None,
        );
        let mut locals = Vec::with_capacity(param_tys.len());
        for (index, &ty) in param_tys.iter().enumerate() {
            // The receiver is borrowed; the bounds are plain values.
            let mode = if index == 0 {
                MirParamMode::Ref
            } else {
                MirParamMode::Owned
            };
            let local = nested.next_local_id();
            nested
                .params
                .push(MirParam { local, name: None, ty, mode, span });
            nested
                .locals
                .push(MirLocalDecl { id: local, name: None, ty, mutable: false, span });
            locals.push(local);
        }
        let receiver = MirOperand::Place(MirPlace::local(locals[0]));
        let bounds: Vec<MirOperand> = locals[1..]
            .iter()
            .map(|&local| MirOperand::Place(MirPlace::local(local)))
            .collect();

        let entry = nested.fresh_block(span);
        let ok = nested.fresh_block(span);
        let fail = nested.fresh_block(span);
        nested.switch_to(entry);
        let length = nested.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::Length),
            vec![receiver.clone()],
            numerus,
            span,
        );
        let zero = MirOperand::Constant(MirConstant::Int(0));
        // `take` admits `0 <= count <= len`; `slice` also requires the window
        // to run forward, so an empty window at a legal position is fine.
        let checks = if op == MirCollectionOp::Take {
            vec![
                (bounds[0].clone(), MirBinOp::Lt, zero),
                (bounds[0].clone(), MirBinOp::Gt, length.clone()),
            ]
        } else {
            vec![
                (bounds[0].clone(), MirBinOp::Lt, zero),
                (bounds[0].clone(), MirBinOp::Gt, bounds[1].clone()),
                (bounds[1].clone(), MirBinOp::Gt, length.clone()),
            ]
        };
        for (index, (lhs, comparison, rhs)) in checks.iter().enumerate() {
            let condition = nested.binary_temp(*comparison, lhs.clone(), rhs.clone(), bivalens, span);
            let next = if index + 1 == checks.len() {
                ok
            } else {
                nested.fresh_block(span)
            };
            nested.terminate_current(
                MirTerminatorKind::Branch { condition, then_block: fail, else_block: next },
                span,
            );
            nested.switch_to(next);
        }

        nested.switch_to(ok);
        let mut call_args = vec![receiver];
        call_args.extend(bounds.iter().cloned());
        let trimmed = nested.runtime_call_value(MirIntrinsic::Collection(op), call_args, return_ty, span);
        nested.terminate_current(MirTerminatorKind::Return(Some(trimmed)), span);

        nested.switch_to(fail);
        let template = if op == MirCollectionOp::Take {
            nested.context.diagnostic_take_out_of_range
        } else {
            nested.context.diagnostic_slice_out_of_range
        };
        let mut payload = bounds;
        payload.push(length);
        let message = nested.runtime_call_value(MirIntrinsic::FormatString { template }, payload, error_ty, span);
        nested.terminate_current(MirTerminatorKind::ReturnError(message), span);

        Self::finish_synthetic_function(
            &mut self.errors,
            &mut self.synthetic_functions,
            id,
            nested,
            return_ty,
            error_ty,
            span,
        );
        self.synthetic_helpers.insert(key, id);
        id
    }

    /// The helper body, for `(in receiver, index[, value]) ⇥ textus`:
    ///
    /// ```text
    /// entry: shape = TensorShape(recv); rank = Length(shape)
    ///        Length(index) == rank ? cond : fail
    /// cond:  axis < rank ? body : ok
    /// body:  ix = index[axis]; dim = shape[axis]
    ///        ix < 0 ? fail : upper
    /// upper: ix >= dim ? fail : step
    /// step:  axis += 1; goto cond
    /// ok:    TensorSet(recv, index, value); return        (ponde)
    ///        return TensorGet(recv, index)                (accipe)
    /// fail:  ReturnError("tensor ponde|accipe invalid index")
    /// ```
    fn push_checked_tensor_helper(
        &mut self,
        op: MirCollectionOp,
        param_tys: &[MirType],
        return_ty: MirType,
        error_ty: MirType,
        span: Span,
    ) -> Option<MirFunctionId> {
        let key = SyntheticHelperKey::CheckedTensor { op, params: param_tys.to_vec(), return_ty, error_ty };
        if let Some(&id) = self.synthetic_helpers.get(&key) {
            return Some(id);
        }
        let numerus = MirType::semantic(self.types.primitive(Primitive::Numerus));
        let bivalens = MirType::semantic(self.types.primitive(Primitive::Bivalens));
        let Some(array) = self.types.find_array(numerus.semantic_id()) else {
            self.errors
                .push(MirError::missing_type(span, "tensor bounds check shape list"));
            return None;
        };
        let array_ty = MirType::semantic(array);
        #[expect(
            clippy::cast_possible_truncation,
            reason = "MirFunctionId is u32 by design; synthetic function count is bounded by u32::MAX"
        )]
        let id = MirFunctionId(self.parent_function_id.0 + 1 + self.synthetic_functions.len() as u32);
        let mut nested = FunctionBuilder::for_function(
            self.types,
            Some(error_ty),
            Self::clone_context(&self.context),
            id,
            self.device_role,
            None,
        );
        let receiver_mode = if op == MirCollectionOp::TensorSet {
            MirParamMode::MutRef
        } else {
            MirParamMode::Ref
        };
        let modes = [receiver_mode, MirParamMode::Owned, MirParamMode::Owned];
        let mut locals = Vec::with_capacity(3);
        for (&ty, mode) in param_tys.iter().zip(modes) {
            let local = nested.next_local_id();
            nested
                .params
                .push(MirParam { local, name: None, ty, mode, span });
            nested
                .locals
                .push(MirLocalDecl { id: local, name: None, ty, mutable: false, span });
            locals.push(local);
        }
        let axis = nested.next_local_id();
        nested
            .locals
            .push(MirLocalDecl { id: axis, name: None, ty: numerus, mutable: true, span });
        let receiver = MirOperand::Place(MirPlace::local(locals[0]));
        let index = MirOperand::Place(MirPlace::local(locals[1]));
        let axis_place = MirPlace::local(axis);

        let entry = nested.fresh_block(span);
        let cond = nested.fresh_block(span);
        let body = nested.fresh_block(span);
        let upper = nested.fresh_block(span);
        let step = nested.fresh_block(span);
        let ok = nested.fresh_block(span);
        let fail = nested.fresh_block(span);

        let (shape, rank) = emit_checked_tensor_helper_entry(
            &mut nested,
            &receiver,
            &index,
            axis_place.clone(),
            array_ty,
            numerus,
            bivalens,
            entry,
            cond,
            fail,
            span,
        );
        emit_checked_tensor_helper_loop(
            &mut nested,
            &index,
            shape,
            rank,
            axis_place,
            numerus,
            bivalens,
            cond,
            body,
            upper,
            step,
            ok,
            fail,
            span,
        );
        emit_checked_tensor_helper_result(&mut nested, op, receiver, index, &locals, return_ty, ok, fail, span);

        let id = Self::finish_synthetic_function(
            &mut self.errors,
            &mut self.synthetic_functions,
            id,
            nested,
            return_ty,
            error_ty,
            span,
        );
        self.synthetic_helpers.insert(key, id);
        Some(id)
    }

    /// Record a completed synthetic MIR function into the lowering's function
    /// arena, returning its id. Shared by the checked `at`, trim, and tensor
    /// helper builders.
    ///
    /// Takes the two arenas rather than `&mut self`: `nested` borrows
    /// `self.context`, so a receiver for the whole of `self` cannot be borrowed
    /// while it is live. Disjoint field borrows stay legal.
    fn finish_synthetic_function(
        errors: &mut Vec<MirError>,
        synthetic_functions: &mut Vec<MirFunction>,
        id: MirFunctionId,
        mut nested: FunctionBuilder<'_>,
        return_ty: MirType,
        error_ty: MirType,
        span: Span,
    ) -> MirFunctionId {
        let blocks = nested.finish_blocks();
        errors.extend(nested.errors);
        synthetic_functions.push(MirFunction {
            id,
            source: None,
            name: None,
            params: nested.params,
            locals: nested.locals,
            temps: nested.temps,
            blocks,
            return_ty,
            error_ty: Some(error_ty),
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span,
        });
        id
    }

    /// Emit a runtime call and return the operand shape promised by its return
    /// type.
    ///
    /// This keeps runtime-call lowering consistent across providers,
    /// diagnostics, formatting, and conversions: `vacuum` calls are statements,
    /// while value calls allocate one destination temp.
    pub(super) fn runtime_call_value(
        &mut self,
        intrinsic: MirIntrinsic,
        args: Vec<MirOperand>,
        return_ty: MirType,
        span: Span,
    ) -> MirOperand {
        if self.is_vacuum(return_ty) {
            self.runtime_call_to_place(None, intrinsic, args, return_ty, span);
            return MirOperand::Constant(MirConstant::Unit);
        }

        let temp = self.push_temp(return_ty, span);
        self.runtime_call_to_place(Some(MirPlace::temp(temp)), intrinsic, args, return_ty, span);
        MirOperand::Temp(temp)
    }

    pub(super) fn runtime_call_to_place(
        &mut self,
        destination: Option<MirPlace>,
        intrinsic: MirIntrinsic,
        args: Vec<MirOperand>,
        return_ty: MirType,
        span: Span,
    ) {
        self.append_stmt(MirStatement {
            kind: MirStatementKind::RuntimeCall { destination, call: MirRuntimeCall { intrinsic, args, return_ty } },
            span,
        });
    }
}

/// Emit the entry block of a checked tensor helper: bind the shape list and its
/// rank, require the index list to match the rank, seed the axis at zero, and
/// branch into the per-axis check loop or the failure block. Returns the
/// `(shape, rank)` operands the loop needs.
fn emit_checked_tensor_helper_entry(
    nested: &mut FunctionBuilder<'_>,
    receiver: &MirOperand,
    index: &MirOperand,
    axis: MirPlace,
    array_ty: MirType,
    numerus: MirType,
    bivalens: MirType,
    entry: MirBlockId,
    cond: MirBlockId,
    fail: MirBlockId,
    span: Span,
) -> (MirOperand, MirOperand) {
    nested.switch_to(entry);
    let shape = nested.runtime_call_value(
        MirIntrinsic::Collection(MirCollectionOp::TensorShape),
        vec![receiver.clone()],
        array_ty,
        span,
    );
    let rank = nested.runtime_call_value(
        MirIntrinsic::Collection(MirCollectionOp::Length),
        vec![shape.clone()],
        numerus,
        span,
    );
    let index_len = nested.runtime_call_value(
        MirIntrinsic::Collection(MirCollectionOp::Length),
        vec![index.clone()],
        numerus,
        span,
    );
    nested.assign(axis, MirOperand::Constant(MirConstant::Int(0)), numerus, span);
    let same_rank = nested.binary_temp(MirBinOp::Eq, index_len, rank.clone(), bivalens, span);
    nested.terminate_current(
        MirTerminatorKind::Branch { condition: same_rank, then_block: cond, else_block: fail },
        span,
    );
    (shape, rank)
}

/// Emit the per-axis bounds loop of a checked tensor helper: walk every axis,
/// reading the index coordinate and the shape extent, and branch to `fail` on
/// the first out-of-range coordinate or to `ok` when every axis is in range.
fn emit_checked_tensor_helper_loop(
    nested: &mut FunctionBuilder<'_>,
    index: &MirOperand,
    shape: MirOperand,
    rank: MirOperand,
    axis: MirPlace,
    numerus: MirType,
    bivalens: MirType,
    cond: MirBlockId,
    body: MirBlockId,
    upper: MirBlockId,
    step: MirBlockId,
    ok: MirBlockId,
    fail: MirBlockId,
    span: Span,
) {
    let axis_operand = MirOperand::Place(axis.clone());
    nested.switch_to(cond);
    let more = nested.binary_temp(MirBinOp::Lt, axis_operand.clone(), rank, bivalens, span);
    nested.terminate_current(
        MirTerminatorKind::Branch { condition: more, then_block: body, else_block: ok },
        span,
    );

    nested.switch_to(body);
    // Both lists are in range by construction (axis < rank == both lengths),
    // so the strict-element `Index` read cannot fault.
    let extent_at = |nested: &mut FunctionBuilder<'_>, list: MirOperand| {
        nested.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::Index),
            vec![list, axis_operand.clone()],
            numerus,
            span,
        )
    };
    let coordinate = extent_at(nested, index.clone());
    let extent = extent_at(nested, shape);
    let negative = nested.binary_temp(
        MirBinOp::Lt,
        coordinate.clone(),
        MirOperand::Constant(MirConstant::Int(0)),
        bivalens,
        span,
    );
    nested.terminate_current(
        MirTerminatorKind::Branch { condition: negative, then_block: fail, else_block: upper },
        span,
    );

    nested.switch_to(upper);
    let past_end = nested.binary_temp(MirBinOp::GtEq, coordinate, extent, bivalens, span);
    nested.terminate_current(
        MirTerminatorKind::Branch { condition: past_end, then_block: fail, else_block: step },
        span,
    );

    nested.switch_to(step);
    let next = nested.binary_temp(
        MirBinOp::Add,
        axis_operand,
        MirOperand::Constant(MirConstant::Int(1)),
        numerus,
        span,
    );
    nested.assign(axis, next, numerus, span);
    nested.terminate_current(MirTerminatorKind::Goto(cond), span);
}

/// Emit the terminal blocks of a checked tensor helper: `ok` performs the
/// `TensorSet` store (returning unit) or the `TensorGet` read, and `fail`
/// returns the `ponde`/`accipe` invalid-index error.
/// # Panics
///
/// A `TensorSet` helper body is `(in receiver, index, value)`, so its third
/// parameter local is present whenever this path is taken; the index at
/// `locals[2]` sits inside the `TensorSet` arm deliberately, because the
/// two-parameter form (`accipe`) never reaches it.
fn emit_checked_tensor_helper_result(
    nested: &mut FunctionBuilder<'_>,
    op: MirCollectionOp,
    receiver: MirOperand,
    index: MirOperand,
    locals: &[MirLocalId],
    return_ty: MirType,
    ok: MirBlockId,
    fail: MirBlockId,
    span: Span,
) {
    nested.switch_to(ok);
    if op == MirCollectionOp::TensorSet {
        let value = MirOperand::Place(MirPlace::local(locals[2]));
        nested.runtime_call_to_place(
            None,
            MirIntrinsic::Collection(MirCollectionOp::TensorSet),
            vec![receiver, index, value],
            return_ty,
            span,
        );
        nested.terminate_current(MirTerminatorKind::Return(None), span);
    } else {
        let element = nested.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::TensorGet),
            vec![receiver, index],
            return_ty,
            span,
        );
        nested.terminate_current(MirTerminatorKind::Return(Some(element)), span);
    }

    nested.switch_to(fail);
    let invalid_index = if op == MirCollectionOp::TensorSet {
        nested.context.diagnostic_ponde_invalid_index
    } else {
        nested.context.diagnostic_accipe_invalid_index
    };
    let message = MirOperand::Constant(MirConstant::String(invalid_index));
    nested.terminate_current(MirTerminatorKind::ReturnError(message), span);
}

/// Append a shape operand to `arguments`, deduplicating an existing equal
/// entry, and return its index. Errors when the operand list is not
/// addressable as a `u32`.
fn append_runtime_shape_argument(arguments: &mut Vec<MirOperand>, operand: MirOperand) -> Result<u32, String> {
    if let Some(position) = arguments.iter().position(|existing| existing == &operand) {
        return u32::try_from(position).map_err(|_| "runtime shape check has too many operands".to_owned());
    }
    let argument =
        u32::try_from(arguments.len()).map_err(|_| "runtime shape check has too many operands".to_owned())?;
    arguments.push(operand);
    Ok(argument)
}

fn mir_diagnostic_kind(kind: HirScribeKind) -> MirDiagnosticKind {
    match kind {
        HirScribeKind::Nota => MirDiagnosticKind::Nota,
        HirScribeKind::Vide => MirDiagnosticKind::Vide,
        HirScribeKind::Mone => MirDiagnosticKind::Mone,
        HirScribeKind::Scribe => MirDiagnosticKind::Scribe,
    }
}

#[derive(Clone, Copy)]
enum VectorMethodKind {
    Dot,
    Cross,
    Swizzle,
    Longitudo,
    Norm,
    Normalizata,
    Crea,
    Elementwise(MirLaneOp),
}

fn vector_method_kind(method_name: &str) -> Option<VectorMethodKind> {
    match method_name {
        "productum" => Some(VectorMethodKind::Dot),
        "transversum" => Some(VectorMethodKind::Cross),
        "swizzle" => Some(VectorMethodKind::Swizzle),
        "longitudo" => Some(VectorMethodKind::Longitudo),
        "norm" => Some(VectorMethodKind::Norm),
        "normalizata" => Some(VectorMethodKind::Normalizata),
        "crea" => Some(VectorMethodKind::Crea),
        "addita" => Some(VectorMethodKind::Elementwise(MirLaneOp::Add)),
        "subtracta" => Some(VectorMethodKind::Elementwise(MirLaneOp::Sub)),
        "multiplicata" => Some(VectorMethodKind::Elementwise(MirLaneOp::Mul)),
        "divisa" => Some(VectorMethodKind::Elementwise(MirLaneOp::Div)),
        _ => None,
    }
}

fn matrix_method_kind(method_name: &str) -> Option<MirLaneOp> {
    match method_name {
        "addita" => Some(MirLaneOp::Add),
        "subtracta" => Some(MirLaneOp::Sub),
        "multiplicata" => Some(MirLaneOp::Mul),
        "divisa" => Some(MirLaneOp::Div),
        _ => None,
    }
}

/// Glyph product → method-name mapping by operand family. Returns `None` for
/// combinations without a supported lowering (callers fail them closed).
///
/// The names are the shipped method twins (`productum`/`transversum`/
/// `matmul`/`applica`/`multiplicata`), so glyph lowering reuses the exact
/// vector/matrix/tensor machinery those methods already prove.
fn glyph_method_name(op: HirBinOp, lhs_ty: &Type, rhs_ty: &Type) -> Option<&'static str> {
    match (op, lhs_ty, rhs_ty) {
        (HirBinOp::Dot, Type::Vector(..), Type::Vector(..)) => Some("productum"),
        (HirBinOp::Dot, Type::Matrix(..), Type::Matrix(..)) | (HirBinOp::Dot, Type::Tensor(..), Type::Tensor(..)) => {
            Some("matmul")
        }
        (HirBinOp::Dot, Type::Matrix(..), Type::Vector(..)) => Some("applica"),
        (HirBinOp::Cross, Type::Vector(..), Type::Vector(..)) => Some("transversum"),
        (HirBinOp::Hadamard, Type::Vector(..), Type::Vector(..))
        | (HirBinOp::Hadamard, Type::Matrix(..), Type::Matrix(..))
        | (HirBinOp::Hadamard, Type::Tensor(..), Type::Tensor(..)) => Some("multiplicata"),
        (HirBinOp::HadamardDiv, Type::Vector(..), Type::Vector(..))
        | (HirBinOp::HadamardDiv, Type::Matrix(..), Type::Matrix(..))
        | (HirBinOp::HadamardDiv, Type::Tensor(..), Type::Tensor(..)) => Some("divisa"),
        (HirBinOp::Add, Type::Vector(..), Type::Vector(..))
        | (HirBinOp::Add, Type::Matrix(..), Type::Matrix(..))
        | (HirBinOp::Add, Type::Tensor(..), Type::Tensor(..)) => Some("addita"),
        (HirBinOp::Sub, Type::Vector(..), Type::Vector(..))
        | (HirBinOp::Sub, Type::Matrix(..), Type::Matrix(..))
        | (HirBinOp::Sub, Type::Tensor(..), Type::Tensor(..)) => Some("subtracta"),
        _ => None,
    }
}

fn vector_swizzle_lane(ch: char) -> Option<u8> {
    match ch {
        'x' => Some(0),
        'y' => Some(1),
        'z' => Some(2),
        'w' => Some(3),
        _ => None,
    }
}

/// Ops whose MIR validator rows require a `textus` receiver. A bounded cord
/// receiver must widen to the unbounded carrier before the runtime call.
fn is_text_receiver_op(op: MirCollectionOp) -> bool {
    matches!(
        op,
        MirCollectionOp::Slice
            | MirCollectionOp::Index
            | MirCollectionOp::Contains
            | MirCollectionOp::StartsWith
            | MirCollectionOp::EndsWith
            | MirCollectionOp::Split
            | MirCollectionOp::Uppercase
            | MirCollectionOp::Lowercase
            | MirCollectionOp::Trim
            | MirCollectionOp::Replace
            | MirCollectionOp::TextEscape
    )
}

/// Position of the structural shape argument for the tensor operations whose
/// source shape list may contain `_`. The resolved result type is the runtime
/// operand source after semantic shape inference has finished.
fn tensor_shape_hole_argument(op: MirCollectionOp, args: &[HirCallArg]) -> Option<usize> {
    let index = match op {
        MirCollectionOp::TensorReshape | MirCollectionOp::TensorExpand => 0,
        MirCollectionOp::TensorCreate | MirCollectionOp::TensorFromFlat => 1,
        _ => return None,
    };
    let HirExpressionKind::Array(elements) = &args.get(index)?.expr.kind else {
        return None;
    };
    elements
        .iter()
        .any(|element| matches!(element, HirArrayElement::Infer(_)))
        .then_some(index)
}

fn projected_operand(place: &MirPlace, projection: MirProjection) -> MirOperand {
    let mut projections = place.projections.clone();
    projections.push(projection);
    MirOperand::Place(MirPlace { base: place.base.clone(), projections })
}

fn vector_lane_operand(place: &MirPlace, lane: u8) -> MirOperand {
    projected_operand(place, MirProjection::VectorLane(lane))
}

pub(super) fn matrix_cell_operand(place: &MirPlace, row: u8, column: u8) -> MirOperand {
    projected_operand(place, MirProjection::MatrixCell { row, column })
}

#[cfg(test)]
#[path = "octeti_tensor_size_param_test.rs"]
mod octeti_tensor_size_param_test;

#[cfg(test)]
#[path = "sink_into_test.rs"]
mod sink_into_test;

#[cfg(test)]
#[path = "imported_default_test.rs"]
mod imported_default_test;

#[cfg(test)]
#[path = "unary_mut_param_test.rs"]
mod unary_mut_param_test;

#[cfg(test)]
#[path = "omitted_optional_test.rs"]
mod omitted_optional_test;
