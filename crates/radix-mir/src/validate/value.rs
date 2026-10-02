//! Value, option-operation, operand, place, callee, and call-argument checks.

use super::{
    FunctionScope, GenericTypeBindings, MirFunctionSignature, Validator, signature_from_func_sig,
    signature_from_function,
};
use crate::generic::MirShapeBindings;
use crate::{
    MirBinOp, MirCallee, MirClosureCallee, MirConstant, MirOperand, MirOptionChainLink,
    MirOptionOp, MirPlace, MirPlaceBase, MirProjection, MirType, MirUnOp, MirValue, MirValueKind,
};
use radix_hir::DefId;
use radix_lexer::Span;
use radix_types::{IndexExpr, IndexId, Primitive, Type, TypeId, TypeTable};

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
                            && !self.is_numeric_integral_type(operand_ty)
                        {
                            self.error(value.span, "bitwise not operand is not numeric integral");
                        }
                        if !self.is_numeric_integral_type(value.ty) {
                            self.error(value.span, "bitwise not result is not numeric integral");
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
                            && !self.is_numeric_integral_type(ty)
                        {
                            self.error(value.span, "bitwise lhs is not numeric integral");
                        }
                        if let Some(ty) = rhs_ty
                            && !self.is_numeric_integral_type(ty)
                        {
                            self.error(value.span, "bitwise rhs is not numeric integral");
                        }
                    }
                }
            }
            MirValueKind::Option(op) => self.validate_option_op(scope, op, value.ty, value.span),
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
                let sig = self.validate_callee(scope, callee, span);
                let mut generic_bindings = GenericTypeBindings::default();
                let mut shape_bindings = MirShapeBindings::default();
                self.validate_call_args(
                    scope,
                    sig.as_ref(),
                    args,
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
                .or_else(|| Self::signature_from_program_def(scope, *def_id)),
            MirCallee::Closure(closure) => self.validate_closure_callee(scope, closure, span),
            MirCallee::Value(value) => {
                let operand_ty = self.validate_operand(scope, value, span);
                let sig = operand_ty
                    .and_then(|ty| self.signature_from_semantic_callable_type(ty.semantic_id()));
                if operand_ty.is_some() && sig.is_none() {
                    self.error(span, "value callee is not callable");
                }
                sig
            }
        }
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

    pub(crate) fn signature_from_semantic_callable_type(
        &self,
        ty: TypeId,
    ) -> Option<MirFunctionSignature> {
        match self.context.types.get(ty) {
            Type::Func(sig) => Some(signature_from_func_sig(sig)),
            Type::Alias(_, inner) => self.signature_from_semantic_callable_type(*inner),
            _ => None,
        }
    }

    pub(crate) fn validate_call_args(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        sig: Option<&MirFunctionSignature>,
        args: &[MirOperand],
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

    pub(crate) fn instantiate_generic_type(
        &mut self,
        ty: MirType,
        generic_bindings: &GenericTypeBindings,
        shape_bindings: &MirShapeBindings,
        span: Span,
    ) -> Option<MirType> {
        match crate::generic::instantiate_existing_generic_type_with_shapes(
            self.context.types.as_table(),
            ty,
            generic_bindings,
            shape_bindings,
            span,
        ) {
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

fn call_argument_mismatch(
    types: &TypeTable,
    index: usize,
    expected: MirType,
    actual: MirType,
) -> String {
    format!(
        "call argument type mismatch (arg {index}: expected {}, got {})",
        spell_mismatch_ty(types, expected),
        spell_mismatch_ty(types, actual),
    )
}

fn spell_mismatch_ty(types: &TypeTable, ty: MirType) -> String {
    spell_type_id(types, ty.semantic_id(), 0)
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
            spell_index(types, *shape, depth + 1)
        ),
        Type::Array(elem) => format!("lista<{}>", spell_type_id(types, *elem, depth + 1)),
        Type::Alias(_, inner) => spell_type_id(types, *inner, depth + 1),
        Type::Param(_) => "T".to_owned(),
        other => format!("{other:?}"),
    }
}

fn spell_index(types: &TypeTable, id: IndexId, depth: u8) -> String {
    spell_index_at_precedence(types, id, depth, 0).0
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
            let text = if *offset < 0 {
                format!("{base} - {}", offset.unsigned_abs())
            } else if *offset > 0 {
                format!("{base} + {offset}")
            } else {
                base
            };
            parenthesize_index(text, 1, parent_precedence)
        }
        IndexExpr::Divide { numerator, divisor } => {
            let (numerator, _) =
                spell_index_at_precedence(types, *numerator, depth + 1, 2);
            parenthesize_index(
                format!("{numerator} / {divisor}"),
                2,
                parent_precedence,
            )
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
