//! Runtime-call, intrinsic, gradient, sermo, collection, map, and atomic checks.

use super::{
    DEFERRED_VECTOR_OP_SURVIVED, FunctionScope, GenericTypeBindings, MirValidationError, Validator,
};
use crate::lista_tensor_conversio::validate_lista_to_tensor_source;
use crate::{
    MirAtomicOp, MirBinOp, MirCollectionOp, MirConstant, MirConversion, MirDeviceBlockOp,
    MirFilumForm, MirGradientOp, MirIntrinsic, MirLayoutKind, MirOperand, MirPlace, MirRuntimeCall,
    MirSermoOp, MirShapeExpr, MirType,
};
use radix_lexer::Span;
use radix_types::{IndexExpr, NumericWidth, Primitive, Type, i64s_from_literal_index};

#[derive(Clone, Copy, PartialEq, Eq)]
enum ElementwiseCollectionKind {
    Array,
    Queue,
    Stack,
}

#[derive(Clone, Copy)]
struct TensorNormFlags {
    rank_1_or_2: bool,
    layernorm_arity_gate: bool,
    beta: bool,
}

struct TensorConstValidation {
    label: &'static str,
    name: &'static str,
    message: &'static str,
}

fn int_constant(constant: &MirConstant) -> Option<i64> {
    match constant {
        MirConstant::Int(value) => Some(*value),
        _ => None,
    }
}

fn float_constant(constant: &MirConstant) -> Option<f64> {
    match constant {
        MirConstant::Float(value) => Some(*value),
        _ => None,
    }
}

struct TensorNormSpec {
    label: &'static str,
    receiver_label: &'static str,
    flags: TensorNormFlags,
    gamma_message: &'static str,
    result_message: &'static str,
}

impl Validator<'_, '_> {
    fn validate_tensor_expand(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        call: &MirRuntimeCall,
        span: Span,
    ) {
        let Some(receiver) = call.args.first() else {
            return;
        };
        let Some(receiver_ty) = self.validate_operand(scope, receiver, span) else {
            return;
        };
        self.require_tensor_type(receiver_ty, span, "tensor expand receiver is not tensor");
        self.require_tensor_type(call.return_ty, span, "tensor expand result is not tensor");
        let Some(shape_ty) = call
            .args
            .get(1)
            .and_then(|shape| self.validate_operand(scope, shape, span))
        else {
            return;
        };
        if matches!(
            self.context.types.get(shape_ty.semantic_id()),
            Type::Tensor(_, _)
        ) {
            if crate::tensor_expansion::TensorExpansion::for_types(
                receiver_ty,
                shape_ty,
                call.return_ty,
                self.context,
            )
            .is_none()
            {
                self.error(
                    span,
                    "tensor expand tensor witness has no exact compatible static shape",
                );
            }
        } else if !radix_types::is_integer_index_vector(
            self.context.types.as_table(),
            shape_ty.semantic_id(),
        ) {
            self.error(
                span,
                "tensor expand target shape is not an integer lista or static tensor witness",
            );
        }
    }

    pub(crate) fn validate_runtime_call(
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

    pub(crate) fn validate_intrinsic(
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
            MirIntrinsic::FormatValue { .. } => {
                if call.args.len() != 1 {
                    self.error(span, "format_value runtime call takes one argument");
                }
                self.require_exact(
                    call.return_ty,
                    self.primitive(Primitive::Textus),
                    span,
                    "format_value runtime call does not return textus",
                );
            }
            MirIntrinsic::Convert(conversion) => {
                // A byte string read as a tensor with run-time extents
                // carries the target's shape as a second operand.
                if let [_, shape] = call.args.as_slice() {
                    self.validate_conversion_shape_operand(scope, conversion, shape, span);
                } else {
                    self.require_arg_count(call.args.len(), 1, span, "conversion runtime call");
                }
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
                    self.validate_runtime_conversion_route(
                        source_ty,
                        conversion.target_ty,
                        conversion.recovery.is_some(),
                        span,
                    );
                }
                // p17b (D1.11): a `⇥` literal recovery was range-checked
                // against the target by semantic analysis, but MIR integer
                // constants carry plain `numerus` / `numerus<u64>` types — a
                // fitting constant is the target-typed value, not a mismatch.
                if let Some(recovery) = &conversion.recovery
                    && let Some(recovery_ty) = self.validate_operand(scope, recovery, span)
                {
                    self.require_assignable_or_fitting_constant(
                        recovery,
                        recovery_ty,
                        conversion.target_ty,
                        span,
                        "conversion recovery type mismatch",
                    );
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
            MirIntrinsic::Provider(_) | MirIntrinsic::SermoOpen | MirIntrinsic::SermoSetOpener => {}
            MirIntrinsic::DeviceBlock(op) => {
                let expected = usize::from(matches!(op, MirDeviceBlockOp::Enter { .. }));
                self.require_arg_count(call.args.len(), expected, span, "device block marker");
                if let MirDeviceBlockOp::Enter { handler, .. } = op
                    && scope.function.block_by_id(*handler).is_none()
                {
                    self.error(span, "device block marker handler block is missing");
                }
            }
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
                            "GPU vector builtin call does not return vector<u32, size 3>",
                        );
                    }
                } else if !self.is_u32_scalar(call.return_ty) {
                    self.error(span, "GPU builtin call does not return u32");
                }
            }
            MirIntrinsic::CursorStream(_) => {
                if !matches!(
                    self.type_kind(call.return_ty),
                    Type::Array(_) | Type::Cursor(_) | Type::AsyncCursor(_, _)
                ) {
                    self.error(
                        span,
                        "cursor_stream runtime call does not return an array or async cursor type",
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
            MirIntrinsic::ReadAll => {
                self.require_arg_count(call.args.len(), 0, span, "read_all runtime call");
                match self.type_kind(call.return_ty) {
                    Type::Option(inner) => self.require_exact(
                        MirType::semantic(*inner),
                        self.primitive(Primitive::Textus),
                        span,
                        "read_all runtime call does not return textus option",
                    ),
                    _ => self.error(span, "read_all runtime call does not return textus option"),
                }
            }
            MirIntrinsic::Gradient(op) => {
                self.validate_gradient_call(scope, *op, call, span);
            }
            MirIntrinsic::TypeCheck(op) => {
                self.require_arg_count(call.args.len(), 1, span, "type_check runtime call");
                self.validate_mir_type(op.ty, span);
                self.require_exact(
                    call.return_ty,
                    self.primitive(Primitive::Bivalens),
                    span,
                    "type_check runtime call does not return bivalens",
                );
            }
            MirIntrinsic::TensorShapeCheck(check) => {
                self.require_exact(
                    call.return_ty,
                    self.primitive(Primitive::Vacuum),
                    span,
                    "tensor shape check does not return vacuum",
                );
                for relation in &check.relations {
                    self.validate_shape_expr(call, &relation.left, span);
                    self.validate_shape_expr(call, &relation.right, span);
                }
                for binding in &check.size_bindings {
                    self.validate_shape_expr(call, &binding.witness, span);
                }
            }
            MirIntrinsic::TensorShapeSize(expression) => {
                self.validate_shape_expr(call, expression, span);
                self.require_exact(
                    call.return_ty,
                    self.primitive(Primitive::Fractus),
                    span,
                    "tensor shape size does not return fractus",
                );
            }
            MirIntrinsic::TensorLimes(policy) => {
                self.require_arg_count(call.args.len(), 1, span, "tensor limes runtime call");
                let Some(receiver_ty) = call
                    .args
                    .first()
                    .and_then(|arg| self.validate_operand(scope, arg, span))
                else {
                    return;
                };
                self.require_tensor_type(receiver_ty, span, "tensor limes receiver is not tensor");
                self.require_tensor_type(call.return_ty, span, "tensor limes result is not tensor");
                if let crate::MirEdgePolicy::Custom {
                    policy_def,
                    remap,
                    transform,
                } = policy
                {
                    if !self
                        .context
                        .struct_fields
                        .get(policy_def)
                        .is_some_and(std::collections::HashMap::is_empty)
                    {
                        self.error(span, "tensor limes custom policy is not a zero-field class");
                    }
                    let Some(remap_function) = scope.functions_by_id.get(remap) else {
                        self.error(
                            span,
                            "tensor limes remap callback does not resolve to a function",
                        );
                        return;
                    };
                    if remap_function.params.len() == 2 {
                        if !matches!(self.type_kind(remap_function.params[0].ty), Type::Struct(def) if def == policy_def)
                        {
                            self.error(
                                span,
                                "tensor limes remap receiver does not match policy class",
                            );
                        }
                        if !self.context.types.equals(
                            remap_function.params[1].ty.semantic_id(),
                            remap_function.return_ty.semantic_id(),
                        ) {
                            self.error(span, "tensor limes remap coordinate type is not preserved");
                        }
                    } else {
                        self.error(
                            span,
                            "tensor limes remap callback has an invalid receiver/coordinate arity",
                        );
                    }
                    if let Some(transform_id) = transform {
                        let Some(transform_function) = scope.functions_by_id.get(transform_id)
                        else {
                            self.error(
                                span,
                                "tensor limes transform callback does not resolve to a function",
                            );
                            return;
                        };
                        if transform_function.params.len() == 3 {
                            if !matches!(self.type_kind(transform_function.params[0].ty), Type::Struct(def) if def == policy_def)
                            {
                                self.error(
                                    span,
                                    "tensor limes transform receiver does not match policy class",
                                );
                            }
                            if !self.context.types.equals(
                                transform_function.params[1].ty.semantic_id(),
                                remap_function.params[1].ty.semantic_id(),
                            ) {
                                self.error(
                                    span,
                                    "tensor limes transform coordinate type differs from remap",
                                );
                            }
                            if let Type::Tensor(element_ty, _) = self.type_kind(call.return_ty) {
                                let element_ty = MirType::semantic(*element_ty);
                                self.require_exact(
                                    transform_function.params[2].ty,
                                    element_ty,
                                    span,
                                    "tensor limes transform value type differs from tensor element",
                                );
                                self.require_exact(
                                    transform_function.return_ty,
                                    element_ty,
                                    span,
                                    "tensor limes transform result differs from tensor element",
                                );
                            }
                        } else {
                            self.error(span, "tensor limes transform callback has an invalid receiver/coordinate/value arity");
                        }
                    }
                }
            }
            // SFR-5 / C4-8: the filum reduction validates its surface shape.
            // The declared form is one source operand and a fractus broadcast
            // result (the L7 f32 device surface). The grid form is the source,
            // one extent per axis, and an optional identity, with a numeric
            // or bool scalar result. The declaration facts (term body,
            // coordinate binders, floor coherence) are the kernel-plan pass's
            // typed checks, never re-derived here.
            MirIntrinsic::FilumReduce(op) => {
                match op.form {
                    MirFilumForm::Declared => {
                        self.require_arg_count(call.args.len(), 1, span, "filum sum runtime call");
                    }
                    MirFilumForm::Grid | MirFilumForm::Nested => {
                        let rank = op.coordinates.len();
                        // Group key helpers follow the optional term helper;
                        // each key carries its captures and function constant.
                        let term_operands =
                            op.term_captures.map_or(0, |captures| captures as usize + 1);
                        let group_operands = op.group.as_ref().map_or(Some(0), |group| {
                            group.keys.iter().try_fold(0_usize, |count, key| {
                                count.checked_add(
                                    usize::try_from(key.captures).ok()?.checked_add(1)?,
                                )
                            })
                        });
                        let Some(group_operands) = group_operands else {
                            self.error(span, "filum group key operand count overflows usize");
                            return;
                        };
                        if rank == 0
                            || ![
                                1 + rank + term_operands + group_operands,
                                2 + rank + term_operands + group_operands,
                            ]
                            .contains(&call.args.len())
                        {
                            self.error(
                                span,
                                "filum grid reduce takes the source, one extent per axis, an optional identity, and its typed term and group helper captures",
                            );
                        }
                        if op.term_captures.is_some()
                            && (call.args.len() < group_operands + 1
                                || !matches!(
                                    call.args.get(call.args.len() - group_operands - 1),
                                    Some(MirOperand::Constant(MirConstant::Function(_)))
                                ))
                        {
                            self.error(span, "filum grid reduce term helper must carry its function constant before group helpers");
                        }
                        if let Some(group) = &op.group {
                            if op.form != MirFilumForm::Grid
                                || !matches!(
                                    op.op,
                                    crate::MirReduceOp::Sum | crate::MirReduceOp::Count
                                )
                                || group.keys.is_empty()
                                || group.keys.len() != group.dimensions.len()
                            {
                                self.error(span, "filum grouped grid reduction has an invalid operator or key shape");
                            }
                            let result_facts = match self.type_kind(call.return_ty) {
                                Type::Tensor(element, shape) => {
                                    let element_ty = MirType::semantic(*element);
                                    let result_element = self.type_kind(element_ty);
                                    let valid_element = match op.op {
                                        crate::MirReduceOp::Sum => matches!(
                                            result_element,
                                            Type::Primitive(
                                                Primitive::Fractus | Primitive::Numerus
                                            ) | Type::SizedNumeric(
                                                Primitive::Fractus | Primitive::Numerus,
                                                _
                                            )
                                        ),
                                        crate::MirReduceOp::Count => matches!(
                                            result_element,
                                            Type::Primitive(Primitive::Numerus)
                                                | Type::SizedNumeric(Primitive::Numerus, _)
                                        ),
                                        _ => false,
                                    };
                                    let dims = i64s_from_literal_index(*shape, &self.context.types);
                                    Some((element_ty, valid_element, dims))
                                }
                                _ => None,
                            };
                            let dims = if let Some((element_ty, valid_element, dims)) = result_facts
                            {
                                if element_ty != group.result_element_ty {
                                    self.error(span, "filum grouped grid reduction element type differs from its result type");
                                }
                                if !valid_element {
                                    self.error(span, "filum grouped grid reduction result element type is unsupported");
                                }
                                dims
                            } else {
                                None
                            };
                            let dimensions = group
                                .dimensions
                                .iter()
                                .map(|extent| i64::try_from(*extent).ok())
                                .collect::<Option<Vec<_>>>();
                            if dims != dimensions {
                                self.error(span, "filum grouped grid reduction result shape differs from its key shape");
                            }
                            for (axis, key) in group.keys.iter().enumerate() {
                                if !matches!(
                                    self.type_kind(key.key_ty),
                                    Type::Primitive(Primitive::Numerus)
                                        | Type::SizedNumeric(Primitive::Numerus, _)
                                ) {
                                    self.error(
                                        span,
                                        "filum group key helper result is not integer",
                                    );
                                }
                                let function_position = call
                                    .args
                                    .len()
                                    .checked_sub(group_operands)
                                    .and_then(|start| {
                                        group.keys.iter().take(axis).try_fold(
                                            start,
                                            |position, previous| {
                                                position.checked_add(
                                                    usize::try_from(previous.captures)
                                                        .ok()?
                                                        .checked_add(1)?,
                                                )
                                            },
                                        )
                                    })
                                    .and_then(|start| {
                                        start.checked_add(usize::try_from(key.captures).ok()?)
                                    });
                                let Some(function_position) = function_position else {
                                    self.error(
                                        span,
                                        "filum group key helper position overflows usize",
                                    );
                                    continue;
                                };
                                let Some(MirOperand::Constant(MirConstant::Function(function_id))) =
                                    call.args.get(function_position)
                                else {
                                    self.error(
                                        span,
                                        "filum group key helper must carry its function constant",
                                    );
                                    continue;
                                };
                                let helper = scope.functions_by_id.get(function_id).copied();
                                if helper.is_none_or(|function| {
                                    usize::try_from(key.captures)
                                        .ok()
                                        .and_then(|captures| captures.checked_add(rank))
                                        != Some(function.params.len())
                                        || function.return_ty != key.key_ty
                                }) {
                                    self.error(span, "filum group key helper signature differs from its typed captures");
                                }
                            }
                        }
                    }
                }
                for arg in &call.args {
                    self.validate_operand(scope, arg, span);
                }
                let scalar_result = match self.type_kind(call.return_ty) {
                    Type::Primitive(Primitive::Fractus)
                    | Type::SizedNumeric(Primitive::Fractus, _) => true,
                    Type::Primitive(Primitive::Numerus | Primitive::Bivalens)
                    | Type::SizedNumeric(Primitive::Numerus, _) => {
                        matches!(op.form, MirFilumForm::Grid | MirFilumForm::Nested)
                    }
                    Type::Tensor(_, _) if op.group.is_some() => {
                        op.form == MirFilumForm::Grid
                            && matches!(op.op, crate::MirReduceOp::Sum | crate::MirReduceOp::Count)
                    }
                    _ => false,
                };
                if !scalar_result {
                    self.error(
                        span,
                        "filum reduce runtime call does not return a numeric scalar",
                    );
                }
                if op.lane_width == 0 || !op.lane_width.is_power_of_two() {
                    self.error(span, "filum lane width must be a positive power of two");
                }
            }
            MirIntrinsic::Scan(op) => self.validate_scan_call(scope, op, call, span),
            // `copy` duplicate: one source operand; the result keeps the
            // source payload type (access markers are source-level wrappers).
            MirIntrinsic::Duplicate => {
                self.require_arg_count(call.args.len(), 1, span, "duplicate runtime call");
                if let Some(arg) = call.args.first() {
                    self.validate_operand(scope, arg, span);
                }
            }
            MirIntrinsic::Pow => {
                self.require_arg_count(call.args.len(), 2, span, "pow runtime call");
                for arg in &call.args {
                    if let Some(ty) = self.validate_operand(scope, arg, span)
                        && !self.is_float_operand_type(ty)
                    {
                        self.error(span, "pow operand is not fractus");
                    }
                }
                if !self.is_float_operand_type(call.return_ty) {
                    self.error(span, "pow result is not fractus");
                }
            }
            // K4 elementwise lift: the operator's arity of operands (tensors
            // or scalars, checked by the typechecker) and a tensor result.
            MirIntrinsic::TensorLift(op) => {
                self.require_arg_count(
                    call.args.len(),
                    op.arity(),
                    span,
                    "tensor lift runtime call",
                );
                for arg in &call.args {
                    self.validate_operand(scope, arg, span);
                }
                if op.returns_tensor() {
                    if !matches!(self.type_kind(call.return_ty), Type::Tensor(..)) {
                        self.error(span, "tensor lift runtime call does not return a tensor");
                    }
                } else if !matches!(
                    self.type_kind(call.return_ty),
                    Type::Primitive(Primitive::Bivalens)
                ) {
                    self.error(
                        span,
                        "tensor equality runtime call does not return bivalens",
                    );
                }
            }
            // Deferred vector method: operand arity and result shape are
            // checked, then the call is rejected by name. The expander
            // rewrites every VectorOp once widths are literal, so a survivor
            // here means an executable function skipped it.
            MirIntrinsic::VectorOp(op) => {
                self.require_arg_count(call.args.len(), op.arity(), span, "vector op runtime call");
                for arg in &call.args {
                    self.validate_operand(scope, arg, span);
                }
                if op.returns_vector()
                    && !matches!(self.type_kind(call.return_ty), Type::Vector(..))
                {
                    self.error(span, "vector op runtime call does not return a vector");
                }
                self.errors.push(MirValidationError::with_issue(
                    span,
                    DEFERRED_VECTOR_OP_SURVIVED,
                    "deferred vector op survives instantiation",
                ));
            }
        }
    }

    fn validate_shape_expr(
        &mut self,
        call: &MirRuntimeCall,
        expression: &MirShapeExpr,
        span: Span,
    ) {
        match expression {
            MirShapeExpr::Literal(_) => {}
            MirShapeExpr::Product(factors) => {
                for factor in factors {
                    self.validate_shape_expr(call, factor, span);
                }
            }
            MirShapeExpr::ScalarArgument { argument }
            | MirShapeExpr::TensorAxis { argument, .. }
            | MirShapeExpr::TensorFieldAxis { argument, .. } => {
                if *argument as usize >= call.args.len() {
                    self.error(span, "tensor shape check references a missing argument");
                }
            }
            MirShapeExpr::Affine { base, .. } => self.validate_shape_expr(call, base, span),
            MirShapeExpr::Divide { numerator, divisor } => {
                if *divisor == 0 {
                    self.error(span, "tensor shape check divides by zero");
                }
                self.validate_shape_expr(call, numerator, span);
            }
        }
    }

    pub(crate) fn validate_gradient_call(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        op: MirGradientOp,
        call: &MirRuntimeCall,
        span: Span,
    ) {
        let expected = match op {
            MirGradientOp::Read | MirGradientOp::Zero => 1,
            MirGradientOp::Create | MirGradientOp::Accumulate => 5,
        };
        self.require_arg_count(call.args.len(), expected, span, "gradient runtime call");

        match op {
            MirGradientOp::Read | MirGradientOp::Zero => {
                if let Some(handle_ty) = call
                    .args
                    .first()
                    .and_then(|arg| self.validate_operand(scope, arg, span))
                    && self.is_gradient_handle_type_invalid(handle_ty)
                {
                    self.error(span, "gradient handle type is not an opaque handle");
                }
            }
            MirGradientOp::Create => {
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

    pub(crate) fn is_gradient_handle_type_invalid(&self, ty: MirType) -> bool {
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

    /// The second operand of `octeti ↦ tensor<E, [.., size n]>`: an integer
    /// list the runner reads the extents from, since a run-time extent is a
    /// value and not a literal in the target type.
    fn validate_conversion_shape_operand(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        conversion: &MirConversion,
        shape: &MirOperand,
        span: Span,
    ) {
        let octeti_source = match self.type_kind(conversion.source_ty) {
            Type::Primitive(Primitive::Octeti) | Type::BoundedOcteti { .. } => true,
            Type::Array(element) => {
                matches!(
                    self.context.types.get(*element),
                    Type::Bits(radix_types::BitsWidth::B8)
                )
            }
            _ => false,
        };
        let tensor_target = matches!(self.type_kind(conversion.target_ty), Type::Tensor(_, _));
        if !(octeti_source && tensor_target) {
            self.error(
                span,
                "conversion shape operand is only valid for octeti to tensor",
            );
        }
        if let Some(shape_ty) = self.validate_operand(scope, shape, span)
            && !radix_types::numeric_width::is_integer_index_vector(
                self.context.types.as_table(),
                shape_ty.semantic_id(),
            )
        {
            self.error(span, "conversion shape operand is not an integer list");
        }
    }

    pub(crate) fn validate_conversion_struct_defaults(
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

    pub(crate) fn validate_sermo_call(
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
            | MirSermoOp::Flush
            | MirSermoOp::Finish
            | MirSermoOp::Close
            | MirSermoOp::CloseFailed
            | MirSermoOp::CloseError
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
            MirSermoOp::Send | MirSermoOp::Flush => {
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
            MirSermoOp::Close | MirSermoOp::CloseFailed => {
                self.require_exact(
                    call.return_ty,
                    self.primitive(Primitive::Bivalens),
                    span,
                    "sermo close result is not bivalens",
                );
            }
            MirSermoOp::CloseError => {
                self.require_exact(
                    call.return_ty,
                    self.primitive(Primitive::Textus),
                    span,
                    "sermo close error is not textus",
                );
            }
            MirSermoOp::Receive { .. } => {
                if !matches!(
                    self.type_kind(call.return_ty),
                    Type::Option(_) | Type::Union(_)
                ) {
                    self.error(span, "sermo receive result is not optional frame");
                }
            }
            MirSermoOp::Cursor { .. } => {
                if !matches!(self.type_kind(call.return_ty), Type::Cursor(_)) {
                    self.error(span, "sermo cursor result is not cursor");
                }
            }
            MirSermoOp::DrainCursor => {
                let Some(receiver_ty) = receiver_ty else {
                    return;
                };
                let element = match self.type_kind(receiver_ty) {
                    Type::Cursor(element) | Type::AsyncCursor(element, _) => *element,
                    _ => {
                        self.error(
                            span,
                            "sermo cursor drain receiver is not cursor or async cursor",
                        );
                        return;
                    }
                };
                let Type::Array(result_element) = self.type_kind(call.return_ty) else {
                    self.error(span, "sermo cursor drain result is not lista");
                    return;
                };
                if !self.context.types.is_assignable(element, *result_element) {
                    self.error(span, "sermo cursor drain element type mismatch");
                }
            }
        }
    }

    fn validate_scan_call(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        scan_op: &crate::MirScanOp,
        call: &MirRuntimeCall,
        span: Span,
    ) {
        if matches!(
            scan_op.op,
            crate::MirReduceOp::ArgMax | crate::MirReduceOp::ArgMin
        ) {
            self.error(span, "scan does not admit argmax or argmin");
        }
        if scan_op.total == crate::MirScanTotal::Exclusive
            && matches!(
                scan_op.op,
                crate::MirReduceOp::Max | crate::MirReduceOp::Min
            )
        {
            self.error(span, "exclusive scan does not admit max or min");
        }
        if scan_op.rank == 0 {
            self.error(span, "scan source rank must be positive");
        }
        self.validate_mir_type(scan_op.term_ty, span);
        self.validate_mir_type(scan_op.result_element_ty, span);
        if let Some(source_ty) = call
            .args
            .first()
            .and_then(|arg| self.validate_operand(scope, arg, span))
        {
            match self.type_kind(source_ty) {
                Type::Tensor(_, _) => {
                    if self
                        .static_tensor_rank(source_ty)
                        .is_some_and(|rank| rank != usize::from(scan_op.rank))
                    {
                        self.error(span, "scan rank differs from source tensor rank");
                    }
                }
                Type::Matrix(_, _) if scan_op.rank == 2 => {}
                _ => self.error(span, "scan source is not a ranked tensor"),
            }
        }
        match self.type_kind(call.return_ty) {
            Type::Tensor(element, _) => self.require_exact(
                MirType::semantic(*element),
                scan_op.result_element_ty,
                span,
                "scan result element type differs from its tensor type",
            ),
            _ => self.error(span, "scan result is not a tensor"),
        }

        let Ok(term_captures) = usize::try_from(scan_op.term_captures) else {
            self.error(span, "scan term capture count exceeds usize");
            return;
        };
        let group_operands = scan_op.group.as_ref().map_or(Some(0), |group| {
            group.axes.iter().try_fold(0_usize, |count, axis| {
                count.checked_add(usize::try_from(axis.captures).ok()?.checked_add(1)?)
            })
        });
        let Some(group_operands) = group_operands else {
            self.error(span, "scan group operand count overflows usize");
            return;
        };
        let Some(group_start) = call.args.len().checked_sub(group_operands) else {
            self.error(span, "scan group operands exceed call arguments");
            return;
        };
        let Some(term_function_position) = 1_usize
            .checked_add(term_captures)
            .filter(|position| *position < group_start)
        else {
            self.error(span, "scan term helper operands are missing");
            return;
        };
        let term_id = if let Some(MirOperand::Constant(MirConstant::Function(id))) =
            call.args.get(term_function_position)
        {
            *id
        } else {
            self.error(span, "scan term helper function is missing");
            return;
        };
        let Some(term) = scope.functions_by_id.get(&term_id) else {
            self.error(span, "scan term helper does not resolve to a function");
            return;
        };
        if term.params.len() != term_captures + usize::from(scan_op.rank) + 1 {
            self.error(
                span,
                "scan term helper must take captures, coordinates, and element",
            );
        }
        self.require_exact(
            term.return_ty,
            scan_op.term_ty,
            span,
            "scan term helper return type mismatch",
        );
        for (argument, parameter) in call.args[1..term_function_position]
            .iter()
            .zip(term.params.iter())
        {
            if let Some(actual) = self.validate_operand(scope, argument, span) {
                self.require_assignable(
                    actual,
                    parameter.ty,
                    span,
                    "scan term capture type mismatch",
                );
            }
        }
        if term_function_position + 1 != group_start {
            self.error(
                span,
                "scan call operands do not match the declared helper segments",
            );
            return;
        }

        let Some(group) = &scan_op.group else {
            return;
        };
        if group.axes.is_empty() {
            self.error(span, "scan group has no key axes");
            return;
        }
        let mut group_count = Some(1_u64);
        let mut position = group_start;
        for axis in &group.axes {
            if axis.extent == 0 || axis.min.checked_add(i128::from(axis.extent - 1)).is_none() {
                self.error(span, "scan group axis has an invalid finite domain");
            }
            group_count = group_count.and_then(|count| count.checked_mul(axis.extent));
            let Ok(captures) = usize::try_from(axis.captures) else {
                self.error(span, "scan key capture count exceeds usize");
                return;
            };
            self.validate_mir_type(axis.key_ty, span);
            if !matches!(
                self.type_kind(axis.key_ty),
                Type::SizedNumeric(Primitive::Numerus, width)
                    if width.is_signed_int() || width.is_unsigned_int()
            ) && !matches!(
                self.type_kind(axis.key_ty),
                Type::Primitive(Primitive::Numerus)
            ) {
                self.error(span, "scan key type is not an integer");
            }
            let Some(function_position) = position.checked_add(captures) else {
                self.error(span, "scan key helper position overflows usize");
                return;
            };
            let key_id = if let Some(MirOperand::Constant(MirConstant::Function(id))) =
                call.args.get(function_position)
            {
                *id
            } else {
                self.error(span, "scan key helper function is missing");
                return;
            };
            let Some(key_fn) = scope.functions_by_id.get(&key_id) else {
                self.error(span, "scan key helper does not resolve to a function");
                return;
            };
            if key_fn.params.len() != captures + usize::from(scan_op.rank) {
                self.error(span, "scan key helper must take captures and coordinates");
            }
            self.require_exact(
                key_fn.return_ty,
                axis.key_ty,
                span,
                "scan key helper return type mismatch",
            );
            for (argument, parameter) in call.args[position..function_position]
                .iter()
                .zip(key_fn.params.iter())
            {
                if let Some(actual) = self.validate_operand(scope, argument, span) {
                    self.require_assignable(
                        actual,
                        parameter.ty,
                        span,
                        "scan key capture type mismatch",
                    );
                }
            }
            position = function_position.saturating_add(1);
        }
        if group_count.is_none() {
            self.error(span, "scan group domain cardinality overflows u64");
        }
        if position != call.args.len() {
            self.error(span, "scan has trailing helper operands");
        }
    }

    pub(crate) fn validate_collection_call(
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
            | MirCollectionOp::ArgMax
            | MirCollectionOp::ArgMin
            | MirCollectionOp::TensorRank
            | MirCollectionOp::TensorNorm
            | MirCollectionOp::TensorShape
            | MirCollectionOp::TensorFlatten
            | MirCollectionOp::TensorMaterialize
            | MirCollectionOp::TensorMean
            | MirCollectionOp::TensorMax
            | MirCollectionOp::TensorMin
            | MirCollectionOp::TensorNeg
            | MirCollectionOp::ArrayNeg
            | MirCollectionOp::ArrayNot
            | MirCollectionOp::TensorMath(_)
            | MirCollectionOp::ScalarMath(_)
            | MirCollectionOp::TensorGelu
            | MirCollectionOp::TensorRelu
            | MirCollectionOp::TensorSoftmax
            | MirCollectionOp::TensorTranspose
            | MirCollectionOp::SparseNonzeroCount
            | MirCollectionOp::SparseDensify
            | MirCollectionOp::IntervalLength
            | MirCollectionOp::IntervalMaterialize
            | MirCollectionOp::Keys
            | MirCollectionOp::Values
            | MirCollectionOp::CordCursor
            | MirCollectionOp::TextEscape
            | MirCollectionOp::MatchText
            | MirCollectionOp::MatchStart
            | MirCollectionOp::MatchEnd
            | MirCollectionOp::TensorCausalMaskedSoftmax => 1,
            MirCollectionOp::ArrayAdd
            | MirCollectionOp::ArraySub
            | MirCollectionOp::ArrayMul
            | MirCollectionOp::ArrayDiv
            | MirCollectionOp::ArrayMod
            | MirCollectionOp::ArrayTrueDiv
            | MirCollectionOp::ArrayLt
            | MirCollectionOp::ArrayGt
            | MirCollectionOp::ArrayLtEq
            | MirCollectionOp::ArrayGtEq
            | MirCollectionOp::ArrayValueEq
            | MirCollectionOp::ArrayValueNotEq
            | MirCollectionOp::ArrayAnd
            | MirCollectionOp::ArrayOr
            | MirCollectionOp::Append
            | MirCollectionOp::SortByKeys
            | MirCollectionOp::Compare
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
            | MirCollectionOp::TensorSumAxis
            | MirCollectionOp::TensorReshape
            | MirCollectionOp::TensorExpand
            | MirCollectionOp::TensorShift
            | MirCollectionOp::TensorCopyInto
            | MirCollectionOp::TensorGet
            | MirCollectionOp::TensorFill
            | MirCollectionOp::TensorAdd
            | MirCollectionOp::TensorSub
            | MirCollectionOp::TensorMul
            | MirCollectionOp::TensorMatMul
            | MirCollectionOp::TensorCruxEntropia
            | MirCollectionOp::TensorSsmScan
            | MirCollectionOp::TensorDiv
            | MirCollectionOp::RegexMatches
            | MirCollectionOp::RegexFind
            | MirCollectionOp::RegexFindAll
            | MirCollectionOp::RegexSplit
            | MirCollectionOp::MatchGroup
            | MirCollectionOp::MatchNamed
            | MirCollectionOp::SparseGet
            | MirCollectionOp::IntervalIntersect
            | MirCollectionOp::IntervalUnion
            | MirCollectionOp::IntervalContains
            | MirCollectionOp::IntervalClamp => 2,
            // Route C (AEU-5): the padded bounded-lista snapshot appends a
            // trailing fill operand after [seed, flat, shape]; the flat path
            // keeps its exact-count 3-argument contract.
            MirCollectionOp::Put
            | MirCollectionOp::Slice
            | MirCollectionOp::Replace
            | MirCollectionOp::RegexReplace
            | MirCollectionOp::RegexReplaceEach
            | MirCollectionOp::TensorSet
            | MirCollectionOp::TensorCreate
            | MirCollectionOp::TensorSlice
            | MirCollectionOp::TensorQuantizedMatMul
            | MirCollectionOp::TensorGroupedMatMul
            | MirCollectionOp::TensorSsmConv1d
            | MirCollectionOp::SparseSet
            | MirCollectionOp::TensorFromFlat => 3,
            MirCollectionOp::TensorRmsNorm
            | MirCollectionOp::TensorRope
            | MirCollectionOp::TensorRopeHalf => 4,
            // [receiver, starts, ends, steps, keeps, inclusives]: one entry per axis.
            MirCollectionOp::TensorSliceNd => 6,
            // Compiler-owned M3 lowering carries tensor peers followed by a
            // variable const contract: projections may carry fused rope and
            // bias peers, while scores/context/softmax carry their normal
            // tensor inputs.
            MirCollectionOp::Approximate
            | MirCollectionOp::TensorLayerNorm
            | MirCollectionOp::TensorGather
            | MirCollectionOp::TensorScatterAdd
            | MirCollectionOp::TensorBatchedAttention => 0,
        };
        if matches!(op, MirCollectionOp::Approximate) {
            if !matches!(call.args.len(), 3 | 5) {
                self.error(span, "collection Approximate expects 3 or 5 MIR arguments");
            }
        } else if matches!(op, MirCollectionOp::TensorLayerNorm) {
            if !(3..=5).contains(&call.args.len()) {
                self.error(span, "collection TensorLayerNorm expects 3-5 MIR arguments");
            }
        } else if matches!(op, MirCollectionOp::TensorFromFlat) {
            // Padded form carries a trailing fill operand (AEU-5 Route C).
            if !(3..=4).contains(&call.args.len()) {
                self.error(
                    span,
                    "collection TensorFromFlat expects 3 or 4 MIR arguments",
                );
            }
        } else if matches!(op, MirCollectionOp::TensorGather) {
            // Dense gather is [table, ids]. Packed gather (R-PACK-02N) adds
            // an optional trailing GGML type-id const.
            if !(2..=3).contains(&call.args.len()) {
                self.error(span, "collection TensorGather expects 2 or 3 MIR arguments");
            }
        } else if matches!(op, MirCollectionOp::TensorScatterAdd) {
            // Scatter-add is [ids, updates]; the destination is implicit and
            // zero-initialized (the AIR reverse-AD Gather VJP contract).
            if call.args.len() != 2 {
                self.error(span, "collection TensorScatterAdd expects 2 MIR arguments");
            }
        } else if matches!(op, MirCollectionOp::TensorMax | MirCollectionOp::TensorMin) {
            // A caller-stated coalesce identity is an optional second scalar
            // operand; the no-identity static path carries only the tensor.
            if !(1..=2).contains(&call.args.len()) {
                self.error(
                    span,
                    format!("collection {op:?} expects 1 or 2 MIR arguments"),
                );
            }
        } else if matches!(op, MirCollectionOp::TensorSlice) {
            if !(3..=4).contains(&call.args.len()) {
                self.error(span, "collection TensorSlice expects 3 or 4 MIR arguments");
            }
        } else if matches!(op, MirCollectionOp::TensorSliceNd) {
            if call.args.len() != 6 {
                self.error(span, "collection TensorSliceNd expects 6 MIR arguments");
            }
        } else if matches!(op, MirCollectionOp::TensorBatchedAttention) {
            // This compiler-owned primitive has tensor peers followed by
            // shape/layout constants. Cache-aware projections may carry
            // x/weight, cos/sin, bias, a rank-3 cache view, and the B1
            // cursor (up to sixteen operands).
            if !(7..=16).contains(&call.args.len()) {
                self.error(
                    span,
                    "collection TensorBatchedAttention expects 7-16 MIR arguments",
                );
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
            MirCollectionOp::Compare => {
                self.require_exact(
                    call.return_ty,
                    self.primitive(Primitive::Numerus),
                    span,
                    "collection compare result is not numerus",
                );
                let second_arg_ty = call
                    .args
                    .get(1)
                    .and_then(|arg| self.validate_operand(scope, arg, span));
                if let (Some(lhs), Some(rhs)) = (first_arg_ty, second_arg_ty) {
                    self.require_assignable(rhs, lhs, span, "collection compare operands differ");
                }
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
                let (Type::Map(key_ty, value_ty) | Type::Ledger(key_ty, value_ty)) =
                    self.type_kind(collection_ty)
                else {
                    self.error(span, "collection put receiver is not a map");
                    return;
                };
                let key_ty = MirType::semantic(*key_ty);
                let value_ty = MirType::semantic(*value_ty);
                if let Some(key) = call.args.get(1)
                    && let Some(arg_ty) = self.validate_operand(scope, key, span)
                {
                    self.require_assignable_or_fitting_constant(
                        key,
                        arg_ty,
                        key_ty,
                        span,
                        "collection put key type mismatch",
                    );
                }
                if let Some(value) = call.args.get(2)
                    && let Some(arg_ty) = self.validate_operand(scope, value, span)
                {
                    self.require_assignable_or_fitting_constant(
                        value,
                        arg_ty,
                        value_ty,
                        span,
                        "collection put value type mismatch",
                    );
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
                            Type::Map(key, _) | Type::Ledger(key, _) => Some((
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
                    // Reverse/sort preserve the element type; the receiver's
                    // exact-bounds literal shape (BoundedArray) does not carry
                    // onto the ArrayInner result. Compare element types and
                    // fall back to the whole-type check for non-collections.
                    match (
                        self.collection_element_ty(call.return_ty),
                        self.collection_element_ty(collection_ty),
                    ) {
                        (Some(result_elem), Some(receiver_elem)) => {
                            self.require_assignable(
                                result_elem,
                                receiver_elem,
                                span,
                                "collection reverse/sort result type mismatch",
                            );
                        }
                        _ => {
                            self.require_assignable(
                                call.return_ty,
                                collection_ty,
                                span,
                                "collection reverse/sort result type mismatch",
                            );
                        }
                    }
                }
            }
            MirCollectionOp::SortByKeys => {
                if let Some(collection_ty) = first_arg_ty {
                    let keys_ty = call
                        .args
                        .get(1)
                        .and_then(|arg| self.validate_operand(scope, arg, span));
                    if keys_ty
                        .and_then(|ty| self.collection_element_ty(ty))
                        .is_none()
                    {
                        self.error(span, "collection sort_by_keys keys are not a lista");
                    }
                    match (
                        self.collection_element_ty(call.return_ty),
                        self.collection_element_ty(collection_ty),
                    ) {
                        (Some(result_elem), Some(receiver_elem)) => self.require_assignable(
                            result_elem,
                            receiver_elem,
                            span,
                            "collection sort_by_keys result type mismatch",
                        ),
                        _ => self.error(span, "collection sort_by_keys receiver is not a lista"),
                    }
                }
            }
            MirCollectionOp::Take | MirCollectionOp::TakeLast | MirCollectionOp::DropFirst => {
                if let Some(collection_ty) = first_arg_ty {
                    // A fixed-width receiver (BoundedArray) yields an
                    // ArrayInner copy/view: only the element type carries.
                    let (Type::Array(_) | Type::BoundedArray { .. } | Type::RangedArray { .. }) =
                        self.type_kind(collection_ty)
                    else {
                        self.error(span, "list copy/view receiver is not a lista");
                        return;
                    };
                    match (
                        self.collection_element_ty(call.return_ty),
                        self.collection_element_ty(collection_ty),
                    ) {
                        (Some(result_elem), Some(receiver_elem)) => self.require_assignable(
                            result_elem,
                            receiver_elem,
                            span,
                            "list copy/view result type mismatch",
                        ),
                        _ => self.require_assignable(
                            call.return_ty,
                            collection_ty,
                            span,
                            "list copy/view result type mismatch",
                        ),
                    }
                }
                if let Some(count) = call.args.get(1)
                    && let Some(count_ty) = self.validate_operand(scope, count, span)
                {
                    self.require_exact(
                        count_ty,
                        self.primitive(Primitive::Numerus),
                        span,
                        "list copy/view count is not numerus",
                    );
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
                // A `wrapping` or `saturating` word sums exactly like its
                // width, so a plain integer that holds the word's range is a
                // valid result: the store applies the word's policy.
                if !self.word_sum_widens(element_ty, call.return_ty) {
                    self.require_assignable(
                        element_ty,
                        call.return_ty,
                        span,
                        "collection sum result type mismatch",
                    );
                }
            }
            MirCollectionOp::ArgMax | MirCollectionOp::ArgMin => {
                // LSR-W0: the receiver is an array-backed collection of
                // ordered numeric elements; the result is the winning index
                // as `numerus ∪ nihil` — nihil exactly when the source is
                // empty, never a −1 sentinel.
                let Some(collection_ty) = first_arg_ty else {
                    return;
                };
                let inner = match self.type_kind(collection_ty) {
                    Type::Array(inner)
                    | Type::BoundedArray { element: inner, .. }
                    | Type::RangedArray { element: inner, .. } => *inner,
                    _ => {
                        self.error(span, "collection argmax/argmin receiver is not a lista");
                        return;
                    }
                };
                if !self.is_numeric_type(MirType::semantic(inner)) {
                    self.error(span, "collection argmax/argmin element is not numeric");
                    return;
                }
                match self.type_kind(call.return_ty) {
                    Type::Option(index)
                        if self
                            .context
                            .types
                            .equals(*index, self.context.types.primitive(Primitive::Numerus)) => {}
                    _ => self.error(
                        span,
                        "collection argmax/argmin result is not numerus ∪ nihil",
                    ),
                }
            }
            MirCollectionOp::TensorSumAxis => {
                let Some(receiver_ty) = first_arg_ty else {
                    return;
                };
                if !matches!(self.type_kind(receiver_ty), Type::Tensor(_, _)) {
                    self.error(span, "TensorSumAxis receiver is not a tensor");
                    return;
                }
                self.require_tensor_type(
                    call.return_ty,
                    span,
                    "TensorSumAxis result is not a tensor",
                );
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
                        // A standalone non-empty list literal types as the
                        // exact-length witness `lista<T, magnitudo N>`. Slicing it yields
                        // the unbounded list of the same element type.
                        Type::BoundedArray { element, .. } | Type::RangedArray { element, .. } => {
                            match self.type_kind(call.return_ty) {
                                Type::Array(inner) if *inner == *element => {}
                                _ => self.error(span, "list slice result type mismatch"),
                            }
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
                if let Some(arg) = call.args.get(1)
                    && let Some(arg_ty) = self.validate_operand(scope, arg, span)
                {
                    self.require_exact(
                        arg_ty,
                        self.primitive(Primitive::Textus),
                        span,
                        "text predicate argument is not textus",
                    );
                }
            }
            MirCollectionOp::RegexMatches => {
                self.require_exact(
                    call.return_ty,
                    self.primitive(Primitive::Bivalens),
                    span,
                    "regex consentit result is not bivalens",
                );
                if let Some(receiver_ty) = first_arg_ty {
                    self.require_exact(
                        receiver_ty,
                        self.primitive(Primitive::Regex),
                        span,
                        "regex consentit receiver is not regex",
                    );
                }
                if let Some(arg) = call.args.get(1)
                    && let Some(arg_ty) = self.validate_operand(scope, arg, span)
                {
                    self.require_exact(
                        arg_ty,
                        self.primitive(Primitive::Textus),
                        span,
                        "regex consentit argument is not textus",
                    );
                }
            }
            MirCollectionOp::RegexFind
            | MirCollectionOp::RegexFindAll
            | MirCollectionOp::RegexSplit
            | MirCollectionOp::MatchText
            | MirCollectionOp::MatchStart
            | MirCollectionOp::MatchEnd
            | MirCollectionOp::MatchGroup
            | MirCollectionOp::MatchNamed => {
                self.validate_regex_match_call(scope, op, first_arg_ty, call, span);
            }
            MirCollectionOp::RegexReplace | MirCollectionOp::TextEscape => {
                self.validate_regex_text_call(scope, op, first_arg_ty, call, span);
            }
            MirCollectionOp::RegexReplaceEach => {
                self.validate_regex_replace_each_call(scope, first_arg_ty, call, span);
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
                if let Some(separator) = call.args.get(1)
                    && let Some(separator_ty) = self.validate_operand(scope, separator, span)
                {
                    self.require_exact(
                        separator_ty,
                        self.primitive(Primitive::Textus),
                        span,
                        "text split separator is not textus",
                    );
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
            MirCollectionOp::ScalarMath(_) => {
                let Some(receiver_ty) = first_arg_ty else {
                    return;
                };
                if !matches!(
                    self.type_kind(receiver_ty),
                    Type::Primitive(Primitive::Fractus) | Type::SizedNumeric(Primitive::Fractus, _)
                ) {
                    self.error(span, "scalar math receiver is not fractus");
                    return;
                }
                self.require_exact(
                    call.return_ty,
                    receiver_ty,
                    span,
                    "scalar math result type mismatch",
                );
            }
            MirCollectionOp::Absolute | MirCollectionOp::Signum => {
                let Some(receiver_ty) = first_arg_ty else {
                    return;
                };
                let integer_receiver = matches!(
                    self.type_kind(receiver_ty),
                    Type::Primitive(Primitive::Numerus) | Type::SizedNumeric(Primitive::Numerus, _)
                );
                if !integer_receiver
                    && !matches!(
                        self.type_kind(receiver_ty),
                        Type::Primitive(Primitive::Fractus)
                            | Type::SizedNumeric(Primitive::Fractus, _)
                    )
                {
                    self.error(span, "numeric intrinsic receiver is not numerus or fractus");
                    return;
                }
                // `|x|` of an integer is typed by the range rule (`|int<i8>|`
                // is `u8`), so the result only has to be an integer type.
                if integer_receiver && matches!(op, MirCollectionOp::Absolute) {
                    if !matches!(
                        self.type_kind(call.return_ty),
                        Type::Primitive(Primitive::Numerus)
                            | Type::SizedNumeric(Primitive::Numerus, _)
                    ) {
                        self.error(span, "numeric intrinsic result type mismatch");
                    }
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
                // K4: a tensor extremum takes a scalar element or a same-shape
                // tensor peer; the result element is their join, so only the
                // tensor shape of the result and peer is validated here.
                if matches!(self.type_kind(receiver_ty), Type::Tensor(..)) {
                    if !matches!(self.type_kind(call.return_ty), Type::Tensor(..)) {
                        self.error(span, "numeric clamp result type mismatch");
                    }
                    let element_ty = match self.type_kind(receiver_ty) {
                        Type::Tensor(element, _) => MirType::semantic(*element),
                        _ => receiver_ty,
                    };
                    if let Some(arg) = call.args.get(1)
                        && let Some(actual_ty) = self.validate_operand(scope, arg, span)
                        && !matches!(self.type_kind(actual_ty), Type::Tensor(..))
                    {
                        self.require_assignable(
                            actual_ty,
                            element_ty,
                            span,
                            "numeric clamp argument type mismatch",
                        );
                    }
                    return;
                }
                let arg_ty = match self.type_kind(receiver_ty) {
                    Type::Primitive(Primitive::Numerus | Primitive::Fractus)
                    | Type::SizedNumeric(Primitive::Numerus | Primitive::Fractus, _) => receiver_ty,
                    // Tensors returned above; a lista receiver clamps by its
                    // element type.
                    Type::Array(element)
                    | Type::BoundedArray { element, .. }
                    | Type::RangedArray { element, .. } => MirType::semantic(*element),
                    _ => {
                        self.error(
                            span,
                            "numeric clamp receiver is not scalar, lista, or tensor",
                        );
                        return;
                    }
                };
                if !self
                    .context
                    .types
                    .is_assignable(call.return_ty.semantic_id(), receiver_ty.semantic_id())
                    || !self
                        .context
                        .types
                        .is_assignable(receiver_ty.semantic_id(), call.return_ty.semantic_id())
                {
                    self.error(span, "numeric clamp result type mismatch");
                }
                if let Some(arg) = call.args.get(1)
                    && let Some(actual_ty) = self.validate_operand(scope, arg, span)
                {
                    self.require_assignable(
                        actual_ty,
                        arg_ty,
                        span,
                        "numeric clamp argument type mismatch",
                    );
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
                if call.args.len() == 5 {
                    if !matches!(
                        self.type_kind(receiver_ty),
                        Type::Primitive(Primitive::Numerus | Primitive::Fractus)
                            | Type::SizedNumeric(Primitive::Numerus | Primitive::Fractus, _)
                            | Type::PolicyFloat(_, _)
                    ) {
                        self.error(span, "fuzzy comparison receiver is not numeric");
                    }
                    for arg in call.args.iter().skip(1) {
                        self.validate_operand(scope, arg, span);
                    }
                } else {
                    if !matches!(
                        self.type_kind(receiver_ty),
                        Type::Primitive(Primitive::Fractus)
                            | Type::SizedNumeric(Primitive::Fractus, _)
                            | Type::PolicyFloat(_, _)
                    ) {
                        self.error(span, "approximate comparison receiver is not fractus");
                        return;
                    }
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
            }
            MirCollectionOp::Round
            | MirCollectionOp::Floor
            | MirCollectionOp::Ceiling
            | MirCollectionOp::Truncate => {
                // A rounding expression is typed by its destination slot, so an
                // explicit signed integer slot reaches MIR as a sized signed
                // result. Unsigned widths convert before this call. The runner
                // handles sized results, so accept every signed width.
                if !matches!(
                    self.type_kind(call.return_ty),
                    Type::Primitive(Primitive::Numerus)
                        | Type::SizedNumeric(
                            Primitive::Numerus,
                            NumericWidth::I8
                                | NumericWidth::I16
                                | NumericWidth::I32
                                | NumericWidth::I64
                        )
                ) {
                    self.error(span, "numeric rounding result is not numerus");
                }
                if let Some(receiver_ty) = first_arg_ty
                    && !matches!(
                        self.type_kind(receiver_ty),
                        Type::Primitive(Primitive::Fractus)
                            | Type::SizedNumeric(Primitive::Fractus, _)
                    )
                {
                    self.error(span, "numeric rounding receiver is not fractus");
                }
            }
            MirCollectionOp::TensorRank => {
                self.require_exact(
                    call.return_ty,
                    self.primitive(Primitive::Numerus),
                    span,
                    "tensor rank result is not numerus",
                );
                if let Some(receiver_ty) = first_arg_ty {
                    self.require_tensor_type(
                        receiver_ty,
                        span,
                        "tensor rank receiver is not tensor",
                    );
                }
            }
            // This is a compiler-owned operation. Its tensor/const contract is
            // validated by radix-mir::abi::contract after lowering, rather
            // than by the source-level collection validator.
            MirCollectionOp::TensorBatchedAttention => {}
            MirCollectionOp::TensorNorm => {
                let Some(receiver_ty) = first_arg_ty else {
                    return;
                };
                self.require_tensor_type(receiver_ty, span, "tensor norm receiver is not tensor");
                if let Type::Tensor(element, _) = self.type_kind(receiver_ty)
                    && !self
                        .context
                        .types
                        .equals(call.return_ty.semantic_id(), *element)
                {
                    self.error(span, "tensor norm result is not the element type");
                }
            }
            MirCollectionOp::TensorShape => {
                let Some(receiver_ty) = first_arg_ty else {
                    return;
                };
                if !matches!(
                    self.type_kind(receiver_ty),
                    Type::Tensor(_, _) | Type::Matrix(_, _) | Type::Sparsa(_, _)
                ) {
                    self.error(
                        span,
                        "tensor shape receiver is not tensor, matrix or sparsa",
                    );
                }
                // A tensor of static rank yields the bounded `lista<numerus, magnitudo R>`;
                // one without a static rank yields the unbounded list.
                if !matches!(
                    self.type_kind(call.return_ty),
                    Type::Array(_) | Type::BoundedArray { .. } | Type::RangedArray { .. }
                ) {
                    self.error(span, "tensor collection result is not lista");
                }
            }
            MirCollectionOp::TensorFlatten => {
                let Some(receiver_ty) = first_arg_ty else {
                    return;
                };
                self.require_tensor_type(
                    receiver_ty,
                    span,
                    "tensor flatten receiver is not tensor",
                );
                if !matches!(self.type_kind(call.return_ty), Type::Array(_)) {
                    self.error(span, "tensor collection result is not lista");
                }
            }
            MirCollectionOp::TensorMaterialize => {
                let Some(receiver_ty) = first_arg_ty else {
                    return;
                };
                self.require_tensor_type(
                    receiver_ty,
                    span,
                    "tensor materialize receiver is not tensor",
                );
                self.require_tensor_type(
                    call.return_ty,
                    span,
                    "tensor materialize result is not tensor",
                );
            }
            MirCollectionOp::TensorExpand => self.validate_tensor_expand(scope, call, span),
            MirCollectionOp::TensorCopyInto => {
                self.validate_tensor_copy_into(scope, call, span);
            }
            MirCollectionOp::TensorShift => {
                self.require_arg_count(call.args.len(), 2, span, "tensor shift runtime call");
                let Some(receiver_ty) = first_arg_ty else {
                    return;
                };
                let (source_element, source_shape) =
                    if let Type::Tensor(element, shape) = self.type_kind(receiver_ty) {
                        (*element, *shape)
                    } else {
                        self.error(span, "tensor shift receiver is not tensor");
                        return;
                    };
                let (result_element, result_shape) =
                    if let Type::Tensor(element, shape) = self.type_kind(call.return_ty) {
                        (*element, *shape)
                    } else {
                        self.error(span, "tensor shift result is not tensor");
                        return;
                    };
                if matches!(self.context.types.get(source_element), Type::Option(_)) {
                    self.error(span, "tensor shift receiver already has nullable elements");
                }
                let Type::Option(optional_inner) = self.context.types.get(result_element) else {
                    self.error(span, "tensor shift result elements are not nullable");
                    return;
                };
                if !self.context.types.equals(source_element, *optional_inner) {
                    self.error(span, "tensor shift changes the receiver element type");
                }
                if !self.context.types.index_equals(source_shape, result_shape) {
                    self.error(span, "tensor shift changes the receiver shape");
                }
                if let Some(offsets) = call.args.get(1)
                    && let Some(offset_ty) = self.validate_operand(scope, offsets, span)
                    && !radix_types::is_integer_index_vector(
                        self.context.types.as_table(),
                        offset_ty.semantic_id(),
                    )
                {
                    self.error(span, "tensor shift offsets are not an integer lista");
                }
            }
            MirCollectionOp::ArrayNeg | MirCollectionOp::ArrayNot => {
                let Some(operand_ty) = first_arg_ty else {
                    return;
                };
                let Some(kind) = self.elementwise_collection_kind(operand_ty) else {
                    self.error(span, "elementwise operand is not a lista, queue, or stack");
                    return;
                };
                let Some(result_kind) = self.elementwise_collection_kind(call.return_ty) else {
                    self.error(span, "elementwise result is not a lista, queue, or stack");
                    return;
                };
                if result_kind != kind {
                    self.error(span, "elementwise result must preserve the collection kind");
                }
                let (Some(element), Some(result_element)) = (
                    self.collection_element_ty(operand_ty),
                    self.collection_element_ty(call.return_ty),
                ) else {
                    return;
                };
                self.require_assignable(
                    element,
                    result_element,
                    span,
                    "elementwise result element type mismatch",
                );
            }
            MirCollectionOp::ArrayAdd
            | MirCollectionOp::ArraySub
            | MirCollectionOp::ArrayMul
            | MirCollectionOp::ArrayDiv
            | MirCollectionOp::ArrayMod
            | MirCollectionOp::ArrayTrueDiv
            | MirCollectionOp::ArrayLt
            | MirCollectionOp::ArrayGt
            | MirCollectionOp::ArrayLtEq
            | MirCollectionOp::ArrayGtEq
            | MirCollectionOp::ArrayValueEq
            | MirCollectionOp::ArrayValueNotEq
            | MirCollectionOp::ArrayAnd
            | MirCollectionOp::ArrayOr => {
                let Some(left_ty) = first_arg_ty else {
                    return;
                };
                let Some(right) = call.args.get(1) else {
                    return;
                };
                let Some(right_ty) = self.validate_operand(scope, right, span) else {
                    return;
                };
                let left_kind = self.elementwise_collection_kind(left_ty);
                let right_kind = self.elementwise_collection_kind(right_ty);
                let (collection_kind, collection_element) = match (left_kind, right_kind) {
                    (Some(kind), _) => {
                        let Some(element) = self.collection_element_ty(left_ty) else {
                            return;
                        };
                        (kind, element)
                    }
                    (None, Some(kind)) => {
                        let Some(element) = self.collection_element_ty(right_ty) else {
                            return;
                        };
                        (kind, element)
                    }
                    (None, None) => {
                        self.error(span, "elementwise operands need a lista, queue, or stack");
                        return;
                    }
                };
                let Some(result_kind) = self.elementwise_collection_kind(call.return_ty) else {
                    self.error(span, "elementwise result is not a lista, queue, or stack");
                    return;
                };
                if result_kind != collection_kind {
                    self.error(span, "elementwise result must preserve the collection kind");
                }
                let Some(result_element) = self.collection_element_ty(call.return_ty) else {
                    return;
                };
                // The operands flow into the result element (ruling 19(g): the
                // exact result is stored under it), so a wider result element
                // is fine and a narrower one is not. `÷` converts integers to
                // a float first, so its result element is any numeric type.
                // A comparison yields a `lista<bivalens>`, so its operands never
                // flow into the result element either.
                let flows = !matches!(
                    op,
                    MirCollectionOp::ArrayTrueDiv
                        | MirCollectionOp::ArrayLt
                        | MirCollectionOp::ArrayGt
                        | MirCollectionOp::ArrayLtEq
                        | MirCollectionOp::ArrayGtEq
                        | MirCollectionOp::ArrayValueEq
                        | MirCollectionOp::ArrayValueNotEq
                );
                match (left_kind, right_kind) {
                    (Some(left_kind), Some(right_kind)) => {
                        if left_kind != right_kind {
                            self.error(
                                span,
                                "elementwise operands must preserve one collection kind",
                            );
                        }
                        let Some(right_element) = self.collection_element_ty(right_ty) else {
                            return;
                        };
                        self.require_assignable(
                            right_element,
                            collection_element,
                            span,
                            "elementwise collection element types differ",
                        );
                    }
                    (Some(_), None) if flows => self.require_assignable_or_fitting_constant(
                        right,
                        right_ty,
                        result_element,
                        span,
                        "elementwise scalar type mismatch",
                    ),
                    (None, Some(_)) if flows => {
                        let left = &call.args[0];
                        self.require_assignable_or_fitting_constant(
                            left,
                            left_ty,
                            result_element,
                            span,
                            "elementwise scalar type mismatch",
                        );
                    }
                    // `(None, None)` already returned above.
                    (Some(_), None) | (None, _) => {}
                }
                if flows {
                    self.require_assignable(
                        collection_element,
                        result_element,
                        span,
                        "elementwise result element type mismatch",
                    );
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
            | MirCollectionOp::TensorMath(_)
            | MirCollectionOp::TensorGelu
            | MirCollectionOp::TensorRelu
            | MirCollectionOp::TensorSoftmax
            | MirCollectionOp::TensorTranspose
            | MirCollectionOp::TensorCausalMaskedSoftmax
            | MirCollectionOp::TensorQuantizedMatMul
            | MirCollectionOp::TensorGroupedMatMul
            | MirCollectionOp::TensorSsmConv1d
            | MirCollectionOp::TensorSsmScan => {
                let Some(receiver_ty) = first_arg_ty else {
                    return;
                };
                self.require_tensor_type(
                    receiver_ty,
                    span,
                    "tensor intrinsic receiver is not tensor",
                );
                self.require_tensor_type(
                    call.return_ty,
                    span,
                    "tensor intrinsic result is not tensor",
                );
            }
            MirCollectionOp::TensorSliceNd => {
                self.validate_tensor_slice_nd(scope, call, span);
            }
            MirCollectionOp::TensorGet => {
                let Some(receiver_ty) = first_arg_ty else {
                    return;
                };
                // The coordinate read admits a `matrix<T, [R, C]>` receiver on
                // the same element/result contract as a tensor.
                let element = match self.type_kind(receiver_ty) {
                    Type::Tensor(element, _) | Type::Matrix(element, _) => *element,
                    _ => {
                        self.error(span, "tensor get receiver is not tensor");
                        return;
                    }
                };
                let result_ok = match self.type_kind(call.return_ty) {
                    Type::Option(inner) => self.context.types.equals(*inner, element),
                    _ => self
                        .context
                        .types
                        .equals(call.return_ty.semantic_id(), element),
                };
                if !result_ok {
                    self.error(
                        span,
                        "tensor get result must be element or optional element",
                    );
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
                    self.require_tensor_or_matrix_type(
                        receiver_ty,
                        span,
                        "tensor mutation receiver type mismatch",
                    );
                }
            }
            MirCollectionOp::TensorFill => {
                if let Some(receiver_ty) = first_arg_ty {
                    self.require_tensor_type(
                        receiver_ty,
                        span,
                        "tensor fill receiver is not tensor",
                    );
                }
                self.require_tensor_type(call.return_ty, span, "tensor fill result is not tensor");
            }
            MirCollectionOp::TensorMean
            | MirCollectionOp::TensorMax
            | MirCollectionOp::TensorMin => {
                let Some(receiver_ty) = first_arg_ty else {
                    return;
                };
                self.require_tensor_type(
                    receiver_ty,
                    span,
                    "tensor reduction receiver is not tensor",
                );
                if !self.is_numeric_type(call.return_ty) {
                    self.error(span, "tensor reduction result is not numeric");
                }
                if let Some(identity) = call
                    .args
                    .get(1)
                    .and_then(|arg| self.validate_operand(scope, arg, span))
                    && !self.is_numeric_type(identity)
                {
                    self.error(span, "tensor reduction identity is not numeric");
                }
            }
            MirCollectionOp::TensorCruxEntropia => {
                let Some(receiver_ty) = first_arg_ty else {
                    return;
                };
                self.require_tensor_type(
                    receiver_ty,
                    span,
                    "tensor crux_entropia receiver is not tensor",
                );
                if !self.is_numeric_type(call.return_ty) {
                    self.error(span, "tensor crux_entropia result is not numeric");
                }
            }
            MirCollectionOp::TensorLayerNorm => {
                self.validate_tensor_layernorm(scope, first_arg_ty, call, span);
            }
            MirCollectionOp::TensorRmsNorm => {
                self.validate_tensor_rms_norm(scope, first_arg_ty, call, span);
            }
            MirCollectionOp::TensorRope | MirCollectionOp::TensorRopeHalf => {
                self.validate_tensor_rope(scope, first_arg_ty, call, span);
            }
            MirCollectionOp::TensorGather => {
                let Some(receiver_ty) = first_arg_ty else {
                    return;
                };
                self.require_tensor_type(receiver_ty, span, "tensor gather receiver is not tensor");
                if let Some(ids_ty) = call
                    .args
                    .get(1)
                    .and_then(|arg| self.validate_operand(scope, arg, span))
                {
                    // K7c: the ids are a rank-1 `tensor<u32, [n]>`; the extent
                    // travels with the value (rule 11), a `lista` is not a
                    // device buffer (rule 12.3).
                    let is_u32_ids = matches!(
                        self.type_kind(ids_ty),
                        Type::Tensor(element, _)
                            if matches!(
                                self.context.types.get(*element),
                                Type::SizedNumeric(Primitive::Numerus, NumericWidth::U32)
                            )
                    );
                    if !is_u32_ids {
                        self.error(span, "tensor gather ids is not a tensor<u32, [n]>");
                    } else if self
                        .static_tensor_rank(ids_ty)
                        .is_some_and(|rank| rank != 1)
                    {
                        self.error(span, "tensor gather ids must be a rank-1 tensor");
                    }
                }
                self.require_tensor_type(
                    call.return_ty,
                    span,
                    "tensor gather result is not tensor",
                );
            }
            MirCollectionOp::TensorScatterAdd => {
                // Mirror of the gather contract with the two tensor operands
                // swapped: [ids, updates] → [V, D], V taken from the result.
                if let Some(ids_ty) = call
                    .args
                    .first()
                    .and_then(|arg| self.validate_operand(scope, arg, span))
                {
                    let is_u32_ids = matches!(
                        self.type_kind(ids_ty),
                        Type::Tensor(element, _)
                            if matches!(
                                self.context.types.get(*element),
                                Type::SizedNumeric(Primitive::Numerus, NumericWidth::U32)
                            )
                    );
                    if !is_u32_ids {
                        self.error(span, "tensor scatter-add ids is not a tensor<u32, [n]>");
                    } else if self
                        .static_tensor_rank(ids_ty)
                        .is_some_and(|rank| rank != 1)
                    {
                        self.error(span, "tensor scatter-add ids must be a rank-1 tensor");
                    }
                }
                if let Some(updates_ty) = call
                    .args
                    .get(1)
                    .and_then(|arg| self.validate_operand(scope, arg, span))
                {
                    let updates_element =
                        if let Type::Tensor(element, _) = self.type_kind(updates_ty) {
                            *element
                        } else {
                            self.error(span, "tensor scatter-add updates is not tensor");
                            return;
                        };
                    if self
                        .static_tensor_rank(updates_ty)
                        .is_some_and(|rank| rank != 2)
                    {
                        self.error(span, "tensor scatter-add updates must be a rank-2 tensor");
                    }
                    match self.type_kind(call.return_ty) {
                        Type::Tensor(result_element, _) => {
                            let result_element = *result_element;
                            if self
                                .static_tensor_rank(call.return_ty)
                                .is_some_and(|rank| rank != 2)
                            {
                                self.error(
                                    span,
                                    "tensor scatter-add result must be a rank-2 tensor",
                                );
                            }
                            if !self.context.types.equals(updates_element, result_element) {
                                self.error(span, "tensor scatter-add result element type mismatch");
                            }
                        }
                        _ => self.error(span, "tensor scatter-add result is not tensor"),
                    }
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
                    .is_assignable(*source_element, *target_element)
                    && self
                        .context
                        .types
                        .is_assignable(*target_element, *source_element);
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
                    .is_some_and(|ty| matches!(self.type_kind(ty), Type::Intervallum(..)))
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
                let Type::Intervallum(element, _) = self.type_kind(receiver_ty) else {
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
                if !matches!(self.type_kind(receiver_ty), Type::Intervallum(..)) {
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
                if !matches!(self.type_kind(target_ty), Type::Intervallum(..)) {
                    self.error(span, "interval clamp target is not intervallum");
                }
            }
            MirCollectionOp::IntervalMaterialize => {
                if !first_arg_ty
                    .is_some_and(|ty| matches!(self.type_kind(ty), Type::Intervallum(..)))
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
            MirCollectionOp::CordCursor => {
                if let Some(receiver_ty) = first_arg_ty
                    && !matches!(
                        self.type_kind(receiver_ty),
                        Type::Primitive(Primitive::Ascii | Primitive::Textus)
                            | Type::BoundedAscii { .. }
                            | Type::BoundedTextus { .. }
                    )
                {
                    self.error(span, "cord cursor receiver is not ascii or textus");
                }
                if !matches!(self.type_kind(call.return_ty), Type::Cursor(_)) {
                    self.error(span, "cord cursor result is not a cursor");
                }
            }
        }
    }

    /// `TensorCopyInto(dest, src)`: the destination is a tensor place (the
    /// storage that receives the values), the source is a tensor of the same
    /// element type and shape, and the call is `vacuum`.
    fn validate_tensor_copy_into(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        call: &MirRuntimeCall,
        span: Span,
    ) {
        self.require_arg_count(call.args.len(), 2, span, "tensor copy-into runtime call");
        self.require_exact(
            call.return_ty,
            self.primitive(Primitive::Vacuum),
            span,
            "tensor copy-into result is not vacuum",
        );
        let [dest, src] = call.args.as_slice() else {
            return;
        };
        if !matches!(dest, MirOperand::Place(_)) {
            self.error(span, "tensor copy-into destination is not a place");
        }
        let dest_ty = self.validate_operand(scope, dest, span);
        let src_ty = self.validate_operand(scope, src, span);
        let (Some(dest_ty), Some(src_ty)) = (dest_ty, src_ty) else {
            return;
        };
        let (dest_element, dest_shape) =
            if let Type::Tensor(element, shape) = self.type_kind(dest_ty) {
                (*element, *shape)
            } else {
                self.error(span, "tensor copy-into destination is not tensor");
                return;
            };
        let (src_element, src_shape) = if let Type::Tensor(element, shape) = self.type_kind(src_ty)
        {
            (*element, *shape)
        } else {
            self.error(span, "tensor copy-into source is not tensor");
            return;
        };
        if !self.context.types.equals(dest_element, src_element) {
            self.error(span, "tensor copy-into element types differ");
        }
        if !self.context.types.index_equals(dest_shape, src_shape) {
            self.error(span, "tensor copy-into shapes differ");
        }
    }

    /// `TensorSliceNd(receiver, starts, ends, steps, keeps, inclusives)`: the
    /// trapping multi-axis bracket-range view. The receiver is a tensor and the
    /// result is a tensor — a view of the receiver storage, never a copy. The
    /// five lists each carry one entry per receiver axis and share one length;
    /// starts/ends/steps are integer lists and keeps/inclusives are `bivalens`
    /// lists. Unbounded `lista<T>` types carry no static length, so only
    /// bounded list capacities are held to the shared-axis rule.
    fn validate_tensor_slice_nd(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        call: &MirRuntimeCall,
        span: Span,
    ) {
        let receiver_ty = call
            .args
            .first()
            .and_then(|arg| self.validate_operand(scope, arg, span));
        let receiver_rank = receiver_ty.and_then(|ty| self.static_tensor_rank(ty));
        if let Some(receiver_ty) = receiver_ty {
            self.require_tensor_type(receiver_ty, span, "tensor slice-nd receiver is not tensor");
        }
        self.require_tensor_type(call.return_ty, span, "tensor slice-nd result is not tensor");
        let mut axis_length: Option<u64> = None;
        let mut length_error = false;
        for (index, arg) in call.args.iter().enumerate().skip(1) {
            let Some(arg_ty) = self.validate_operand(scope, arg, span) else {
                continue;
            };
            let axis = match self.type_kind(arg_ty) {
                Type::Array(element) => Some((*element, None)),
                Type::BoundedArray { element, capacity } => Some((*element, Some(*capacity))),
                _ => None,
            };
            let Some((element, capacity)) = axis else {
                self.error(
                    span,
                    format!("tensor slice-nd argument {index} is not a lista"),
                );
                continue;
            };
            let element_ok = if index >= 4 {
                matches!(
                    self.context.types.get(element),
                    Type::Primitive(Primitive::Bivalens)
                )
            } else {
                self.is_numeric_integral_type(MirType::semantic(element))
            };
            if !element_ok {
                self.error(
                    span,
                    format!("tensor slice-nd argument {index} has the wrong element type"),
                );
            }
            let Some(capacity) = capacity else {
                continue;
            };
            let IndexExpr::Literal(length) = *self.context.types.get_index(capacity) else {
                continue;
            };
            match axis_length {
                None => axis_length = Some(length),
                Some(first) if first != length && !length_error => {
                    self.error(span, "tensor slice-nd lists do not share one axis length");
                    length_error = true;
                }
                Some(_) => {}
            }
        }
        if let (Some(rank), Some(length)) = (receiver_rank, axis_length)
            && usize::try_from(length).ok() != Some(rank)
        {
            self.error(span, "tensor slice-nd lists do not match receiver rank");
        }
    }

    pub(crate) fn require_tensor_type(&mut self, ty: MirType, span: Span, message: &'static str) {
        if !matches!(self.type_kind(ty), Type::Tensor(_, _)) {
            self.error(span, message);
        }
    }

    /// The coordinate read/write surface admits a `matrix` receiver on the
    /// same contract as a tensor (ruling 2026-10-15).
    pub(crate) fn require_tensor_or_matrix_type(
        &mut self,
        ty: MirType,
        span: Span,
        message: &'static str,
    ) {
        if !matches!(self.type_kind(ty), Type::Tensor(_, _) | Type::Matrix(_, _)) {
            self.error(span, message);
        }
    }

    /// Axis/epsilon literals often arrive as temps after sized-fractus lowering.
    /// Those operands stay valid; only a wrong constant kind is rejected.
    fn validate_tensor_const_arg<V, P>(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        arg: &MirOperand,
        span: Span,
        validation: &TensorConstValidation,
        extract: fn(&MirConstant) -> Option<V>,
        predicate: P,
    ) where
        P: FnOnce(&mut Self, V),
    {
        self.validate_operand(scope, arg, span);
        let MirOperand::Constant(constant) = arg else {
            return;
        };
        if let Some(value) = extract(constant) {
            predicate(self, value);
        } else {
            self.error(
                span,
                format!(
                    "tensor {} {} must be {}",
                    validation.label, validation.name, validation.message
                ),
            );
        }
    }

    pub(crate) fn validate_tensor_axis_const(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        arg: &MirOperand,
        rank: usize,
        span: Span,
        label: &'static str,
    ) {
        self.validate_tensor_const_arg(
            scope,
            arg,
            span,
            &TensorConstValidation {
                label,
                name: "axis",
                message: "an int constant or temp",
            },
            int_constant,
            |this: &mut Self, axis_val: i64| {
                if usize::try_from(axis_val).map_or(true, |axis| axis >= rank) {
                    this.error(
                        span,
                        format!("tensor {label} axis {axis_val} is out of range for rank {rank}"),
                    );
                }
            },
        );
    }

    pub(crate) fn validate_tensor_epsilon_const(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        arg: &MirOperand,
        span: Span,
        label: &'static str,
    ) {
        self.validate_tensor_const_arg(
            scope,
            arg,
            span,
            &TensorConstValidation {
                label,
                name: "epsilon",
                message: "a float constant or temp",
            },
            float_constant,
            |this: &mut Self, f: f64| {
                if f <= 0.0 || !f.is_finite() {
                    this.error(
                        span,
                        format!("tensor {label} epsilon must be positive and finite"),
                    );
                }
            },
        );
    }

    fn validate_tensor_rope_pos_const(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        arg: &MirOperand,
        span: Span,
    ) {
        self.validate_tensor_const_arg(
            scope,
            arg,
            span,
            &TensorConstValidation {
                label: "rope",
                name: "pos",
                message: "an int constant or temp",
            },
            int_constant,
            |this: &mut Self, pos_val: i64| {
                if pos_val < 0 {
                    this.error(
                        span,
                        format!("tensor rope pos {pos_val} must be non-negative"),
                    );
                }
            },
        );
    }

    // Shape dims are exact by contract: finite, integral, and i64-round-trip
    // verified before any cast, and `fract() == 0.0` is an exact integrality
    // test — the comparisons are intentional, not epsilon candidates.
    #[allow(clippy::float_cmp)]
    #[allow(clippy::cast_precision_loss)]
    #[allow(clippy::cast_possible_truncation)]
    fn validate_tensor_rope_dim_const(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        arg: &MirOperand,
        receiver_ty: MirType,
        span: Span,
    ) {
        self.validate_operand(scope, arg, span);
        let dim_val = match arg {
            MirOperand::Constant(MirConstant::Int(dim_val)) => *dim_val,
            MirOperand::Constant(MirConstant::UInt(dim_val)) => {
                let Some(dim_val) = i64::try_from(*dim_val).ok() else {
                    self.error(span, "tensor rope dim must be an int constant or temp");
                    return;
                };
                dim_val
            }
            MirOperand::Constant(MirConstant::Float(dim_val)) => {
                // Whole-number floats are emitted by literal shape lowering.
                let valid = dim_val.is_finite()
                    && dim_val.fract() == 0.0
                    && (*dim_val as i64) as f64 == *dim_val;
                if !valid {
                    self.error(span, "tensor rope dim must be an int constant or temp");
                    return;
                }
                *dim_val as i64
            }
            MirOperand::Value(_)
            | MirOperand::Temp(_)
            | MirOperand::Place(_)
            | MirOperand::Constant(MirConstant::ShapeSize { .. }) => return,
            MirOperand::Constant(_) => {
                self.error(span, "tensor rope dim must be an int constant or temp");
                return;
            }
        };
        if dim_val <= 0 {
            self.error(span, format!("tensor rope dim {dim_val} must be positive"));
        }
        if dim_val % 2 != 0 {
            self.error(span, format!("tensor rope dim {dim_val} must be even"));
        }
        // Elements past `dim` stay; only the last-axis width is a bound.
        let last_extent = match self.type_kind(receiver_ty) {
            Type::Tensor(_, shape) => match self.context.types.get_index(*shape) {
                IndexExpr::Tuple(dims) => dims
                    .last()
                    .and_then(|dim| {
                        radix_types::evaluate_index_dimension(*dim, &self.context.types)
                            .ok()
                            .flatten()
                    })
                    .and_then(|count| i64::try_from(count).ok()),
                _ => radix_types::evaluate_index_dimension(*shape, &self.context.types)
                    .ok()
                    .flatten()
                    .and_then(|count| i64::try_from(count).ok()),
            },
            _ => None,
        };
        if last_extent.is_some_and(|extent| dim_val > extent) {
            self.error(
                span,
                format!("tensor rope dim {dim_val} exceeds the last-axis extent {last_extent:?}"),
            );
        }
    }

    /// RX-3b/RX-4a: `regex.inventa`, `regex.collecta`, `regex.divide` and the
    /// `match` accessors. The receiver is a `regex` (find, `find_all`, split) or
    /// a `match` (accessors); results are `match ∪ nihil`, `lista<match>`,
    /// `lista<textus>`, `textus`, `numerus`, or `textus ∪ nihil` per op.
    fn validate_regex_match_call(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        op: MirCollectionOp,
        first_arg_ty: Option<MirType>,
        call: &MirRuntimeCall,
        span: Span,
    ) {
        let receiver = if matches!(
            op,
            MirCollectionOp::RegexFind
                | MirCollectionOp::RegexFindAll
                | MirCollectionOp::RegexSplit
        ) {
            Primitive::Regex
        } else {
            Primitive::Match
        };
        if let Some(receiver_ty) = first_arg_ty {
            self.require_exact(
                receiver_ty,
                self.primitive(receiver),
                span,
                "regex/match operation receiver type mismatch",
            );
        }
        if let MirCollectionOp::RegexFindAll | MirCollectionOp::RegexSplit = op {
            let element = if matches!(op, MirCollectionOp::RegexFindAll) {
                Primitive::Match
            } else {
                Primitive::Textus
            };
            let expected = self.primitive(element).semantic_id();
            let ok = matches!(
                self.type_kind(call.return_ty),
                Type::Array(inner) if *inner == expected
            );
            if !ok {
                self.error(
                    span,
                    "regex bulk operation result is not the expected lista",
                );
            }
        } else {
            let (result_inner, optional) = match op {
                MirCollectionOp::RegexFind => (Primitive::Match, true),
                MirCollectionOp::MatchText => (Primitive::Textus, false),
                MirCollectionOp::MatchStart | MirCollectionOp::MatchEnd => {
                    (Primitive::Numerus, false)
                }
                _ => (Primitive::Textus, true),
            };
            let expected = self.primitive(result_inner);
            if optional {
                let ok = matches!(
                    self.type_kind(call.return_ty),
                    Type::Option(inner) if self.context.types.equals(*inner, expected.semantic_id())
                );
                if !ok {
                    self.error(
                        span,
                        "regex/match operation result is not the expected option",
                    );
                }
            } else {
                self.require_exact(
                    call.return_ty,
                    expected,
                    span,
                    "regex/match operation result type mismatch",
                );
            }
        }
        let Some(arg) = call.args.get(1) else {
            return;
        };
        let Some(arg_ty) = self.validate_operand(scope, arg, span) else {
            return;
        };
        if matches!(op, MirCollectionOp::MatchGroup) {
            if !self.is_numeric_type(arg_ty) {
                self.error(span, "match group index is not numerus");
            }
        } else {
            self.require_exact(
                arg_ty,
                self.primitive(Primitive::Textus),
                span,
                "regex/match operation argument is not textus",
            );
        }
    }

    /// RX-4b: `regex.muta(text, replacement)` (`textus` result from a `regex`
    /// receiver and two `textus` arguments) and `textus.munita()` (`textus`
    /// result from a `textus` receiver, no further argument).
    fn validate_regex_text_call(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        op: MirCollectionOp,
        first_arg_ty: Option<MirType>,
        call: &MirRuntimeCall,
        span: Span,
    ) {
        let receiver = if matches!(op, MirCollectionOp::RegexReplace) {
            Primitive::Regex
        } else {
            Primitive::Textus
        };
        if let Some(receiver_ty) = first_arg_ty {
            self.require_exact(
                receiver_ty,
                self.primitive(receiver),
                span,
                "regex text operation receiver type mismatch",
            );
        }
        self.require_exact(
            call.return_ty,
            self.primitive(Primitive::Textus),
            span,
            "regex text operation result is not textus",
        );
        for arg in call.args.iter().skip(1) {
            if let Some(arg_ty) = self.validate_operand(scope, arg, span) {
                self.require_exact(
                    arg_ty,
                    self.primitive(Primitive::Textus),
                    span,
                    "regex text operation argument is not textus",
                );
            }
        }
    }

    /// RX-5: closure `muta` joins `[regex, text, lista<textus>]` into a `textus`.
    fn validate_regex_replace_each_call(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        first_arg_ty: Option<MirType>,
        call: &MirRuntimeCall,
        span: Span,
    ) {
        if let Some(receiver_ty) = first_arg_ty {
            self.require_exact(
                receiver_ty,
                self.primitive(Primitive::Regex),
                span,
                "regex replace-each receiver type mismatch",
            );
        }
        self.require_exact(
            call.return_ty,
            self.primitive(Primitive::Textus),
            span,
            "regex replace-each result is not textus",
        );
        if let Some(text) = call.args.get(1)
            && let Some(text_ty) = self.validate_operand(scope, text, span)
        {
            self.require_exact(
                text_ty,
                self.primitive(Primitive::Textus),
                span,
                "regex replace-each text is not textus",
            );
        }
        if let Some(replacements) = call.args.get(2)
            && let Some(replacements_ty) = self.validate_operand(scope, replacements, span)
        {
            let textus = self.primitive(Primitive::Textus);
            let ok = matches!(
                self.type_kind(replacements_ty),
                Type::Array(inner) if self.context.types.equals(*inner, textus.semantic_id())
            );
            if !ok {
                self.error(
                    span,
                    "regex replace-each replacements are not lista<textus>",
                );
            }
        }
    }

    pub(crate) fn validate_tensor_layernorm(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        first_arg_ty: Option<MirType>,
        call: &MirRuntimeCall,
        span: Span,
    ) {
        self.validate_tensor_norm(
            scope,
            first_arg_ty,
            call,
            span,
            &TensorNormSpec {
                label: "layernorm",
                receiver_label: "input",
                flags: TensorNormFlags {
                    rank_1_or_2: true,
                    layernorm_arity_gate: true,
                    beta: true,
                },
                gamma_message: "tensor layernorm gamma is not tensor",
                result_message: "tensor layernorm result is not tensor",
            },
        );
    }

    pub(crate) fn validate_tensor_rms_norm(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        first_arg_ty: Option<MirType>,
        call: &MirRuntimeCall,
        span: Span,
    ) {
        self.validate_tensor_norm(
            scope,
            first_arg_ty,
            call,
            span,
            &TensorNormSpec {
                label: "rms_norm",
                receiver_label: "receiver",
                flags: TensorNormFlags {
                    rank_1_or_2: false,
                    layernorm_arity_gate: false,
                    beta: false,
                },
                gamma_message: "tensor rms_norm gamma is not tensor",
                result_message: "tensor rms_norm result is not tensor",
            },
        );
    }

    fn validate_tensor_norm(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        first_arg_ty: Option<MirType>,
        call: &MirRuntimeCall,
        span: Span,
        spec: &TensorNormSpec,
    ) {
        let label = spec.label;
        let receiver_label = spec.receiver_label;
        let flags = spec.flags;
        let Some(receiver_ty) = first_arg_ty else {
            return;
        };
        if !matches!(self.type_kind(receiver_ty), Type::Tensor(_, _)) {
            self.error(
                span,
                format!("tensor {label} {receiver_label} is not tensor"),
            );
            return;
        }
        let Some(rank) = self.static_tensor_rank(receiver_ty) else {
            return;
        };
        if flags.rank_1_or_2 && rank != 1 && rank != 2 {
            self.error(span, "tensor layernorm input must have rank 1 or 2");
            return;
        }
        // Arity failure is recorded earlier and does not return for rms_norm.
        if flags.layernorm_arity_gate && call.args.len() < 3 {
            return;
        }
        if let Some(arg) = call.args.get(1) {
            self.validate_tensor_axis_const(scope, arg, rank, span, label);
        }
        if let Some(arg) = call.args.get(2) {
            self.validate_tensor_epsilon_const(scope, arg, span, label);
        }
        if call.args.len() >= 4
            && let Some(gamma_ty) = call
                .args
                .get(3)
                .and_then(|arg| self.validate_operand(scope, arg, span))
        {
            self.require_tensor_type(gamma_ty, span, spec.gamma_message);
        }
        if flags.beta
            && call.args.len() >= 5
            && let Some(beta_ty) = call
                .args
                .get(4)
                .and_then(|arg| self.validate_operand(scope, arg, span))
        {
            self.require_tensor_type(beta_ty, span, "tensor layernorm beta is not tensor");
        }
        self.require_tensor_type(call.return_ty, span, spec.result_message);
    }

    pub(crate) fn validate_tensor_rope(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        first_arg_ty: Option<MirType>,
        call: &MirRuntimeCall,
        span: Span,
    ) {
        let Some(receiver_ty) = first_arg_ty else {
            return;
        };
        self.require_tensor_type(receiver_ty, span, "tensor rope receiver is not tensor");
        if let Some(arg) = call.args.get(1) {
            self.validate_tensor_rope_pos_const(scope, arg, span);
        }
        if let Some(arg) = call.args.get(2) {
            self.validate_tensor_rope_dim_const(scope, arg, receiver_ty, span);
        }
        if let Some(table_ty) = call
            .args
            .get(3)
            .and_then(|arg| self.validate_operand(scope, arg, span))
        {
            self.require_tensor_type(table_ty, span, "tensor rope table is not tensor");
        }
        self.require_tensor_type(call.return_ty, span, "tensor rope result is not tensor");
    }

    pub(crate) fn validate_map_snapshot_call(
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
            Type::Map(key, _) | Type::Ledger(key, _) if matches!(op, MirCollectionOp::Keys) => *key,
            Type::Map(_, value) | Type::Ledger(_, value)
                if matches!(op, MirCollectionOp::Values) =>
            {
                *value
            }
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

    pub(crate) fn validate_atomic_call(
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

        let Some(receiver_ty) = call
            .args
            .first()
            .and_then(|receiver| self.validate_operand(scope, receiver, span))
        else {
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

    pub(crate) fn validate_atomic_value_arg(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        call: &MirRuntimeCall,
        index: usize,
        element_ty: MirType,
        span: Span,
        message: &'static str,
    ) {
        if let Some(arg) = call.args.get(index)
            && let Some(arg_ty) = self.validate_operand(scope, arg, span)
        {
            self.require_assignable(arg_ty, element_ty, span, message);
        }
    }

    /// Route check for a generic runtime `convert` carrying a conversio.
    ///
    /// A recovery-free `lista ↦ tensor` conversio must lower to the
    /// `tensor_from_flat` collection op, never the runtime route. A conversio
    /// with a `⊥` recovery legitimately rides the runtime convert: the
    /// lowering only routes through `tensor_from_flat` when `recovery` is
    /// `None` (it has no failure channel to carry the recovery), so reject
    /// the eligible route only for recovery-free conversions.
    pub(crate) fn validate_runtime_conversion_route(
        &mut self,
        source_ty: MirType,
        target_ty: MirType,
        recovered: bool,
        span: Span,
    ) {
        let source = source_ty.semantic_id();
        let target = target_ty.semantic_id();
        if !recovered
            && validate_lista_to_tensor_source(source, target, self.context.types.as_table())
                .is_ok()
        {
            self.error(
                span,
                "eligible lista-to-tensor conversio must lower to tensor_from_flat, not runtime convert",
            );
        }
    }

    pub(crate) fn require_arg_count(
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

    fn elementwise_collection_kind(
        &self,
        collection_ty: MirType,
    ) -> Option<ElementwiseCollectionKind> {
        match self.type_kind(collection_ty) {
            Type::Array(_) | Type::BoundedArray { .. } | Type::RangedArray { .. } => {
                Some(ElementwiseCollectionKind::Array)
            }
            Type::Queue(_) | Type::BoundedQueue { .. } => Some(ElementwiseCollectionKind::Queue),
            Type::Stack(_) | Type::BoundedStack { .. } => Some(ElementwiseCollectionKind::Stack),
            _ => None,
        }
    }

    pub(crate) fn collection_element_ty(&self, collection_ty: MirType) -> Option<MirType> {
        match self.type_kind(collection_ty) {
            Type::Array(inner)
            | Type::BoundedArray { element: inner, .. }
            | Type::RangedArray { element: inner, .. }
            | Type::Queue(inner)
            | Type::BoundedQueue { element: inner, .. }
            | Type::Stack(inner)
            | Type::BoundedStack { element: inner, .. }
            | Type::Set(inner) => Some(MirType::semantic(*inner)),
            _ => None,
        }
    }

    /// Static rank of a tensor type, when its shape resolves to a literal or
    /// a tuple of literals. Unresolved shapes return `None` (the MIR is valid
    /// regardless — shape facts are checked where they are statically known).
    pub(crate) fn static_tensor_rank(&self, ty: MirType) -> Option<usize> {
        match self.type_kind(ty) {
            Type::Tensor(_, shape) => match self.context.types.get_index(*shape) {
                IndexExpr::Literal(_)
                | IndexExpr::Param(_)
                | IndexExpr::Runtime(_)
                | IndexExpr::Affine { .. }
                | IndexExpr::Divide { .. } => Some(1),
                IndexExpr::Tuple(dims) => Some(dims.len()),
                IndexExpr::Infer(_) | IndexExpr::Unspecified => None,
            },
            _ => None,
        }
    }

    /// Whether `element` is a `wrapping` or `saturating` word and `result` is
    /// a plain integer whose range holds every value of the word's width.
    fn word_sum_widens(&self, element: MirType, result: MirType) -> bool {
        let (Type::ModularWord(word)
        | Type::SaturatingWord(word)
        | Type::RangedInt { width: word, .. }) = self.type_kind(element)
        else {
            return false;
        };
        let result_width = match self.type_kind(result) {
            Type::Primitive(Primitive::Numerus) => NumericWidth::I64,
            Type::SizedNumeric(Primitive::Numerus, width) => *width,
            _ => return false,
        };
        match (word.integer_range(), result_width.integer_range()) {
            (Some((lo, hi)), Some((result_lo, result_hi))) => result_lo <= lo && hi <= result_hi,
            _ => false,
        }
    }

    pub(crate) fn is_numeric_type(&self, ty: MirType) -> bool {
        matches!(
            self.type_kind(ty),
            Type::Primitive(Primitive::Numerus | Primitive::Fractus)
                | Type::SizedNumeric(Primitive::Numerus | Primitive::Fractus, _)
                | Type::ModularWord(_)
                | Type::SaturatingWord(_)
                | Type::RangedInt { .. }
                | Type::PolicyFloat(_, _)
        )
    }

    pub(crate) fn is_numeric_integral_type(&self, ty: MirType) -> bool {
        matches!(
            self.type_kind(ty),
            Type::Primitive(Primitive::Numerus)
                | Type::SizedNumeric(Primitive::Numerus, _)
                | Type::ModularWord(_)
                | Type::SaturatingWord(_)
                | Type::RangedInt { .. }
        )
    }

    /// Operand types of the bit operations: integers and the raw-bits
    /// markers `b8 b16 b32 b64`.
    pub(crate) fn is_bitwise_operand_type(&self, ty: MirType) -> bool {
        self.is_numeric_integral_type(ty) || matches!(self.type_kind(ty), Type::Bits(_))
    }

    pub(crate) fn is_float_type(&self, ty: MirType) -> bool {
        matches!(
            self.type_kind(ty),
            Type::Primitive(Primitive::Fractus)
                | Type::SizedNumeric(Primitive::Fractus, _)
                | Type::PolicyFloat(_, _)
        )
    }

    /// True for scalar float types and tensors whose element type is float.
    pub(crate) fn is_float_operand_type(&self, ty: MirType) -> bool {
        match self.type_kind(ty) {
            Type::Primitive(Primitive::Fractus)
            | Type::SizedNumeric(Primitive::Fractus, _)
            | Type::PolicyFloat(_, _) => true,
            Type::Tensor(inner, _) => self.is_float_type(MirType::semantic(*inner)),
            _ => false,
        }
    }

    pub(crate) fn is_arithmetic_operand_type(&self, op: MirBinOp, ty: MirType) -> bool {
        self.is_numeric_type(ty) || (matches!(op, MirBinOp::Add) && self.is_text_type(ty))
    }

    pub(crate) fn is_text_type(&self, ty: MirType) -> bool {
        matches!(self.type_kind(ty), Type::Primitive(Primitive::Textus))
    }
}
