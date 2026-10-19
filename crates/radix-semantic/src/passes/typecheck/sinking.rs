//! The sinking contract: `Sink<T>`.
//!
//! Norma declares `implendum Sink<T> { functio sink(T res) → vacuum ⇥ E;
//! functio flush() → vacuum ⇥ E }` under `@ radix contract "sinking"`.
//! `s ⇜ x` is the operator form of `s.sink(x)` and dies on a failed write;
//! `s.sink(x)` reports. This module owns recognition only; the glyph's typing
//! rules come with the sink-into check (unit 0c4d3dc1-4).

use radix_hir::{DefId, HirInterface, HirInterfaceMethod};
use radix_lexer::Span;
use radix_types::{Primitive, Type};

use super::TypeChecker;
use crate::radix_contract::{
    RadixContractRole, SINKING_FLUSH_METHOD, SINKING_METHOD, contract_error,
};

/// `@ radix contract "sinking"` on an implendum whose shape is not
/// `Name<T> { sink(T) → vacuum ⇥ E; flush() → vacuum ⇥ E }`.
const ISSUE_SINKING_CONTRACT_SHAPE: &str = "radix_contract_sinking_shape";

/// A second implendum claims the sinking role in one unit.
const ISSUE_SINKING_CONTRACT_DUPLICATE: &str = "radix_contract_duplicate";

impl TypeChecker<'_> {
    /// Record a local implendum carrying `@ radix contract "sinking"`.
    pub(super) fn register_local_sinking_contract(
        &mut self,
        def_id: DefId,
        interface: &HirInterface,
        span: Span,
    ) {
        if !self.sinking_contract_shape_ok(interface) {
            self.errors
                .push(contract_error(span, ISSUE_SINKING_CONTRACT_SHAPE));
            return;
        }
        self.claim_sinking_contract(def_id, span);
    }

    /// Record an imported implendum whose file interface names the role.
    pub(super) fn register_imported_sinking_contract(&mut self, def_id: DefId) {
        if self
            .resolver
            .imported_contract_role(def_id)
            .and_then(RadixContractRole::parse)
            == Some(RadixContractRole::Sinking)
        {
            self.claim_sinking_contract(def_id, Span::default());
        }
    }

    fn claim_sinking_contract(&mut self, def_id: DefId, span: Span) {
        match self.types.sinking_contract {
            Some(existing) if existing != def_id => {
                self.errors
                    .push(contract_error(span, ISSUE_SINKING_CONTRACT_DUPLICATE));
            }
            _ => self.types.sinking_contract = Some(def_id),
        }
    }

    /// `Name<T> { sink(T) → vacuum ⇥ E; flush() → vacuum ⇥ E }`, nothing else.
    fn sinking_contract_shape_ok(&self, interface: &HirInterface) -> bool {
        let ([param], methods) = (
            interface.type_params.as_slice(),
            interface.methods.as_slice(),
        ) else {
            return false;
        };
        let is_sink = |method: &HirInterfaceMethod| {
            self.interner
                .is_some_and(|interner| interner.resolve(method.name) == SINKING_METHOD)
                && matches!(
                    method.params.as_slice(),
                    [item] if matches!(self.types.get(item.ty), Type::Param(sym) if *sym == param.name)
                )
                && self.returns_vacuum_with_err(method)
        };
        let is_flush = |method: &HirInterfaceMethod| {
            self.interner
                .is_some_and(|interner| interner.resolve(method.name) == SINKING_FLUSH_METHOD)
                && method.params.is_empty()
                && self.returns_vacuum_with_err(method)
        };
        let [first, second] = methods else {
            return false;
        };
        (is_sink(first) && is_flush(second)) || (is_flush(first) && is_sink(second))
    }

    /// `→ vacuum ⇥ E`: the error identity is the contract's own choice.
    fn returns_vacuum_with_err(&self, method: &HirInterfaceMethod) -> bool {
        method
            .ret_ty
            .is_some_and(|ret| matches!(self.types.get(ret), Type::Primitive(Primitive::Vacuum)))
            && method.err_ty.is_some()
    }
}
