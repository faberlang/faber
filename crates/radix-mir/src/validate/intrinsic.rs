//! Runtime-call, intrinsic, gradient, sermo, collection, map, and atomic checks.

use super::{FunctionScope, GenericTypeBindings, Validator};
use crate::lista_tensor_conversio::validate_lista_to_tensor_source;
use crate::{
    MirAtomicOp, MirBinOp, MirCollectionOp, MirConstant, MirConversion, MirGradientOp,
    MirIntrinsic, MirLayoutKind, MirOperand, MirPlace, MirRuntimeCall, MirSermoOp, MirType,
};
use radix_lexer::Span;
use radix_types::{IndexExpr, Primitive, Type};

#[derive(Clone, Copy, PartialEq, Eq)]
enum ElementwiseCollectionKind {
    Array,
    Queue,
    Stack,
}

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
                if let Some(recovery) = &conversion.recovery
                    && let Some(recovery_ty) = self.validate_operand(scope, recovery, span)
                {
                    self.require_assignable(
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
            // SFR-5: the filum sum declaration validates its surface shape —
            // one source operand, a fractus broadcast result (the L7 f32
            // device surface). The declaration facts (term body, coordinate
            // binders, floor coherence) are the kernel-plan pass's typed
            // checks, never re-derived here.
            MirIntrinsic::FilumSum(op) => {
                self.require_arg_count(call.args.len(), 1, span, "filum sum runtime call");
                if let Some(arg) = call.args.first() {
                    self.validate_operand(scope, arg, span);
                }
                if !matches!(
                    self.type_kind(call.return_ty),
                    Type::Primitive(Primitive::Fractus) | Type::SizedNumeric(Primitive::Fractus, _)
                ) {
                    self.error(span, "filum sum runtime call does not return fractus");
                }
                if op.lane_width == 0 || !op.lane_width.is_power_of_two() {
                    self.error(span, "filum lane width must be a positive power of two");
                }
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
                if !self.context.types.is_assignable(element, *result_element) {
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
            | MirCollectionOp::ArgMax
            | MirCollectionOp::ArgMin
            | MirCollectionOp::TensorShape
            | MirCollectionOp::TensorFlatten
            | MirCollectionOp::TensorMaterialize
            | MirCollectionOp::TensorMean
            | MirCollectionOp::TensorMax
            | MirCollectionOp::TensorMin
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
            | MirCollectionOp::CordCursor
            | MirCollectionOp::TensorCausalMaskedSoftmax => 1,
            MirCollectionOp::ArrayAdd
            | MirCollectionOp::ArraySub
            | MirCollectionOp::ArrayMul
            | MirCollectionOp::ArrayDiv
            | MirCollectionOp::Append
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
            | MirCollectionOp::TensorSsmScan
            | MirCollectionOp::TensorDiv
            | MirCollectionOp::RegexMatches
            | MirCollectionOp::SparseGet
            | MirCollectionOp::IntervalIntersect
            | MirCollectionOp::IntervalUnion
            | MirCollectionOp::IntervalContains
            | MirCollectionOp::IntervalClamp => 2,
            MirCollectionOp::Approximate => 0,
            MirCollectionOp::Put
            | MirCollectionOp::Slice
            | MirCollectionOp::Replace
            | MirCollectionOp::TensorSet
            | MirCollectionOp::TensorCreate
            | MirCollectionOp::TensorSlice
            | MirCollectionOp::TensorQuantizedMatMul
            | MirCollectionOp::TensorGroupedMatMul
            | MirCollectionOp::TensorSsmConv1d
            | MirCollectionOp::SparseSet => 3,
            // Route C (AEU-5): the padded bounded-lista snapshot appends a
            // trailing fill operand after [seed, flat, shape]; the flat path
            // keeps its exact-count 3-argument contract.
            MirCollectionOp::TensorFromFlat => 3,
            MirCollectionOp::TensorRmsNorm => 4,
            MirCollectionOp::TensorRope | MirCollectionOp::TensorRopeHalf => 4,
            MirCollectionOp::TensorLayerNorm => 0,
            MirCollectionOp::TensorGather => 0,
            // Compiler-owned M3 lowering carries tensor peers followed by a
            // variable const contract: projections may carry fused rope and
            // bias peers, while scores/context/softmax carry their normal
            // tensor inputs.
            MirCollectionOp::TensorBatchedAttention => 0,
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
        } else if matches!(op, MirCollectionOp::TensorMax | MirCollectionOp::TensorMin) {
            // A caller-stated coalesce identity is an optional second scalar
            // operand; the no-identity static path carries only the tensor.
            if !(1..=2).contains(&call.args.len()) {
                self.error(
                    span,
                    format!("collection {op:?} expects 1 or 2 MIR arguments"),
                );
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
                self.require_assignable(
                    element_ty,
                    call.return_ty,
                    span,
                    "collection sum result type mismatch",
                );
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
                    Type::Array(inner) | Type::BoundedArray { element: inner, .. } => *inner,
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
                let arg_ty = match self.type_kind(receiver_ty) {
                    Type::Primitive(Primitive::Numerus | Primitive::Fractus)
                    | Type::SizedNumeric(Primitive::Numerus | Primitive::Fractus, _) => receiver_ty,
                    Type::Array(element)
                    | Type::BoundedArray { element, .. }
                    | Type::Tensor(element, _) => MirType::semantic(*element),
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
                self.require_exact(
                    call.return_ty,
                    self.primitive(Primitive::Numerus),
                    span,
                    "numeric rounding result is not numerus",
                );
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
            MirCollectionOp::ArrayAdd
            | MirCollectionOp::ArraySub
            | MirCollectionOp::ArrayMul
            | MirCollectionOp::ArrayDiv => {
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
                    (Some(_), None) => self.require_assignable(
                        right_ty,
                        collection_element,
                        span,
                        "elementwise scalar type mismatch",
                    ),
                    (None, Some(_)) => self.require_assignable(
                        left_ty,
                        collection_element,
                        span,
                        "elementwise scalar type mismatch",
                    ),
                    (None, None) => unreachable!("elementwise collection kind selected above"),
                }
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
                self.require_assignable(
                    result_element,
                    collection_element,
                    span,
                    "elementwise result element type mismatch",
                );
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
            MirCollectionOp::TensorGet => {
                let Some(receiver_ty) = first_arg_ty else {
                    return;
                };
                let receiver_kind = self.type_kind(receiver_ty).clone();
                let Type::Tensor(element, _) = receiver_kind else {
                    self.error(span, "tensor get receiver is not tensor");
                    return;
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
                    self.require_tensor_type(
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
                    // CCI-1: bounded `lista<numerus,N>` literals are legal ids
                    // post-7562d0cf (exact list literal bounds).
                    if !matches!(
                        self.type_kind(ids_ty),
                        Type::Array(_) | Type::BoundedArray { .. }
                    ) {
                        self.error(span, "tensor gather ids is not a lista");
                    }
                }
                self.require_tensor_type(
                    call.return_ty,
                    span,
                    "tensor gather result is not tensor",
                );
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
            // This is a compiler-owned operation. Its tensor/const contract is
            // validated by radix-mir::abi::contract after lowering, rather
            // than by the source-level collection validator.
            MirCollectionOp::TensorBatchedAttention => {}
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
            MirCollectionOp::CordCursor => {
                if let Some(receiver_ty) = first_arg_ty
                    && !matches!(
                        self.type_kind(receiver_ty),
                        Type::Primitive(Primitive::Ascii | Primitive::Textus)
                            | Type::BoundedAscii { .. }
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

    pub(crate) fn require_tensor_type(&mut self, ty: MirType, span: Span, message: &'static str) {
        if !matches!(self.type_kind(ty), Type::Tensor(_, _)) {
            self.error(span, message);
        }
    }

    /// Axis/epsilon literals often arrive as temps after sized-fractus lowering.
    /// Those operands stay valid; only a wrong constant kind is rejected.
    pub(crate) fn validate_tensor_axis_const(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        arg: &MirOperand,
        rank: usize,
        span: Span,
        label: &'static str,
    ) {
        let _ = self.validate_operand(scope, arg, span);
        match arg {
            MirOperand::Constant(MirConstant::Int(axis_val)) => {
                if usize::try_from(*axis_val).map_or(true, |axis| axis >= rank) {
                    self.error(
                        span,
                        format!("tensor {label} axis {axis_val} is out of range for rank {rank}"),
                    );
                }
            }
            MirOperand::Value(_) | MirOperand::Temp(_) | MirOperand::Place(_) => {}
            MirOperand::Constant(_) => {
                self.error(
                    span,
                    format!("tensor {label} axis must be an int constant or temp"),
                );
            }
        }
    }

    pub(crate) fn validate_tensor_epsilon_const(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        arg: &MirOperand,
        span: Span,
        label: &'static str,
    ) {
        let _ = self.validate_operand(scope, arg, span);
        match arg {
            MirOperand::Constant(MirConstant::Float(f)) => {
                if *f <= 0.0 || !f.is_finite() {
                    self.error(
                        span,
                        format!("tensor {label} epsilon must be positive and finite"),
                    );
                }
            }
            MirOperand::Value(_) | MirOperand::Temp(_) | MirOperand::Place(_) => {}
            MirOperand::Constant(_) => {
                self.error(
                    span,
                    format!("tensor {label} epsilon must be a float constant or temp"),
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
        let Some(input_ty) = first_arg_ty else {
            return;
        };
        if !matches!(self.type_kind(input_ty), Type::Tensor(_, _)) {
            self.error(span, "tensor layernorm input is not tensor");
            return;
        }
        // Unresolved shapes skip rank-dependent checks; the call remains typed.
        let Some(rank) = self.static_tensor_rank(input_ty) else {
            return;
        };
        if rank != 1 && rank != 2 {
            self.error(span, "tensor layernorm input must have rank 1 or 2");
            return;
        }
        // Arity failure is recorded earlier and does not return.
        if call.args.len() < 3 {
            return;
        }
        if let Some(arg) = call.args.get(1) {
            self.validate_tensor_axis_const(scope, arg, rank, span, "layernorm");
        }
        if let Some(arg) = call.args.get(2) {
            self.validate_tensor_epsilon_const(scope, arg, span, "layernorm");
        }
        if call.args.len() >= 4
            && let Some(gamma_ty) = call
                .args
                .get(3)
                .and_then(|arg| self.validate_operand(scope, arg, span))
        {
            self.require_tensor_type(gamma_ty, span, "tensor layernorm gamma is not tensor");
        }
        if call.args.len() >= 5
            && let Some(beta_ty) = call
                .args
                .get(4)
                .and_then(|arg| self.validate_operand(scope, arg, span))
        {
            self.require_tensor_type(beta_ty, span, "tensor layernorm beta is not tensor");
        }
        self.require_tensor_type(
            call.return_ty,
            span,
            "tensor layernorm result is not tensor",
        );
    }

    pub(crate) fn validate_tensor_rms_norm(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        first_arg_ty: Option<MirType>,
        call: &MirRuntimeCall,
        span: Span,
    ) {
        let Some(receiver_ty) = first_arg_ty else {
            return;
        };
        if !matches!(self.type_kind(receiver_ty), Type::Tensor(_, _)) {
            self.error(span, "tensor rms_norm receiver is not tensor");
            return;
        }
        let Some(rank) = self.static_tensor_rank(receiver_ty) else {
            return;
        };
        if let Some(arg) = call.args.get(1) {
            self.validate_tensor_axis_const(scope, arg, rank, span, "rms_norm");
        }
        if let Some(arg) = call.args.get(2) {
            self.validate_tensor_epsilon_const(scope, arg, span, "rms_norm");
        }
        if let Some(gamma_ty) = call
            .args
            .get(3)
            .and_then(|arg| self.validate_operand(scope, arg, span))
        {
            self.require_tensor_type(gamma_ty, span, "tensor rms_norm gamma is not tensor");
        }
        self.require_tensor_type(call.return_ty, span, "tensor rms_norm result is not tensor");
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
                        self.error(span, format!("tensor rope dim {dim_val} must be positive"));
                    }
                    if *dim_val % 2 != 0 {
                        self.error(span, format!("tensor rope dim {dim_val} must be even"));
                    }
                    // Elements past `dim` stay; only the last-axis width is a bound.
                    let last_extent = match self.type_kind(receiver_ty) {
                        Type::Tensor(_, shape) => match self.context.types.get_index(*shape) {
                            IndexExpr::Tuple(dims) => dims.last().and_then(|dim| {
                                match self.context.types.get_index(*dim) {
                                    IndexExpr::Literal(count) => i64::try_from(*count).ok(),
                                    _ => None,
                                }
                            }),
                            IndexExpr::Literal(count) => i64::try_from(*count).ok(),
                            _ => None,
                        },
                        _ => None,
                    };
                    if last_extent.is_some_and(|extent| *dim_val > extent) {
                        self.error(
                            span,
                            format!(
                                "tensor rope dim {dim_val} exceeds the last-axis extent {last_extent:?}"
                            ),
                        );
                    }
                }
                // Symbolic size-param dims ride in as ShapeSize on generic
                // templates; the instantiate seam folds them to Int before
                // link, so numeric checks wait for the concrete value.
                MirOperand::Value(_)
                | MirOperand::Temp(_)
                | MirOperand::Place(_)
                | MirOperand::Constant(MirConstant::ShapeSize { .. }) => {}
                MirOperand::Constant(_) => {
                    self.error(span, "tensor rope dim must be an int constant or temp");
                }
            }
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
        if let Some(arg) = call.args.get(index)
            && let Some(arg_ty) = self.validate_operand(scope, arg, span)
        {
            self.require_assignable(arg_ty, element_ty, span, message);
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

    fn elementwise_collection_kind(
        &self,
        collection_ty: MirType,
    ) -> Option<ElementwiseCollectionKind> {
        match self.type_kind(collection_ty) {
            Type::Array(_) | Type::BoundedArray { .. } => Some(ElementwiseCollectionKind::Array),
            Type::Queue(_) | Type::BoundedQueue { .. } => Some(ElementwiseCollectionKind::Queue),
            Type::Stack(_) | Type::BoundedStack { .. } => Some(ElementwiseCollectionKind::Stack),
            _ => None,
        }
    }

    pub(crate) fn collection_element_ty(&self, collection_ty: MirType) -> Option<MirType> {
        match self.type_kind(collection_ty) {
            Type::Array(inner)
            | Type::BoundedArray { element: inner, .. }
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

    /// True for scalar float types and tensors whose element type is float.
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
