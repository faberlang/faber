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

use super::{InterfaceMethodInfo, TypeChecker};
use crate::SemanticErrorKind;

/// `implet` (on a genus or a bound) names something that is not an implendum.
pub(super) const ISSUE_IMPLET_REQUIRES_IMPLENDUM: &str = "implet_requires_implendum";

/// A contract is named with the wrong number of type arguments.
const ISSUE_IMPLET_CONTRACT_ARITY: &str = "implet_contract_arity";

/// A genus declares an `implet` contract but supplies no such method.
const ISSUE_IMPLET_MISSING_METHOD: &str = "implet_missing_method";

/// A genus's method for an `implet` contract does not match its signature.
const ISSUE_IMPLET_METHOD_SIGNATURE_MISMATCH: &str = "implet_method_signature_mismatch";

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

    /// Check that a genus supplies every method of each declared contract,
    /// with a matching signature (CTO ruling Q-T1, 2026-10-15).
    ///
    /// `contracts` has already passed [`Self::validate_contract_ref`], so a
    /// target that is not an implendum or carries the wrong arity is absent.
    pub(super) fn check_impled_conformance(
        &mut self,
        def_id: radix_hir::DefId,
        span: radix_lexer::Span,
        contracts: &[radix_types::ContractRef],
    ) {
        let genus = self
            .function_display_name(def_id)
            .unwrap_or_else(|| format!("Def{}", def_id.0));
        for contract in contracts {
            let Some(interface) = self.interfaces.get(&contract.contract) else {
                continue;
            };
            let mut rows: Vec<(Symbol, InterfaceMethodInfo)> = interface
                .iter()
                .map(|(name, info)| (*name, info.clone()))
                .collect();
            rows.sort_by_key(|(_, info)| (info.span.start, info.span.end));
            let vars: FxHashMap<Symbol, TypeId> = self
                .type_params_by_def
                .get(&contract.contract)
                .map(|params| {
                    params
                        .iter()
                        .copied()
                        .zip(contract.args.iter().copied())
                        .collect()
                })
                .unwrap_or_default();
            let contract_name = self.contract_display(contract);
            for (name, info) in rows {
                let expected = self.substitute_func_sig(&info.sig, &mut vars.clone(), None);
                let method = self.interner.map_or_else(
                    || "?".to_owned(),
                    |interner| interner.resolve(name).to_owned(),
                );
                let found = self
                    .structs
                    .get(&def_id)
                    .and_then(|struct_info| struct_info.methods.get(&name))
                    .cloned();
                match found {
                    None => {
                        self.error_issue_with_args(
                            SemanticErrorKind::TypeMismatch,
                            ISSUE_IMPLET_MISSING_METHOD,
                            span,
                            &[
                                ("genus", genus.clone()),
                                ("contract", contract_name.clone()),
                                ("method", method),
                            ],
                        );
                    }
                    Some(found) => {
                        if self.impled_signatures_match(&expected, &found) {
                            continue;
                        }
                        let expected_text = self.format_func_sig(&expected);
                        let found_text = self.format_func_sig(&found);
                        self.error_issue_with_args(
                            SemanticErrorKind::TypeMismatch,
                            ISSUE_IMPLET_METHOD_SIGNATURE_MISMATCH,
                            span,
                            &[
                                ("genus", genus.clone()),
                                ("contract", contract_name.clone()),
                                ("method", method),
                                ("expected", expected_text),
                                ("found", found_text),
                            ],
                        );
                    }
                }
            }
        }
    }

    /// Whether a genus method satisfies a contract method (CTO ruling Q-T1).
    ///
    /// Compares parameter count and per-parameter type/posture/optionality/
    /// sponte, generic-parameter count, return and alternate-exit types, and
    /// the async/generator markers. The rejected `func_sigs_agree` in
    /// `call.rs` omits `sponte` and `type_params.len()`; this is the full list.
    fn impled_signatures_match(&self, expected: &FuncSig, found: &FuncSig) -> bool {
        if expected.params.len() != found.params.len()
            || expected.type_params.len() != found.type_params.len()
            || expected.is_async != found.is_async
            || expected.is_generator != found.is_generator
        {
            return false;
        }
        match (expected.err, found.err) {
            (None, None) => {}
            (Some(expected_err), Some(found_err)) => {
                if !self.types.equals(
                    self.resolve_inferred_type(expected_err),
                    self.resolve_inferred_type(found_err),
                ) {
                    return false;
                }
            }
            _ => return false,
        }
        if !self.types.equals(
            self.resolve_inferred_type(expected.ret),
            self.resolve_inferred_type(found.ret),
        ) {
            return false;
        }
        expected
            .params
            .iter()
            .zip(found.params.iter())
            .all(|(expected_param, found_param)| {
                expected_param.mode == found_param.mode
                    && expected_param.optional == found_param.optional
                    && expected_param.sponte == found_param.sponte
                    && self.types.equals(
                        self.resolve_inferred_type(expected_param.ty),
                        self.resolve_inferred_type(found_param.ty),
                    )
            })
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
