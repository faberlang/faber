//! Typecheck for tensor type-directed construction and reshape clean break.
//!
//! COMPILER PHASE: Semantic typecheck
//! INPUT: Tensor namespace method calls (creata, structa, formata) and tensor
//! receiver methods (crea, strue, forma) with resolved `DefIds` and explicit type arguments
//! OUTPUT: Checked call sites with finalized `TypeIds` on tensor values; structured
//! diagnostics for stale API shapes (structa, creata type args, retired crea/strue)

use super::TypeChecker;
use super::intrinsics::ISSUE_TENSOR_INDEX_VECTOR_INTEGER_ELEMENTS;
use crate::tensor_type_directed::complete_tensor_type;
use crate::{Type, TypeId, numeric_width};
use radix_hir::{HirCallArg, HirExpression, HirExpressionKind};
use radix_lexer::{Span, Symbol};

const ISSUE_ARGUMENT_TYPE_MISMATCH: &str = "argument_type_mismatch";
const ISSUE_TENSOR_RECEIVER_STRUCTA_RENAMED: &str = "tensor_receiver_structa_renamed";
const ISSUE_TENSOR_RECEIVER_CREATA_TYPE_ARGS: &str = "tensor_receiver_creata_type_args";
const ISSUE_TENSOR_STRUE_RETIRED: &str = "tensor_strue_retired";
const ISSUE_TENSOR_CREA_RETIRED: &str = "tensor_crea_retired";
const ISSUE_TENSOR_STRUCTA_FORMA_REMOVED: &str = "tensor_structa_forma_removed";

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
        receiver: &HirExpression,
        receiver_ty: TypeId,
        name: &mut Symbol,
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

        // The population methods are retired (tensor-conversions C3): the
        // `flat ↦ tensor` conversion and the type-expression constructor own
        // their work. The Tensor registry rows are gone, so the family-scoped
        // surface→canonical cache no longer maps the retired spellings;
        // compare the written name against each retired canonical's pack
        // surface directly (canonical→surface is family-unscoped and the
        // `crea` row stays — Vector owns the canonical now). The English
        // `from_flat` surface retired with its pack row, so it stays as a
        // literal here to name the retirement for un-migrated call sites.
        // This table runs before intrinsic resolution, so a call names its
        // retirement instead of falling through to unknown-method.
        let crea_surface = self
            .locale_pack
            .map_or("crea", |pack| pack.intrinsic_surface("crea"));
        let strue_surface = self
            .locale_pack
            .map_or("strue", |pack| pack.intrinsic_surface("strue"));
        // The type-expression constructor (size-type Z2) synthesizes exactly
        // this call — a vacua receiver pre-typed with the claimed type, the
        // pack's surface spelling of `crea`, fill then extents
        // (`lower_construct`). The receiver-row retirement removed the
        // registry entry it typed and lowered through, so the constructor is
        // recognized here by that shape; everything else on the retired
        // names declines.
        // The empty-literal rewrite (`empty_dense`) synthesizes the same call
        // with whichever spelling the interner holds; only a synthesized
        // receiver carries its tensor type, so authored `.crea(..)` stays
        // retired.
        let is_construct = matches!(receiver.kind, HirExpressionKind::Vacua)
            && (method_name == crea_surface || (method_name == "crea" && receiver.ty.is_some()))
            && args.len() == 2;
        if is_construct {
            return Some(self.check_tensor_construct(receiver_ty, name, args, span));
        }
        let retired = if method_name == strue_surface || method_name == "from_flat" {
            Some(ISSUE_TENSOR_STRUE_RETIRED)
        } else if method_name == crea_surface || method_name == "crea" {
            Some(ISSUE_TENSOR_CREA_RETIRED)
        } else {
            None
        };
        if let Some(issue) = retired {
            self.invalid_operand_types(issue, span);
            self.typecheck_call_arg_exprs(args);
            return Some(self.error_type);
        }

        None
    }

    /// Type the type-expression constructor's synthesized `crea` call: the
    /// claimed type is authoritative, the fill unifies against its element,
    /// the extents stay an integer index lista. Rewrites the HIR symbol to
    /// the canonical spelling the MIR lane and filum freshness match.
    fn check_tensor_construct(
        &mut self,
        claimed: TypeId,
        name: &mut Symbol,
        args: &mut [HirCallArg],
        span: Span,
    ) -> TypeId {
        let resolved = self.resolve_inferred_type(claimed);
        let Some(elem) = (match self.types.get(resolved) {
            Type::Tensor(elem, _) => Some(*elem),
            _ => None,
        }) else {
            return self.error_type;
        };
        let fill = self.check_expr_with_expected(&mut args[0].expr, Some(elem));
        self.unify_assign_issue(fill, elem, args[0].span, ISSUE_ARGUMENT_TYPE_MISMATCH);
        let extents = self.check_expr(&mut args[1].expr);
        if !numeric_width::is_integer_index_vector(self.types, extents) {
            self.invalid_operand_types(ISSUE_TENSOR_INDEX_VECTOR_INTEGER_ELEMENTS, span);
            return self.error_type;
        }
        if let Some(interner) = self.interner
            && let Some(canonical) = interner.lookup("crea")
        {
            *name = canonical;
        }
        claimed
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

        let entry = radix_intrinsics::lookup_namespace_method_by_name("tensor", method_name)?;
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
}
