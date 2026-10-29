//! Validates MIR before interpretation or target lowering.
//!
//! Validation accumulates independent structural and type failures against the
//! caller-supplied type table and signature/field metadata. It never repairs
//! malformed MIR or invents missing producer facts.
//!
//! This pass is not a semantic analyzer or borrow checker.

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

// Sibling check modules import these flattened names via `use super::{...}`.
pub(crate) use validator::{
    DuplicateFunctionFinder, FunctionScope, FunctionShape, GenericTypeBindings, Validator,
    signature_from_func_sig, signature_from_function,
};

#[cfg(test)]
#[path = "../validate_test.rs"]
mod tests;

#[cfg(test)]
#[path = "../validate_ref_param_test.rs"]
mod ref_param_tests;

#[cfg(test)]
#[path = "../validate_detach_test.rs"]
mod detach_tests;

#[cfg(test)]
#[path = "intrinsic_test.rs"]
mod intrinsic_tests;
