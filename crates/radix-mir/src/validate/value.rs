//! Value, option-operation, operand, place, callee, and call-argument checks.

use super::{
    FunctionScope, GenericTypeBindings, MirFunctionSignature, Validator, signature_from_func_sig,
    signature_from_function,
};
use crate::generic::MirShapeBindings;
use crate::{
    MirBinOp, MirCallee, MirClosureCallee, MirConstant, MirIntrinsic, MirOperand,
    MirOptionChainLink, MirOptionOp, MirPlace, MirPlaceBase, MirProjection, MirRuntimeCall,
    MirShapeExpr, MirType, MirUnOp, MirValue, MirValueKind,
};
use radix_hir::DefId;
use radix_lexer::Span;
use radix_types::{
    IndexExpr, IndexId, Primitive, RuntimeExtentId, SemanticMutability, Type, TypeId, TypeTable,
};

impl Validator<'_, '_> {
    pub(crate) fn validate_value(&mut self, scope: &FunctionScope<'_, '_>, value: &MirValue) {
        self.validate_mir_type(value.ty, value.span);
        match &value.kind {
            MirValueKind::Operand(operand) => {
                self.validate_operand(scope, operand, value.span);
            }
            MirValueKind::Closure(closure) => {
                self.validate_closure_value(scope, closure, value.ty, value.span);
            }
            MirValueKind::Unary { op, operand } => {
                let operand_ty = self.validate_operand(scope, operand, value.span);
                match op {
                    MirUnOp::Not => {
                        if let Some(operand_ty) = operand_ty {
                            self.require_exact(
                                operand_ty,
                                self.primitive(Primitive::Bivalens),
                                value.span,
                                "not operand is not bivalens",
                            );
                        }
                        self.require_exact(
                            value.ty,
                            self.primitive(Primitive::Bivalens),
                            value.span,
                            "not result is not bivalens",
                        );
                    }
                    MirUnOp::IsNil | MirUnOp::IsNotNil => {
                        self.require_exact(
                            value.ty,
                            self.primitive(Primitive::Bivalens),
                            value.span,
                            "nil-test result is not bivalens",
                        );
                    }
                    MirUnOp::Neg => {
                        if let Some(operand_ty) = operand_ty
                            && !self.is_numeric_type(operand_ty)
                        {
                            self.error(value.span, "neg operand is not numeric");
                        }
                        if !self.is_numeric_type(value.ty) {
                            self.error(value.span, "neg result is not numeric");
                        }
                    }
                    MirUnOp::BitNot => {
                        if let Some(operand_ty) = operand_ty
                            && !self.is_bitwise_operand_type(operand_ty)
                        {
                            self.error(
                                value.span,
                                "bitwise not operand is not numeric integral or bits",
                            );
                        }
                        if !self.is_bitwise_operand_type(value.ty) {
                            self.error(
                                value.span,
                                "bitwise not result is not numeric integral or bits",
                            );
                        }
                    }
                    MirUnOp::Sqrt
                    | MirUnOp::Relu
                    | MirUnOp::Gelu
                    | MirUnOp::Tanh
                    | MirUnOp::Exp
                    | MirUnOp::Log
                    | MirUnOp::F16Round => {
                        // R-W1: accept tensor operands whose ELEMENT type is float
                        // (AIR lowers AirTensorOp::Gelu/Tanh/Exp/Log/Relu/Sqrt to
                        // MirUnOp on tensor<f32,…>), while still rejecting genuinely
                        // non-float operands — scalar or tensor.
                        if let Some(operand_ty) = operand_ty
                            && !self.is_float_operand_type(operand_ty)
                        {
                            self.error(value.span, "float unary operand is not fractus");
                        }
                        if !self.is_float_operand_type(value.ty) {
                            self.error(value.span, "float unary result is not fractus");
                        }
                    }
                }
            }
            MirValueKind::Binary { op, lhs, rhs } => {
                let lhs_ty = self.validate_operand(scope, lhs, value.span);
                let rhs_ty = self.validate_operand(scope, rhs, value.span);
                match op {
                    MirBinOp::Add | MirBinOp::Sub | MirBinOp::Mul | MirBinOp::Div => {
                        if let Some(ty) = lhs_ty
                            && !self.is_arithmetic_operand_type(*op, ty)
                        {
                            self.error(value.span, "binary arithmetic lhs is not numeric");
                        }
                        if let Some(ty) = rhs_ty
                            && !self.is_arithmetic_operand_type(*op, ty)
                        {
                            self.error(value.span, "binary arithmetic rhs is not numeric");
                        }
                    }
                    MirBinOp::Mod => {
                        if let Some(ty) = lhs_ty
                            && !self.is_numeric_integral_type(ty)
                            && !self.is_float_type(ty)
                        {
                            self.error(value.span, "mod lhs is not numeric integral");
                        }
                        if let Some(ty) = rhs_ty
                            && !self.is_numeric_integral_type(ty)
                            && !self.is_float_type(ty)
                        {
                            self.error(value.span, "mod rhs is not numeric integral");
                        }
                    }
                    MirBinOp::Eq
                    | MirBinOp::NotEq
                    | MirBinOp::Lt
                    | MirBinOp::Gt
                    | MirBinOp::LtEq
                    | MirBinOp::GtEq => {
                        self.require_exact(
                            value.ty,
                            self.primitive(Primitive::Bivalens),
                            value.span,
                            "comparison result is not bivalens",
                        );
                    }
                    MirBinOp::And | MirBinOp::Or => {
                        if let Some(ty) = lhs_ty {
                            self.require_exact(
                                ty,
                                self.primitive(Primitive::Bivalens),
                                value.span,
                                "logical and/or lhs is not bivalens",
                            );
                        }
                        if let Some(ty) = rhs_ty {
                            self.require_exact(
                                ty,
                                self.primitive(Primitive::Bivalens),
                                value.span,
                                "logical and/or rhs is not bivalens",
                            );
                        }
                        self.require_exact(
                            value.ty,
                            self.primitive(Primitive::Bivalens),
                            value.span,
                            "logical and/or result is not bivalens",
                        );
                    }
                    MirBinOp::Coalesce => {
                        // Coalesce: operands are individually validated; result type
                        // is checked by the caller via require_assignable.
                    }
                    MirBinOp::BitAnd
                    | MirBinOp::BitOr
                    | MirBinOp::BitXor
                    | MirBinOp::Shl
                    | MirBinOp::Shr => {
                        if let Some(ty) = lhs_ty
                            && !self.is_bitwise_operand_type(ty)
                        {
                            self.error(value.span, "bitwise lhs is not numeric integral or bits");
                        }
                        if let Some(ty) = rhs_ty
                            && !self.is_bitwise_operand_type(ty)
                        {
                            self.error(value.span, "bitwise rhs is not numeric integral or bits");
                        }
                    }
                }
            }
            MirValueKind::Option(op) => self.validate_option_op(scope, op, value.ty, value.span),
            MirValueKind::TensorExtent { tensor, axis } => {
                self.validate_tensor_extent(scope, tensor, *axis, value.ty, value.span);
            }
        }
    }

    /// `TensorExtent`: a tensor-typed operand, an axis inside its rank, and a
    /// `numerus` result. Device-legal by construction (a scalar read).
    fn validate_tensor_extent(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        tensor: &MirOperand,
        axis: u32,
        result_ty: MirType,
        span: Span,
    ) {
        let tensor_ty = self.validate_operand(scope, tensor, span);
        // `numerus` family: a kernel unit types the extent at a device-legal
        // width (i32/u32), the host at bare `numerus`.
        if !matches!(
            self.type_kind(result_ty),
            Type::Primitive(Primitive::Numerus) | Type::SizedNumeric(Primitive::Numerus, _)
        ) {
            self.error(span, "tensor extent result is not numerus");
        }
        let Some(tensor_ty) = tensor_ty else { return };
        if !matches!(self.type_kind(tensor_ty), Type::Tensor(_, _)) {
            self.error(span, "tensor extent operand is not a tensor");
            return;
        }
        if let Some(rank) = self.static_tensor_rank(tensor_ty)
            && axis as usize >= rank
        {
            self.error(
                span,
                format!("tensor extent axis {axis} is out of range for rank {rank}"),
            );
        }
    }

    pub(crate) fn validate_option_op(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        op: &MirOptionOp,
        result_ty: MirType,
        span: Span,
    ) {
        match op {
            MirOptionOp::None => {
                if !matches!(self.type_kind(result_ty), Type::Option(_)) {
                    self.error(span, "option none result is not nullable");
                }
            }
            MirOptionOp::Some(value) => {
                let value_ty = self.validate_operand(scope, value, span);
                match self.type_kind(result_ty) {
                    Type::Option(inner) => {
                        if let Some(value_ty) = value_ty {
                            self.require_assignable(
                                value_ty,
                                MirType::semantic(*inner),
                                span,
                                "option some payload type mismatch",
                            );
                        }
                    }
                    _ => self.error(span, "option some result is not nullable"),
                }
            }
            MirOptionOp::IsNil(value) | MirOptionOp::IsNotNil(value) => {
                self.validate_operand(scope, value, span);
                self.require_exact(
                    result_ty,
                    self.primitive(Primitive::Bivalens),
                    span,
                    "option test result is not bivalens",
                );
            }
            MirOptionOp::Unwrap { value, .. } => {
                if let Some(value_ty) = self.validate_operand(scope, value, span) {
                    if let Some(inner_ty) = self.nullable_inner_ty(value_ty) {
                        self.require_assignable(
                            inner_ty,
                            result_ty,
                            span,
                            "option unwrap result type mismatch",
                        );
                    } else {
                        self.error(span, "option unwrap operand is not nullable");
                    }
                }
            }
            MirOptionOp::Coalesce { value, fallback } => {
                let value_ty = self.validate_operand(scope, value, span);
                let fallback_ty = self.validate_operand(scope, fallback, span);
                if let Some(value_ty) = value_ty {
                    match self.type_kind(value_ty) {
                        Type::Option(inner) => {
                            self.require_assignable(
                                MirType::semantic(*inner),
                                result_ty,
                                span,
                                "option coalesce value type mismatch",
                            );
                        }
                        Type::Primitive(Primitive::Nihil) => {}
                        _ => {
                            self.require_assignable(
                                value_ty,
                                result_ty,
                                span,
                                "option coalesce value type mismatch",
                            );
                        }
                    }
                }
                if let Some(fallback_ty) = fallback_ty {
                    self.require_assignable(
                        fallback_ty,
                        result_ty,
                        span,
                        "option coalesce fallback type mismatch",
                    );
                }
            }
            MirOptionOp::Chain { base, link } => {
                self.validate_operand(scope, base, span);
                self.validate_option_chain_link(scope, link, span);
                if !matches!(self.type_kind(result_ty), Type::Option(_)) {
                    self.error(span, "optional chain result is not nullable");
                }
            }
        }
    }

    pub(crate) fn validate_option_chain_link(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        link: &MirOptionChainLink,
        span: Span,
    ) {
        match link {
            MirOptionChainLink::Field(_) | MirOptionChainLink::VariantField { .. } => {}
            MirOptionChainLink::Index(index) => {
                self.validate_operand(scope, index, span);
            }
            MirOptionChainLink::Call { callee, args } => {
                let sig = self.validate_callee(scope, callee, span, false);
                let mut generic_bindings = GenericTypeBindings::default();
                let mut shape_bindings = MirShapeBindings::default();
                self.validate_call_args(
                    scope,
                    sig.as_ref(),
                    args,
                    None,
                    span,
                    &mut generic_bindings,
                    &mut shape_bindings,
                );
            }
        }
    }
}
impl Validator<'_, '_> {
    pub(crate) fn validate_callee(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        callee: &MirCallee,
        span: Span,
        cursor_settle: bool,
    ) -> Option<MirFunctionSignature> {
        match callee {
            MirCallee::Function(id) => {
                let Some(function) = scope.functions_by_id.get(id).copied() else {
                    self.error(span, format!("callee function f{} does not exist", id.0));
                    return None;
                };
                Some(signature_from_function(function))
            }
            MirCallee::Definition { source: def_id, .. } => self
                .context
                .functions
                .get(def_id)
                .cloned()
                .map(|signature| self.strip_param_ref_wrappers(signature, true))
                .or_else(|| Self::signature_from_program_def(scope, *def_id)),
            MirCallee::Closure(closure) => self.validate_closure_callee(scope, closure, span),
            MirCallee::Value(value) => {
                let operand_ty = self.validate_operand(scope, value, span);
                let sig = operand_ty
                    .and_then(|ty| self.signature_from_semantic_callable_type(ty.semantic_id()));
                let cursor_settle =
                    cursor_settle && operand_ty.is_some_and(|ty| self.is_array_semantic_type(ty));
                if operand_ty.is_some() && sig.is_none() && !cursor_settle {
                    self.error(span, "value callee is not callable");
                }
                sig
            }
        }
    }

    /// The lowering shape for a stepped-cursor settle: `mark_stepped_cursor_stream`
    /// retypes the cursor-stream temp to its array type, so the `TryCall`
    /// invokes a `lista<T>` value with no arguments. The cursor runtime carries
    /// the failable contract; there is no callee signature to check. A plain
    /// `Call` on the same value is still not callable.
    pub(crate) fn is_cursor_settle_callee(
        &self,
        scope: &FunctionScope<'_, '_>,
        callee: &MirCallee,
        args: &[MirOperand],
    ) -> bool {
        if !args.is_empty() {
            return false;
        }
        let MirCallee::Value(operand) = callee else {
            return false;
        };
        direct_operand_type(scope, operand).is_some_and(|ty| self.is_array_semantic_type(ty))
    }

    fn is_array_semantic_type(&self, ty: MirType) -> bool {
        matches!(self.type_kind(ty), Type::Array(_))
    }

    pub(crate) fn validate_closure_callee(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        closure: &MirClosureCallee,
        span: Span,
    ) -> Option<MirFunctionSignature> {
        let Some(environment) = self
            .context
            .closure_environments
            .get(&closure.environment_id)
            .cloned()
        else {
            self.error(span, "closure callee is missing environment metadata");
            self.validate_operand(scope, &closure.environment, span);
            return None;
        };
        if environment.function != closure.function {
            self.error(
                span,
                "closure callee function does not match environment metadata",
            );
        }
        self.validate_operand(scope, &closure.environment, span);
        let Some(function) = scope.functions_by_id.get(&closure.function).copied() else {
            self.error(
                span,
                format!(
                    "closure callee function f{} does not exist",
                    closure.function.0
                ),
            );
            return None;
        };
        Some(signature_from_function(function))
    }

    pub(crate) fn signature_from_program_def(
        scope: &FunctionScope<'_, '_>,
        def_id: DefId,
    ) -> Option<MirFunctionSignature> {
        scope
            .functions_by_id
            .values()
            .find(|function| function.source == Some(def_id))
            .map(|function| signature_from_function(function))
    }

    /// A semantic signature spells access intent as a `Ref` wrapper on the
    /// parameter type (a function type, or a function lowered in another
    /// module whose signature the context carries); MIR carries intent in
    /// `MirParamMode` and the argument is the payload place, exactly as a
    /// direct call to a `mut` parameter passes it. A definition callee strips
    /// only the `Mutable` wrapper (`mutable_only`): a parameter whose type is
    /// genuinely a reference with another access (`Immutable`, `Copy`) takes a
    /// `Ref` argument of the same inner type unchanged.
    fn strip_param_ref_wrappers(
        &self,
        mut signature: MirFunctionSignature,
        mutable_only: bool,
    ) -> MirFunctionSignature {
        for param in &mut signature.params {
            if let Type::Ref(mutability, inner) = self.context.types.get(param.ty.semantic_id())
                && (!mutable_only || *mutability == SemanticMutability::Mutable)
            {
                param.ty = MirType::semantic(*inner);
            }
        }
        signature
    }

    pub(crate) fn signature_from_semantic_callable_type(
        &self,
        ty: TypeId,
    ) -> Option<MirFunctionSignature> {
        match self.context.types.get(ty) {
            Type::Func(sig) => {
                Some(self.strip_param_ref_wrappers(signature_from_func_sig(sig), false))
            }
            Type::Alias(_, inner) => self.signature_from_semantic_callable_type(*inner),
            _ => None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn validate_call_args(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        sig: Option<&MirFunctionSignature>,
        args: &[MirOperand],
        shape_check: Option<&MirRuntimeCall>,
        span: Span,
        generic_bindings: &mut GenericTypeBindings,
        shape_bindings: &mut MirShapeBindings,
    ) {
        let Some(sig) = sig else {
            for arg in args {
                self.validate_operand(scope, arg, span);
            }
            return;
        };

        let required = sig.params.iter().filter(|param| !param.optional).count();
        if args.len() < required || args.len() > sig.params.len() {
            self.error(
                span,
                format!(
                    "call argument count mismatch: expected {}..={}, got {}",
                    required,
                    sig.params.len(),
                    args.len()
                ),
            );
        }

        for (index, arg) in args.iter().enumerate() {
            let arg_ty = self.validate_operand(scope, arg, span);
            let Some(expected) = sig.params.get(index) else {
                continue;
            };
            if let Some(arg_ty) = arg_ty {
                if expected.optional && matches!(arg, MirOperand::Constant(MirConstant::Nil)) {
                    continue;
                }
                if self.call_argument_matches_shape_check(
                    scope,
                    expected.ty,
                    arg_ty,
                    u32::try_from(index).unwrap_or(u32::MAX),
                    shape_check,
                ) {
                    continue;
                }
                if self.call_argument_matches_own_axes(
                    expected.ty,
                    arg_ty,
                    u32::try_from(index).unwrap_or(u32::MAX),
                ) {
                    continue;
                }
                if index == 0 && self.receiver_is_instance_of_self(expected.ty, arg_ty) {
                    continue;
                }
                self.bind_generic_type_witness(
                    expected.ty,
                    arg_ty,
                    generic_bindings,
                    shape_bindings,
                    span,
                    call_argument_mismatch(
                        self.context.types.as_table(),
                        index,
                        expected.ty,
                        arg_ty,
                    ),
                );
                if let Some(expected_ty) = self.instantiate_generic_type(
                    expected.ty,
                    generic_bindings,
                    shape_bindings,
                    span,
                ) {
                    self.require_assignable(
                        arg_ty,
                        expected_ty,
                        span,
                        call_argument_mismatch(
                            self.context.types.as_table(),
                            index,
                            expected_ty,
                            arg_ty,
                        ),
                    );
                }
            }
        }
    }

    /// A method's `self` parameter is the bare class (`Struct(def)`) while the
    /// receiver of an instantiated generic class is `Applied(base, args)`
    /// whose base is that same class: the receiver is an instance of the
    /// method's `self` class.
    fn receiver_is_instance_of_self(&self, expected: MirType, actual: MirType) -> bool {
        let types = self.context.types.as_table();
        let peel = |mut ty: TypeId| loop {
            match types.get(ty) {
                Type::Alias(_, inner) | Type::Ref(_, inner) => ty = *inner,
                _ => return ty,
            }
        };
        let Type::Struct(expected_def) = types.get(peel(expected.semantic_id())) else {
            return false;
        };
        let Type::Applied(base, _) = types.get(peel(actual.semantic_id())) else {
            return false;
        };
        matches!(types.get(peel(*base)), Type::Struct(def) if def == expected_def)
    }

    /// A `_` extent in a parameter type falls through to the parameter's own
    /// axis (size-type Z3 law 3), so the formal dimension is the argument's
    /// extent by construction: it is a wildcard bound at run time and every
    /// argument of the same element type and rank satisfies it. Axes that are
    /// not the parameter's own formal axis must still be equal.
    fn call_argument_matches_own_axes(
        &self,
        expected: MirType,
        actual: MirType,
        argument: u32,
    ) -> bool {
        let types = self.context.types.as_table();
        if crate::generic::type_contains_param(types, expected.semantic_id()) {
            return false;
        }
        let (Some((expected_element, expected_dims)), Some((actual_element, actual_dims))) = (
            tensor_type_dims(types, expected.semantic_id()),
            tensor_type_dims(types, actual.semantic_id()),
        ) else {
            return false;
        };
        if !types.equals(expected_element, actual_element)
            || expected_dims.len() != actual_dims.len()
        {
            return false;
        }
        let mut wildcard = false;
        for (axis, (expected_dim, actual_dim)) in expected_dims.iter().zip(&actual_dims).enumerate()
        {
            if types.index_equals(*expected_dim, *actual_dim) {
                continue;
            }
            match types.get_index(*expected_dim) {
                IndexExpr::Runtime(RuntimeExtentId::FunctionParameterAxis {
                    parameter,
                    axis: formal_axis,
                }) if *parameter == argument && *formal_axis as usize == axis => wildcard = true,
                _ => return false,
            }
        }
        wildcard
    }

    fn call_argument_matches_shape_check(
        &self,
        scope: &FunctionScope<'_, '_>,
        expected: MirType,
        actual: MirType,
        argument: u32,
        shape_check: Option<&MirRuntimeCall>,
    ) -> bool {
        let Some(shape_check) = shape_check else {
            return false;
        };
        let MirIntrinsic::TensorShapeCheck(check) = &shape_check.intrinsic else {
            return false;
        };
        let types = self.context.types.as_table();
        if crate::generic::type_contains_param(types, expected.semantic_id()) {
            return false;
        }
        let Some((expected_element, expected_dims)) =
            tensor_type_dims(types, expected.semantic_id())
        else {
            return false;
        };
        let Some((actual_element, actual_dims)) = tensor_type_dims(types, actual.semantic_id())
        else {
            return false;
        };
        if !types.equals(expected_element, actual_element)
            || expected_dims.len() != actual_dims.len()
        {
            return false;
        }

        let mut authorized_runtime_axis = false;
        for (axis, (expected_dim, actual_dim)) in expected_dims.iter().zip(&actual_dims).enumerate()
        {
            if types.index_equals(*expected_dim, *actual_dim) {
                continue;
            }
            let IndexExpr::Runtime(RuntimeExtentId::FunctionParameterAxis {
                parameter,
                axis: formal_axis,
            }) = types.get_index(*expected_dim)
            else {
                return false;
            };
            let carrier = MirShapeExpr::TensorAxis {
                argument: *parameter,
                axis: *formal_axis,
            };
            if !check.size_bindings.iter().any(|binding| {
                check.proves_equal(&binding.witness, &carrier)
                    || check.relations.iter().any(|relation| {
                        [&relation.left, &relation.right]
                            .into_iter()
                            .any(|endpoint| {
                                shape_expr_uses_witness(endpoint, &binding.witness)
                                    && check.proves_equal(endpoint, &carrier)
                            })
                    })
            }) {
                return false;
            }
            let Ok(axis) = u32::try_from(axis) else {
                return false;
            };
            let current = MirShapeExpr::TensorAxis { argument, axis };
            if current == carrier {
                authorized_runtime_axis = true;
                continue;
            }
            let Some(carrier_operand) = shape_check.args.get(*parameter as usize) else {
                return false;
            };
            let Some(carrier_ty) = direct_operand_type(scope, carrier_operand) else {
                return false;
            };
            let Some((_, carrier_dims)) = tensor_type_dims(types, carrier_ty.semantic_id()) else {
                return false;
            };
            let Some(carrier_dim) = carrier_dims.get(*formal_axis as usize) else {
                return false;
            };
            if types.index_equals(*carrier_dim, *actual_dim) {
                authorized_runtime_axis = true;
                continue;
            }
            if !index_has_runtime_extent(types, *carrier_dim)
                || !index_has_runtime_extent(types, *actual_dim)
            {
                return false;
            }
            if !check.proves_equal(&carrier, &current) {
                return false;
            }
            authorized_runtime_axis = true;
        }
        authorized_runtime_axis
    }

    pub(crate) fn call_destination_matches_shape_check(
        &self,
        scope: &FunctionScope<'_, '_>,
        destination: Option<&MirPlace>,
        expected: Option<MirType>,
        args: &[MirOperand],
        shape_check: Option<&MirRuntimeCall>,
    ) -> bool {
        let (Some(destination), Some(expected), Some(_)) = (destination, expected, shape_check)
        else {
            return false;
        };
        let Some(actual) = direct_operand_type(scope, &MirOperand::Place(destination.clone()))
        else {
            return false;
        };
        let types = self.context.types.as_table();
        let (Some((element, expected_dims)), Some((actual_element, actual_dims))) = (
            tensor_type_dims(types, expected.semantic_id()),
            tensor_type_dims(types, actual.semantic_id()),
        ) else {
            return false;
        };
        if !types.equals(element, actual_element) || expected_dims.len() != actual_dims.len() {
            return false;
        }
        let mut mapped_runtime_axis = false;
        for (expected_dim, actual_dim) in expected_dims.iter().zip(&actual_dims) {
            if types.index_equals(*expected_dim, *actual_dim) {
                continue;
            }
            let IndexExpr::Runtime(RuntimeExtentId::FunctionParameterAxis { parameter, axis }) =
                types.get_index(*expected_dim)
            else {
                return false;
            };
            let Some(carrier_ty) = args
                .get(*parameter as usize)
                .and_then(|arg| direct_operand_type(scope, arg))
            else {
                return false;
            };
            let Some((_, carrier_dims)) = tensor_type_dims(types, carrier_ty.semantic_id()) else {
                return false;
            };
            if !carrier_dims
                .get(*axis as usize)
                .is_some_and(|dim| types.index_equals(*dim, *actual_dim))
                || !self.call_argument_matches_shape_check(
                    scope,
                    expected,
                    carrier_ty,
                    *parameter,
                    shape_check,
                )
            {
                return false;
            }
            mapped_runtime_axis = true;
        }
        mapped_runtime_axis
    }

    pub(crate) fn bind_generic_type_witness(
        &mut self,
        expected: MirType,
        actual: MirType,
        generic_bindings: &mut GenericTypeBindings,
        shape_bindings: &mut MirShapeBindings,
        span: Span,
        message: impl Into<String>,
    ) {
        if self.assignable(actual, expected)
            && matches!(
                self.context.types.as_table().get(expected.semantic_id()),
                Type::Interface(_)
            )
        {
            return;
        }
        if crate::generic::bind_generic_type_witness_with_shapes(
            self.context.types.as_table(),
            expected,
            actual,
            generic_bindings,
            shape_bindings,
        ) {
            return;
        }
        self.error(span, message);
    }

    /// Read-only instantiation against the context's type table, sharing one
    /// [`crate::generic::ExistingTypeIndex`] across the whole validation run.
    pub(crate) fn instantiate_existing(
        &self,
        ty: MirType,
        generic_bindings: &GenericTypeBindings,
        shape_bindings: &MirShapeBindings,
        span: Span,
    ) -> Result<MirType, crate::generic::MirGenericError> {
        let types = self.context.types.as_table();
        let index = self
            .existing_type_index
            .get_or_init(|| crate::generic::ExistingTypeIndex::build(types));
        crate::generic::instantiate_existing_generic_type_with_index(
            types,
            ty,
            generic_bindings,
            shape_bindings,
            span,
            index,
        )
    }

    pub(crate) fn instantiate_generic_type(
        &mut self,
        ty: MirType,
        generic_bindings: &GenericTypeBindings,
        shape_bindings: &MirShapeBindings,
        span: Span,
    ) -> Option<MirType> {
        match self.instantiate_existing(ty, generic_bindings, shape_bindings, span) {
            Ok(ty) => Some(ty),
            Err(error) => {
                self.error(error.span, error.message);
                None
            }
        }
    }

    pub(crate) fn validate_operand(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        operand: &MirOperand,
        span: Span,
    ) -> Option<MirType> {
        match operand {
            MirOperand::Place(place) => self.validate_place(scope, place, span),
            MirOperand::Temp(id) => {
                if let Some(ty) = scope.temps.get(id).copied() {
                    Some(ty)
                } else {
                    self.error(span, format!("temp %{} does not exist", id.0));
                    None
                }
            }
            MirOperand::Value(id) => {
                if let Some(ty) = scope.values.get(id).copied() {
                    Some(ty)
                } else {
                    self.error(
                        span,
                        format!("value v{} is not defined earlier in MIR", id.0),
                    );
                    None
                }
            }
            MirOperand::Constant(value) => Some(self.constant_ty(value)),
        }
    }

    pub(crate) fn validate_place(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        place: &MirPlace,
        span: Span,
    ) -> Option<MirType> {
        let mut ty = match place.base {
            MirPlaceBase::Local(id) => {
                if let Some(ty) = scope.locals.get(&id).copied() {
                    ty
                } else {
                    self.error(span, format!("local _{} does not exist", id.0));
                    return None;
                }
            }
            MirPlaceBase::Temp(id) => {
                if let Some(ty) = scope.temps.get(&id).copied() {
                    ty
                } else {
                    self.error(span, format!("temp %{} does not exist", id.0));
                    return None;
                }
            }
        };

        for projection in &place.projections {
            let base_ty = ty;
            ty = match projection {
                MirProjection::Field(field) => self.project_field(ty, *field, span)?,
                MirProjection::VariantField { variant, field } => {
                    self.project_variant_field(ty, *variant, *field, span)?
                }
                MirProjection::ClosureCapture {
                    environment,
                    source,
                } => self.project_closure_capture(*environment, *source, span)?,
                MirProjection::SharedCell => ty,
                MirProjection::Index(index) => {
                    let index_ty = self.validate_operand(scope, index, span);
                    self.project_index(ty, index_ty.map(|ty| (index, ty)), span)?
                }
                MirProjection::VectorLane(index) => self.project_vector_lane(ty, *index, span)?,
                MirProjection::MatrixCell { row, column } => {
                    self.project_matrix_cell(ty, *row, *column, span)?
                }
            };
            if !matches!(
                projection,
                MirProjection::ClosureCapture { .. } | MirProjection::SharedCell
            ) {
                self.validate_projection_representation(base_ty, ty, projection, span);
            }
        }

        Some(ty)
    }
}

fn shape_expr_uses_witness(expression: &MirShapeExpr, witness: &MirShapeExpr) -> bool {
    expression == witness
        || match expression {
            MirShapeExpr::Affine { base, .. } => shape_expr_uses_witness(base, witness),
            MirShapeExpr::Divide { numerator, divisor } if *divisor != 0 => {
                shape_expr_uses_witness(numerator, witness)
            }
            _ => false,
        }
}

fn index_has_runtime_extent(types: &TypeTable, mut index: IndexId) -> bool {
    for _ in 0..types.index_count() {
        index = match types.get_index(index) {
            IndexExpr::Runtime(_) => return true,
            IndexExpr::Affine { base, .. } => *base,
            IndexExpr::Divide { numerator, divisor } if *divisor != 0 => *numerator,
            _ => return false,
        };
    }
    false
}

fn direct_operand_type(scope: &FunctionScope<'_, '_>, operand: &MirOperand) -> Option<MirType> {
    match operand {
        MirOperand::Place(place) if place.projections.is_empty() => match &place.base {
            MirPlaceBase::Local(id) => scope.locals.get(id).copied(),
            MirPlaceBase::Temp(id) => scope.temps.get(id).copied(),
        },
        MirOperand::Temp(id) => scope.temps.get(id).copied(),
        MirOperand::Value(id) => scope.values.get(id).copied(),
        _ => None,
    }
}

pub(super) fn tensor_type_dims(
    types: &TypeTable,
    mut ty: TypeId,
) -> Option<(TypeId, Vec<IndexId>)> {
    loop {
        match types.get(ty) {
            Type::Alias(_, inner) | Type::Ref(_, inner) => ty = *inner,
            Type::Tensor(element, shape) => {
                let dimensions = match types.get_index(*shape) {
                    IndexExpr::Tuple(dimensions) => dimensions.clone(),
                    IndexExpr::Unspecified => return None,
                    _ => vec![*shape],
                };
                return Some((*element, dimensions));
            }
            _ => return None,
        }
    }
}

fn call_argument_mismatch(
    types: &TypeTable,
    index: usize,
    expected: MirType,
    actual: MirType,
) -> String {
    format!(
        "call argument type mismatch (arg {index}: expected {}, got {})",
        spell_type_id(types, expected.semantic_id(), 0),
        spell_type_id(types, actual.semantic_id(), 0),
    )
}

fn spell_type_id(types: &TypeTable, id: TypeId, depth: u8) -> String {
    if depth > 6 {
        return "_".to_owned();
    }
    match types.get(id) {
        Type::Primitive(prim) => match prim {
            Primitive::Ascii => "ascii",
            Primitive::Littera => "littera",
            Primitive::Textus => "textus",
            Primitive::Numerus => "numerus",
            Primitive::Fractus => "fractus",
            Primitive::Bivalens => "bivalens",
            Primitive::Nihil => "nihil",
            Primitive::Vacuum => "vacuum",
            Primitive::Numquam => "numquam",
            Primitive::Ignotum => "ignotum",
            Primitive::Octeti => "octeti",
            Primitive::Regex => "regex",
            Primitive::Match => "match",
            Primitive::Json => "json",
            Primitive::Valor => "valor",
            Primitive::Instans => "instans",
        }
        .to_owned(),
        // Bare width marker: the wrapped `numerus<W>` / `fractus<W>` spellings
        // are retired (N7c).
        Type::SizedNumeric(prim, width) => match prim {
            Primitive::Numerus | Primitive::Fractus => width.faber_marker().to_owned(),
            _ => "ignotum".to_owned(),
        },
        Type::Tensor(elem, shape) => format!(
            "tensor<{}, {}>",
            spell_type_id(types, *elem, depth + 1),
            spell_index_at_precedence(types, *shape, depth + 1, 0).0
        ),
        Type::Array(elem) => format!("lista<{}>", spell_type_id(types, *elem, depth + 1)),
        Type::Alias(_, inner) => spell_type_id(types, *inner, depth + 1),
        Type::Param(_) => "T".to_owned(),
        other => format!("{other:?}"),
    }
}

fn spell_index_at_precedence(
    types: &TypeTable,
    id: IndexId,
    depth: u8,
    parent_precedence: u8,
) -> (String, u8) {
    if depth > 6 {
        return ("_".to_owned(), 3);
    }
    match types.get_index(id) {
        IndexExpr::Literal(n) => (n.to_string(), 3),
        IndexExpr::Runtime(source) => (format!("<runtime:{source:?}>"), 3),
        IndexExpr::Affine { base, offset } => {
            let (base, _) = spell_index_at_precedence(types, *base, depth + 1, 1);
            let text = match (*offset).cmp(&0) {
                std::cmp::Ordering::Less => format!("{base} - {}", offset.unsigned_abs()),
                std::cmp::Ordering::Greater => format!("{base} + {offset}"),
                std::cmp::Ordering::Equal => base,
            };
            parenthesize_index(text, 1, parent_precedence)
        }
        IndexExpr::Divide { numerator, divisor } => {
            let (numerator, _) = spell_index_at_precedence(types, *numerator, depth + 1, 2);
            parenthesize_index(format!("{numerator} / {divisor}"), 2, parent_precedence)
        }
        IndexExpr::Tuple(dims) => {
            let parts = dims
                .iter()
                .map(|dim| spell_index_at_precedence(types, *dim, depth + 1, 0).0)
                .collect::<Vec<_>>();
            (format!("[{}]", parts.join(", ")), 3)
        }
        IndexExpr::Param(_) => ("N".to_owned(), 3),
        IndexExpr::Infer(_) | IndexExpr::Unspecified => ("_".to_owned(), 3),
    }
}

fn parenthesize_index(text: String, precedence: u8, parent_precedence: u8) -> (String, u8) {
    if precedence < parent_precedence {
        (format!("({text})"), precedence)
    } else {
        (text, precedence)
    }
}
