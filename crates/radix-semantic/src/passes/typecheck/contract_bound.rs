//! Contract bounds on type parameters and generic `implet` (DECISIONS D1.1,
//! D1.2, D1.3; operator default F1).
//!
//! A bound `T implet Orderable<T>` does three things here:
//!
//! - **Well-formedness**: every contract named by a bound or a genus `implet`
//!   clause is an `implendum` taking exactly its declared number of type
//!   arguments. There is no bare-name shorthand for a generic contract.
//! - **Member access**: inside the bounded callable, a method of a bound
//!   contract is callable on a `T` receiver, with the contract's parameters
//!   replaced by the bound's arguments.
//! - **Instantiation**: at every call, each witness must nominally declare the
//!   bound (D1.3) — a genus whose `implet` clause names the contract with the
//!   same arguments, or a caller's own type parameter carrying that bound.

use radix_hir::{HirCallArg, HirTypeParam, HirTypeParamConstraint};
use radix_lexer::Symbol;
use radix_types::{AppliedArg, ContractRef, FuncSig, Type, TypeId, TypeParamConstraint};
use rustc_hash::FxHashMap;

use super::TypeChecker;
use crate::SemanticErrorKind;

/// `implet` (on a genus or a bound) names something that is not an implendum.
pub(super) const ISSUE_IMPLET_REQUIRES_IMPLENDUM: &str = "implet_requires_implendum";

/// A contract is named with the wrong number of type arguments.
const ISSUE_IMPLET_CONTRACT_ARITY: &str = "implet_contract_arity";

/// A call's witness does not declare a bound contract of its parameter.
const ISSUE_GENERIC_TYPE_ARG_BOUND: &str = "generic_type_arg_bound";

impl TypeChecker<'_> {
    /// Check one contract reference: an implendum with matching arity.
    ///
    /// Returns whether the reference is usable; each failure reports once.
    pub(super) fn validate_contract_ref(
        &mut self,
        contract: &ContractRef,
        span: radix_lexer::Span,
    ) -> bool {
        if !self.interfaces.contains_key(&contract.contract) {
            self.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_IMPLET_REQUIRES_IMPLENDUM,
                span,
            );
            return false;
        }
        let expected = self
            .type_params_by_def
            .get(&contract.contract)
            .map_or(0, Vec::len);
        if expected != contract.args.len() {
            let name = self.contract_display_name(contract.contract);
            self.error_issue_with_args(
                SemanticErrorKind::TypeMismatch,
                ISSUE_IMPLET_CONTRACT_ARITY,
                span,
                &[
                    ("contract", name),
                    ("expected", expected.to_string()),
                    ("found", contract.args.len().to_string()),
                ],
            );
            return false;
        }
        true
    }

    /// Validate a callable's bounds and install them for its body.
    ///
    /// Returns the previous bound scope so the caller restores it.
    pub(super) fn enter_generic_bounds(
        &mut self,
        type_params: &[HirTypeParam],
    ) -> FxHashMap<Symbol, Vec<ContractRef>> {
        let mut bounds = FxHashMap::default();
        for param in type_params {
            let HirTypeParamConstraint::Implet(contracts) = &param.constraint else {
                continue;
            };
            let valid: Vec<ContractRef> = contracts
                .iter()
                .filter(|contract| self.validate_contract_ref(contract, param.span))
                .cloned()
                .collect();
            bounds.insert(param.name, valid);
        }
        std::mem::replace(&mut self.current_generic_bounds, bounds)
    }

    /// Check `receiver.method(args)` where the receiver is a bounded type
    /// parameter. Returns `None` when the receiver is not a bounded
    /// parameter or no bound contract declares the method.
    pub(super) fn check_bound_method_call(
        &mut self,
        receiver_ty: TypeId,
        name: Symbol,
        args: &mut [HirCallArg],
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let sig = self.bound_method_signature(receiver_ty, name)?;
        self.check_call_args(&sig, args, span);
        if self.reject_failable_call(&sig, span) {
            return Some(self.error_type);
        }
        Some(self.resolve_inferred_type(sig.ret))
    }

    /// The signature of `name` through the receiver's bounds, with the
    /// contract's parameters replaced by the bound's arguments.
    pub(super) fn bound_method_signature(
        &mut self,
        receiver_ty: TypeId,
        name: Symbol,
    ) -> Option<FuncSig> {
        let param = self.bounded_param_of(receiver_ty)?;
        let bounds = self.current_generic_bounds.get(&param)?.clone();
        for bound in bounds {
            let Some(sig) = self
                .interfaces
                .get(&bound.contract)
                .and_then(|methods| methods.get(&name))
                .map(|method| method.sig.clone())
            else {
                continue;
            };
            let mut vars: FxHashMap<Symbol, TypeId> = self
                .type_params_by_def
                .get(&bound.contract)
                .map(|params| {
                    params
                        .iter()
                        .copied()
                        .zip(bound.args.iter().copied())
                        .collect()
                })
                .unwrap_or_default();
            return Some(self.substitute_func_sig(&sig, &mut vars, None));
        }
        None
    }

    /// The type parameter symbol a receiver type names, through access
    /// markers, when that parameter carries bounds in the current body.
    fn bounded_param_of(&self, ty: TypeId) -> Option<Symbol> {
        let ty = self.resolve_inferred_type(ty);
        let (_, ty) = self.types.peel_access_marker(ty);
        match self.types.get(ty) {
            Type::Param(sym) if self.current_generic_bounds.contains_key(sym) => Some(*sym),
            Type::Ref(_, inner) => self.bounded_param_of(*inner),
            _ => None,
        }
    }

    /// Report each solved witness that does not declare its parameter's
    /// bounds (D1.3). `param_vars` maps every callee parameter to its witness.
    pub(super) fn reject_bound_misses(
        &mut self,
        sig: &FuncSig,
        param_vars: &FxHashMap<Symbol, TypeId>,
        span: radix_lexer::Span,
    ) {
        for param in &sig.type_params {
            let TypeParamConstraint::Implet(bounds) = &param.constraint else {
                continue;
            };
            let Some(witness) = param_vars.get(&param.name).copied() else {
                continue;
            };
            let witness = self.resolve_inferred_type(witness);
            if self.is_infer(witness) || self.infer_union_var_of(witness).is_some() {
                continue;
            }
            for bound in bounds {
                let mut vars = param_vars.clone();
                let args: Vec<TypeId> = bound
                    .args
                    .iter()
                    .map(|arg| {
                        let arg = self.substitute_type_params(*arg, &mut vars, None);
                        self.resolve_inferred_type(arg)
                    })
                    .collect();
                let required = ContractRef {
                    contract: bound.contract,
                    args,
                };
                if self.witness_declares(witness, &required)
                    || self.builtin_satisfies_contract(witness, &required)
                {
                    continue;
                }
                let param_name = self.interner.map_or_else(
                    || "T".to_owned(),
                    |interner| interner.resolve(param.name).to_owned(),
                );
                let rejected_name = self.format_union_constituent_type(witness);
                let contract = self.contract_display(&required);
                self.invalid_operand_types_with_args(
                    ISSUE_GENERIC_TYPE_ARG_BOUND,
                    span,
                    &[
                        ("rejected", rejected_name),
                        ("param", param_name),
                        ("contract", contract),
                    ],
                );
            }
        }
    }

    /// Whether `witness` nominally declares `required`: a genus naming it in
    /// its `implet` clause, or an enclosing type parameter bounded by it.
    fn witness_declares(&self, witness: TypeId, required: &ContractRef) -> bool {
        let (_, witness) = self.types.peel_access_marker(witness);
        let declared = if let Type::Param(sym) = self.types.get(witness) {
            self.current_generic_bounds.get(sym)
        } else {
            self.struct_def_from_type(witness)
                .and_then(|def| self.implemented_contracts.get(&def))
        };
        declared.is_some_and(|contracts| {
            contracts.iter().any(|contract| {
                contract.contract == required.contract
                    && contract.args.len() == required.args.len()
                    && contract
                        .args
                        .iter()
                        .zip(&required.args)
                        .all(|(have, want)| {
                            self.types.equals(
                                self.resolve_inferred_type(*have),
                                self.resolve_inferred_type(*want),
                            )
                        })
            })
        })
    }

    fn contract_display_name(&self, contract: radix_hir::DefId) -> String {
        self.function_display_name(contract)
            .unwrap_or_else(|| format!("Def{}", contract.0))
    }

    /// `Orderable<Persona>` spelling for diagnostics.
    fn contract_display(&mut self, contract: &ContractRef) -> String {
        let base = self.types.intern(Type::Interface(contract.contract));
        if contract.args.is_empty() {
            return self.format_union_constituent_type(base);
        }
        let applied = self.types.intern(Type::Applied(
            base,
            contract
                .args
                .iter()
                .copied()
                .map(AppliedArg::Type)
                .collect(),
        ));
        self.format_union_constituent_type(applied)
    }
}
