//! Typecheck for tensor type-directed construction and reshape clean break.
//!
//! COMPILER PHASE: Semantic typecheck
//! INPUT: Tensor namespace method calls (creata, structa, formata) and tensor
//! receiver methods (crea, forma) with resolved DefIds and explicit type arguments
//! OUTPUT: Checked call sites with finalized TypeIds on tensor values; structured
//! diagnostics for removed or stale API shapes (structa, creata type args, argument order)

use super::TypeChecker;
use crate::tensor_type_directed::complete_tensor_type;
use crate::{Type, TypeId, numeric_width, tensor_bracket};
use radix_hir::{HirCallArg, HirExpression};
use radix_lexer::Span;

const ISSUE_ARGUMENT_TYPE_MISMATCH: &str = "argument_type_mismatch";
const ISSUE_TENSOR_RECEIVER_STRUCTA_RENAMED: &str = "tensor_receiver_structa_renamed";
const ISSUE_TENSOR_RECEIVER_CREATA_TYPE_ARGS: &str = "tensor_receiver_creata_type_args";
const ISSUE_TENSOR_CREA_ARGUMENT_ORDER: &str = "tensor_crea_argument_order";
const ISSUE_TENSOR_STRUCTA_FORMA_REMOVED: &str = "tensor_structa_forma_removed";
const ISSUE_TENSOR_CREATA_RUNTIME_SHAPE_REMOVED: &str = "tensor_creata_runtime_shape_removed";

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

        if method_name == "structa_forma" {
            self.invalid_operand_types(ISSUE_TENSOR_STRUCTA_FORMA_REMOVED, span);
            self.typecheck_call_arg_exprs(args);
            return Some(self.error_type);
        }
        if method_name == "creata" && explicit_type_args.is_empty() && args.len() == 2 {
            self.invalid_operand_types(ISSUE_TENSOR_CREATA_RUNTIME_SHAPE_REMOVED, span);
            self.typecheck_call_arg_exprs(args);
            return Some(self.error_type);
        }
        let Some(entry) = radix_intrinsics::lookup_namespace_method_by_name("tensor", method_name)
        else {
            return None;
        };
        Some(self.check_tensor_namespace_catalog_call(entry, explicit_type_args, args, span))
    }

    fn check_tensor_namespace_catalog_call(
        &mut self,
        entry: &radix_intrinsics::NamespaceIntrinsic,
        explicit_type_args: &[TypeId],
        args: &mut [HirCallArg],
        span: Span,
    ) -> TypeId {
        if super::intrinsics::reject_catalog_arity(
            self,
            entry.generic,
            entry.arg_count,
            entry.missing_generic_issue,
            entry.excess_generic_issue,
            entry.value_arity_issue,
            entry.diagnostic_method,
            explicit_type_args,
            args,
            span,
        ) {
            return self.error_type;
        }
        let type_params = super::intrinsics::interned_catalog_generics(self, entry.generic);
        let Some(param_name) = type_params.first().map(|param| param.name) else {
            return self.error_type;
        };
        let ret = self.types.intern(crate::Type::Param(param_name));
        let sig = crate::FuncSig {
            type_params,
            params: Vec::new(),
            ret,
            err: None,
            is_async: false,
            is_generator: false,
        };
        let mut bind_args = Vec::new();
        let bound =
            self.check_instantiated_call(&sig, explicit_type_args, &mut bind_args, span, None);
        let dummy_receiver = self.vacuum_type();
        let target =
            super::intrinsics::validate_generic_typus_tensor(self, dummy_receiver, bound, span);
        if target == self.error_type {
            self.typecheck_call_arg_exprs(args);
            return self.error_type;
        }
        match entry.faber_name {
            "creata" => self.check_tensor_module_creata_fill(target, args, span),
            "structa" => self.check_tensor_module_structa_data(target, args, span),
            _ => {
                self.typecheck_call_arg_exprs(args);
                self.error_type
            }
        }
    }

    fn check_tensor_module_creata_fill(
        &mut self,
        target: TypeId,
        args: &mut [HirCallArg],
        span: Span,
    ) -> TypeId {
        let resolved = self.resolve_inferred_type(target);
        let Some(complete) = complete_tensor_type(resolved, self.types) else {
            return self.error_type;
        };
        if args.len() != 1 {
            self.wrong_arity("wrong_number_of_arguments", span);
            self.typecheck_call_arg_exprs(args);
            return self.error_type;
        }
        let arg_ty = self.check_expr_with_expected(&mut args[0].expr, Some(complete.elem));
        self.unify_assign_issue(
            arg_ty,
            complete.elem,
            args[0].span,
            ISSUE_ARGUMENT_TYPE_MISMATCH,
        );
        self.types.tensor_with_shape(complete.elem, complete.shape)
    }

    fn check_tensor_module_structa_data(
        &mut self,
        target: TypeId,
        args: &mut [HirCallArg],
        span: Span,
    ) -> TypeId {
        let resolved = self.resolve_inferred_type(target);
        let Some(complete) = complete_tensor_type(resolved, self.types) else {
            return self.error_type;
        };
        if args.len() != 1 {
            self.wrong_arity("wrong_number_of_arguments", span);
            self.typecheck_call_arg_exprs(args);
            return self.error_type;
        }
        let list_ty = self.types.array(complete.elem);
        let arg_ty = self.check_expr_with_expected(&mut args[0].expr, Some(list_ty));
        self.unify_assign_issue(arg_ty, list_ty, args[0].span, ISSUE_ARGUMENT_TYPE_MISMATCH);
        self.types.tensor_with_shape(complete.elem, complete.shape)
    }

    fn arg_is_integer_index_list(&self, expr: &HirExpression) -> bool {
        tensor_bracket::is_bracket_index_list_expr(expr, self.types)
            || expr
                .ty
                .is_some_and(|ty| numeric_width::is_integer_index_vector(self.types, ty))
    }
}
