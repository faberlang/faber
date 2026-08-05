//! MIR validation before inspection and backend experiments.
//!
//! Validation checks that MIR is well-formed relative to the semantic type
//! table and the auxiliary signature/field metadata supplied by the lowerer or
//! caller. It catches duplicate IDs, missing local/temp/block/value references,
//! invalid CFG targets, unresolved/error recovery types, call arity/type
//! mismatches, destination typing, aggregate shape mismatches, projection
//! metadata gaps, option-operation contracts, and runtime intrinsic policies.
//!
//! ERROR STRATEGY
//! ==============
//! Validation accumulates all errors it can find and returns them as one vector.
//! It never repairs MIR and it does not guess missing type metadata; a missing
//! field table or failable-call signature is a producer bug that should remain
//! visible to the caller.
//!
//! LIMITS
//! ======
//! This pass verifies MIR shape below typed HIR. It is not a full semantic
//! analyzer or borrow checker, and it currently tracks statement-defined
//! `MirValueId`s within each block rather than globally. CFG reachability and
//! termination are enforced by `validate_cfg`.
//!
//! Module layout
//! =============
//! * `error` — diagnostics and callable-signature metadata.
//! * `context` — semantic metadata plus enum-representation/payload analysis.
//! * `validator` — `Validator`, `FunctionScope`, `ValidatedMir`, `validate_program`.
//! * `stmt` — block/statement/terminator/CFG checks.
//! * `value` — value/option/operand/place and call-argument checks.
//! * `intrinsic` — runtime-call/intrinsic/gradient/sermo/collection/map/atomic checks.
//! * `aggregate` — aggregate shape checks.
//! * `type` — projection, assignability, and type checks.

mod aggregate;
mod context;
mod error;
mod intrinsic;
mod stmt;
#[path = "type.rs"]
mod type_checks;
mod validator;
mod value;

pub use context::*;
pub use error::*;
pub use validator::*;

// Internal items shared across the validation submodules (`use super::*`).
pub(crate) use validator::{
    signature_from_func_sig, signature_from_function, DuplicateFunctionFinder, FunctionScope,
    FunctionShape, GenericTypeBindings, Validator,
};

#[cfg(test)]
#[path = "../validate_test.rs"]
mod tests;
