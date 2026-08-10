//! Runtime-call, intrinsic, gradient, sermo, collection, map, and atomic checks.

use super::{FunctionScope, GenericTypeBindings, Validator};
use crate::lista_tensor_conversio::validate_lista_to_tensor_source;
use crate::{
    MirAtomicOp, MirBinOp, MirCollectionOp, MirConstant, MirConversion, MirGradientOp,
    MirIntrinsic, MirLayoutKind, MirOperand, MirPlace, MirRuntimeCall, MirSermoOp, MirType,
};
use radix_lexer::Span;
use radix_types::{IndexExpr, Primitive, Type};

impl Validator<'_, '_> {
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
                if !matches!(
                    self.type_kind(call.return_ty),
                    Type::Array(_) | Type::AsyncCursor(_, _)
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
        }
    }

    pub(crate) fn validate_gradient_call(
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
                if !self.context.types.assignable(element, *result_element) {
                    self.error(span, "sermo cursor drain element type mismatch");
                }
            }
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
            | MirCollectionOp::Values
            | MirCollectionOp::TensorCausalMaskedSoftmax => 1,
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
            | MirCollectionOp::TensorSumAxis
            | MirCollectionOp::TensorReshape
            | MirCollectionOp::TensorGet
            | MirCollectionOp::TensorFill
            | MirCollectionOp::TensorAdd
            | MirCollectionOp::TensorSub
            | MirCollectionOp::TensorMul
            | MirCollectionOp::TensorMatMul
            | MirCollectionOp::TensorCruxEntropia
            | MirCollectionOp::TensorGather
            | MirCollectionOp::TensorDiv
            | MirCollectionOp::RegexMatches => 2,
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
            | MirCollectionOp::TensorSlice
            | MirCollectionOp::TensorRope => 3,
            MirCollectionOp::SparseSet => 3,
            // TensorLayerNorm: 3-5 args (input + optional gamma/beta + axis + epsilon).
            // Range check is handled below — this arm prevents a non-exhaustive match.
            MirCollectionOp::TensorLayerNorm => 3,
            // TensorRmsNorm: 4 args (input + axis + epsilon + gamma).
            MirCollectionOp::TensorRmsNorm => 4,
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
            MirCollectionOp::TensorSumAxis => {
                let Some(receiver_ty) = first_arg_ty else {
                    return;
                };
                if !matches!(self.type_kind(receiver_ty), Type::Tensor(_, _)) {
                    self.error(span, "TensorSumAxis receiver is not a tensor");
                    return;
                }
                if !matches!(self.type_kind(call.return_ty), Type::Tensor(_, _)) {
                    self.error(span, "TensorSumAxis result is not a tensor");
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
                if let Some(arg) = call.args.get(1) {
                    if let Some(arg_ty) = self.validate_operand(scope, arg, span) {
                        self.require_exact(
                            arg_ty,
                            self.primitive(Primitive::Textus),
                            span,
                            "regex consentit argument is not textus",
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
                        | Type::SizedNumeric(Primitive::Fractus, _)
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
                    if !matches!(
                        self.type_kind(receiver_ty),
                        Type::Primitive(Primitive::Fractus)
                            | Type::SizedNumeric(Primitive::Fractus, _)
                    ) {
                        self.error(span, "numeric rounding receiver is not fractus");
                    }
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
            | MirCollectionOp::TensorTranspose
            | MirCollectionOp::TensorCausalMaskedSoftmax => {
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
            MirCollectionOp::TensorRmsNorm => {
                let Some(receiver_ty) = first_arg_ty else {
                    return;
                };
                let Some(rank) = self.static_tensor_rank(receiver_ty) else {
                    return;
                };
                // axis (args[1]) and epsilon (args[2]) const checks mirror the
                // layernorm arm (MIR layout [x, axis, eps, gamma]).
                if let Some(arg) = call.args.get(1) {
                    let _ = self.validate_operand(scope, arg, span);
                    match arg {
                        MirOperand::Constant(MirConstant::Int(axis_val)) => {
                            if usize::try_from(*axis_val).map_or(true, |axis| axis >= rank) {
                                self.error(
                                    span,
                                    format!(
                                        "tensor rms_norm axis {axis_val} is out of range for rank {rank}"
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
                                "tensor rms_norm axis must be an int constant or temp",
                            );
                        }
                    }
                }
                if let Some(arg) = call.args.get(2) {
                    let _ = self.validate_operand(scope, arg, span);
                    match arg {
                        MirOperand::Constant(MirConstant::Float(f)) => {
                            if *f <= 0.0 || !f.is_finite() {
                                self.error(
                                    span,
                                    "tensor rms_norm epsilon must be positive and finite",
                                );
                            }
                        }
                        MirOperand::Value(_) | MirOperand::Temp(_) | MirOperand::Place(_) => {
                            // Literals lowered through sized-fractus temps are valid.
                        }
                        MirOperand::Constant(_) => {
                            self.error(
                                span,
                                "tensor rms_norm epsilon must be a float constant or temp",
                            );
                        }
                    }
                }
                if let Some(gamma_ty) = call
                    .args
                    .get(3)
                    .and_then(|arg| self.validate_operand(scope, arg, span))
                {
                    if !matches!(self.type_kind(gamma_ty), Type::Tensor(_, _)) {
                        self.error(span, "tensor rms_norm gamma is not tensor");
                    }
                }
                if !matches!(self.type_kind(call.return_ty), Type::Tensor(_, _)) {
                    self.error(span, "tensor rms_norm result is not tensor");
                }
            }
            MirCollectionOp::TensorRope => {
                let Some(receiver_ty) = first_arg_ty else {
                    return;
                };
                // pos (args[1]) and dim (args[2]) const checks (MIR layout
                // [x, pos, dim]). dim must be even and within the last axis.
                if let Some(arg) = call.args.get(1) {
                    let _ = self.validate_operand(scope, arg, span);
                    match arg {
                        MirOperand::Constant(MirConstant::Int(pos_val)) => {
                            if *pos_val < 0 {
                                self.error(
                                    span,
                                    format!("tensor rope pos {pos_val} must be non-negative"),
                                );
                            }
                        }
                        MirOperand::Value(_) | MirOperand::Temp(_) | MirOperand::Place(_) => {}
                        MirOperand::Constant(_) => {
                            self.error(span, "tensor rope pos must be an int constant or temp");
                        }
                    }
                }
                if let Some(arg) = call.args.get(2) {
                    let _ = self.validate_operand(scope, arg, span);
                    match arg {
                        MirOperand::Constant(MirConstant::Int(dim_val)) => {
                            if *dim_val <= 0 {
                                self.error(
                                    span,
                                    format!("tensor rope dim {dim_val} must be positive"),
                                );
                            }
                            if *dim_val % 2 != 0 {
                                self.error(span, format!("tensor rope dim {dim_val} must be even"));
                            }
                            // The last-axis extent check (dim must not exceed
                            // the row width; elements beyond dim are preserved).
                            let last_extent = match self.type_kind(receiver_ty) {
                                Type::Tensor(_, shape) => {
                                    match self.context.types.get_index(*shape) {
                                        IndexExpr::Tuple(dims) => dims.last().and_then(|dim| {
                                            match self.context.types.get_index(*dim) {
                                                IndexExpr::Literal(count) => {
                                                    i64::try_from(*count).ok()
                                                }
                                                _ => None,
                                            }
                                        }),
                                        IndexExpr::Literal(count) => i64::try_from(*count).ok(),
                                        _ => None,
                                    }
                                }
                                _ => None,
                            };
                            if last_extent.is_some_and(|extent| *dim_val > extent) {
                                self.error(
                                    span,
                                    format!("tensor rope dim {dim_val} exceeds the last-axis extent {last_extent:?}"),
                                );
                            }
                        }
                        MirOperand::Value(_) | MirOperand::Temp(_) | MirOperand::Place(_) => {}
                        MirOperand::Constant(_) => {
                            self.error(span, "tensor rope dim must be an int constant or temp");
                        }
                    }
                }
                if !matches!(self.type_kind(call.return_ty), Type::Tensor(_, _)) {
                    self.error(span, "tensor rope result is not tensor");
                }
            }
            MirCollectionOp::TensorGather => {
                let Some(receiver_ty) = first_arg_ty else {
                    return;
                };
                if !matches!(self.type_kind(receiver_ty), Type::Tensor(_, _)) {
                    self.error(span, "tensor gather receiver is not tensor");
                }
                if let Some(ids_ty) = call
                    .args
                    .get(1)
                    .and_then(|arg| self.validate_operand(scope, arg, span))
                {
                    if !matches!(self.type_kind(ids_ty), Type::Array(_)) {
                        self.error(span, "tensor gather ids is not a lista");
                    }
                }
                if !matches!(self.type_kind(call.return_ty), Type::Tensor(_, _)) {
                    self.error(span, "tensor gather result is not tensor");
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

    pub(crate) fn validate_atomic_value_arg(
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

    pub(crate) fn validate_runtime_conversion_route(
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

    pub(crate) fn collection_element_ty(&mut self, collection_ty: MirType) -> Option<MirType> {
        match self.type_kind(collection_ty) {
            Type::Array(inner) | Type::Set(inner) => Some(MirType::semantic(*inner)),
            _ => None,
        }
    }

    /// Static rank of a tensor type, when its shape resolves to a literal or
    /// a tuple of literals. Unresolved shapes return `None` (the MIR is valid
    /// regardless — shape facts are checked where they are statically known).
    pub(crate) fn static_tensor_rank(&self, ty: MirType) -> Option<usize> {
        match self.type_kind(ty) {
            Type::Tensor(_, shape) => match self.context.types.get_index(*shape) {
                IndexExpr::Literal(_) | IndexExpr::Param(_) => Some(1),
                IndexExpr::Tuple(dims) => Some(dims.len()),
                IndexExpr::Infer(_) | IndexExpr::Unspecified => None,
            },
            _ => None,
        }
    }

    pub(crate) fn is_numeric_type(&self, ty: MirType) -> bool {
        matches!(
            self.type_kind(ty),
            Type::Primitive(Primitive::Numerus | Primitive::Fractus)
                | Type::SizedNumeric(Primitive::Numerus | Primitive::Fractus, _)
                | Type::ModularWord(_)
        )
    }

    pub(crate) fn is_numeric_integral_type(&self, ty: MirType) -> bool {
        matches!(
            self.type_kind(ty),
            Type::Primitive(Primitive::Numerus)
                | Type::SizedNumeric(Primitive::Numerus, _)
                | Type::ModularWord(_)
        )
    }

    pub(crate) fn is_float_type(&self, ty: MirType) -> bool {
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
    pub(crate) fn is_float_operand_type(&self, ty: MirType) -> bool {
        match self.type_kind(ty) {
            Type::Primitive(Primitive::Fractus) | Type::SizedNumeric(Primitive::Fractus, _) => true,
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
