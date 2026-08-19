//! Typecheck for tensor type-directed construction and reshape clean break.
//!
//! COMPILER PHASE: Semantic typecheck
//! INPUT: Tensor namespace method calls (creata, structa, formata) and tensor
//! receiver methods (crea, forma) with resolved DefIds and explicit type arguments
//! OUTPUT: Checked call sites with finalized TypeIds on tensor values; structured
//! diagnostics for removed or stale API shapes (structa, creata type args, argument order)

use super::TypeChecker;
use crate::tensor_type_directed::{complete_tensor_type, is_tensor_type, CompleteTensorType};
use crate::{numeric_width, tensor_bracket, SemanticErrorKind, Type, TypeId};
use radix_hir::{HirCallArg, HirExpression};
use radix_lexer::Span;

const ISSUE_ARGUMENT_TYPE_MISMATCH: &str = "argument_type_mismatch";
const ISSUE_TENSOR_RECEIVER_STRUCTA_RENAMED: &str = "tensor_receiver_structa_renamed";
const ISSUE_TENSOR_RECEIVER_CREATA_TYPE_ARGS: &str = "tensor_receiver_creata_type_args";
const ISSUE_TENSOR_CREA_ARGUMENT_ORDER: &str = "tensor_crea_argument_order";
const ISSUE_TENSOR_STRUCTA_FORMA_REMOVED: &str = "tensor_structa_forma_removed";
const ISSUE_TENSOR_CREATA_RUNTIME_SHAPE_REMOVED: &str = "tensor_creata_runtime_shape_removed";
const ISSUE_TENSOR_CREATA_COMPLETE_TYPE_ARG_REQUIRED: &str =
    "tensor_creata_complete_type_arg_required";
const ISSUE_TENSOR_FORMATA_COMPLETE_TYPE_ARG_REQUIRED: &str =
    "tensor_formata_complete_type_arg_required";
const ISSUE_TENSOR_FORMATA_FORBIDS_VALUE_ARGS: &str = "tensor_formata_forbids_value_args";
const ISSUE_TENSOR_FORMATA_ELEMENT_TYPE_MISMATCH: &str = "tensor_formata_element_type_mismatch";
const ISSUE_TENSOR_TYPE_ARG_INCOMPLETE: &str = "tensor_type_arg_incomplete";

impl TypeChecker<'_> {
    pub fn is_tensor_namespace(&self, module_def_id: radix_hir::DefId) -> bool {
        let Some(symbol) = self.resolver.get_symbol(module_def_id) else {
            return false;
        };
        self.interner
            .is_some_and(|interner| interner.resolve(symbol.name) == "tensor")
    }

    pub fn check_tensor_receiver_method_call(
        &mut self,
        receiver_ty: TypeId,
        method_name: &str,
        explicit_type_args: &[TypeId],
        args: &mut [HirCallArg],
        span: Span,
    ) -> Option<TypeId> {
        let resolved = self.resolve_inferred_type(receiver_ty);
        if !matches!(self.types.get(resolved), Type::Tensor(_, _)) {
            return None;
        }

        if method_name == "structa" {
            self.invalid_operand_types(ISSUE_TENSOR_RECEIVER_STRUCTA_RENAMED, span);
            self.typecheck_call_arg_exprs(args);
            return Some(self.error_type);
        }

        if method_name == "creata" && !explicit_type_args.is_empty() {
            self.invalid_operand_types(ISSUE_TENSOR_RECEIVER_CREATA_TYPE_ARGS, span);
            self.typecheck_call_arg_exprs(args);
            return Some(self.error_type);
        }

        if method_name == "crea"
            && explicit_type_args.is_empty()
            && args.len() == 2
            && self.arg_is_integer_index_list(&args[0].expr)
            && !self.arg_is_integer_index_list(&args[1].expr)
        {
            self.invalid_operand_types(ISSUE_TENSOR_CREA_ARGUMENT_ORDER, span);
            self.typecheck_call_arg_exprs(args);
            return Some(self.error_type);
        }

        if method_name == "formata" {
            return Some(self.check_tensor_formata(receiver_ty, explicit_type_args, args, span));
        }

        None
    }

    pub fn check_tensor_namespace_method_call(
        &mut self,
        module_def_id: radix_hir::DefId,
        method_name: &str,
        explicit_type_args: &[TypeId],
        args: &mut [HirCallArg],
        span: Span,
    ) -> Option<TypeId> {
        if !self.is_tensor_namespace(module_def_id) {
            return None;
        }

        match method_name {
            "structa_forma" => {
                self.invalid_operand_types(ISSUE_TENSOR_STRUCTA_FORMA_REMOVED, span);
                self.typecheck_call_arg_exprs(args);
                Some(self.error_type)
            }
            "creata" if explicit_type_args.is_empty() => {
                if args.len() == 2 {
                    self.invalid_operand_types(ISSUE_TENSOR_CREATA_RUNTIME_SHAPE_REMOVED, span);
                } else {
                    self.invalid_operand_types(
                        ISSUE_TENSOR_CREATA_COMPLETE_TYPE_ARG_REQUIRED,
                        span,
                    );
                }
                self.typecheck_call_arg_exprs(args);
                Some(self.error_type)
            }
            "creata" => Some(self.check_tensor_module_creata(explicit_type_args, args, span)),
            "structa" if !explicit_type_args.is_empty() => {
                Some(self.check_tensor_module_structa(explicit_type_args, args, span))
            }
            _ => None,
        }
    }

    fn check_tensor_module_creata(
        &mut self,
        explicit_type_args: &[TypeId],
        args: &mut [HirCallArg],
        span: Span,
    ) -> TypeId {
        let Some(target) =
            self.require_single_complete_tensor_type_arg(explicit_type_args, args, span)
        else {
            return self.error_type;
        };
        if args.len() != 1 {
            self.wrong_arity("wrong_number_of_arguments", span);
            self.typecheck_call_arg_exprs(args);
            return self.error_type;
        }
        let arg_ty = self.check_expr_with_expected(&mut args[0].expr, Some(target.elem));
        self.unify_assign_issue(
            arg_ty,
            target.elem,
            args[0].span,
            ISSUE_ARGUMENT_TYPE_MISMATCH,
        );
        self.types.tensor_with_shape(target.elem, target.shape)
    }

    fn check_tensor_module_structa(
        &mut self,
        explicit_type_args: &[TypeId],
        args: &mut [HirCallArg],
        span: Span,
    ) -> TypeId {
        let Some(target) =
            self.require_single_complete_tensor_type_arg(explicit_type_args, args, span)
        else {
            return self.error_type;
        };
        if args.len() != 1 {
            self.wrong_arity("wrong_number_of_arguments", span);
            self.typecheck_call_arg_exprs(args);
            return self.error_type;
        }
        let list_ty = self.types.array(target.elem);
        let arg_ty = self.check_expr_with_expected(&mut args[0].expr, Some(list_ty));
        self.unify_assign_issue(arg_ty, list_ty, args[0].span, ISSUE_ARGUMENT_TYPE_MISMATCH);
        self.types.tensor_with_shape(target.elem, target.shape)
    }

    fn check_tensor_formata(
        &mut self,
        receiver_ty: TypeId,
        explicit_type_args: &[TypeId],
        args: &mut [HirCallArg],
        span: Span,
    ) -> TypeId {
        if explicit_type_args.is_empty() {
            self.invalid_operand_types(ISSUE_TENSOR_FORMATA_COMPLETE_TYPE_ARG_REQUIRED, span);
            self.typecheck_call_arg_exprs(args);
            return self.error_type;
        }
        if !args.is_empty() {
            self.invalid_operand_types(ISSUE_TENSOR_FORMATA_FORBIDS_VALUE_ARGS, span);
            self.typecheck_call_arg_exprs(args);
            return self.error_type;
        }
        let Some(target) =
            self.require_single_complete_tensor_type_arg(explicit_type_args, args, span)
        else {
            return self.error_type;
        };
        let resolved = self.resolve_inferred_type(receiver_ty);
        let Type::Tensor(receiver_elem, _) = self.types.get(resolved) else {
            return self.error_type;
        };
        if !self.types.equals(*receiver_elem, target.elem) {
            self.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_TENSOR_FORMATA_ELEMENT_TYPE_MISMATCH,
                span,
            );
            return self.error_type;
        }
        self.types.tensor_with_shape(target.elem, target.shape)
    }

    fn require_single_complete_tensor_type_arg(
        &mut self,
        explicit_type_args: &[TypeId],
        args: &mut [HirCallArg],
        span: Span,
    ) -> Option<CompleteTensorType> {
        if explicit_type_args.len() != 1 {
            self.wrong_arity("wrong_number_of_type_arguments", span);
            self.typecheck_call_arg_exprs(args);
            return None;
        }
        let type_arg = self.resolve_inferred_type(explicit_type_args[0]);
        if !is_tensor_type(type_arg, self.types) {
            self.invalid_operand_types(ISSUE_TENSOR_TYPE_ARG_INCOMPLETE, span);
            self.typecheck_call_arg_exprs(args);
            return None;
        }
        let Some(complete) = complete_tensor_type(type_arg, self.types) else {
            self.invalid_operand_types(ISSUE_TENSOR_TYPE_ARG_INCOMPLETE, span);
            self.typecheck_call_arg_exprs(args);
            return None;
        };
        Some(complete)
    }

    fn arg_is_integer_index_list(&self, expr: &HirExpression) -> bool {
        tensor_bracket::is_bracket_index_list_expr(expr, self.types)
            || expr
                .ty
                .is_some_and(|ty| numeric_width::is_integer_index_vector(self.types, ty))
    }
}
