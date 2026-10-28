//! Typed device-program plan mapping and launch descriptors.
//!
//! This module is the fail-closed `MirCollectionOp → CollectionKernelPlan`
//! bridge: it maps a collection intrinsic plus the resolved dispatch contract
//! onto the closed recipe set, re-derives that plan from a kernel function's
//! typed facts, and builds backend-neutral launch descriptors from a
//! [`DeviceProgram`].
//!
//! Nothing here reads emitted text. An op with no recipe is a typed
//! [`PlanMappingError`], never a silent [`CollectionKernelPlan::Elementwise`]
//! floor. Mixed-recipe bodies belong to
//! [`crate::kernel_decomposition::decompose_kernel_function`].

use crate::abi::MirAbiError;
use std::fmt;

mod carried_facts;
mod launch;
mod mapping;
mod recipes;
mod subchain_resolution;
mod subchain_signatures;

pub use self::carried_facts::*;
pub use self::launch::*;
pub use self::mapping::*;
pub use self::recipes::*;
pub use self::subchain_resolution::*;
pub use self::subchain_signatures::*;

/// A typed plan-mapping failure (contract resolution, operand shapes, or a
/// recipe mismatch). Carries a stable message; the emitters map it onto their
/// own fail-closed diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanMappingError {
    /// Human-readable failure detail.
    pub message: String,
}

impl PlanMappingError {
    pub(crate) fn mapping(shape: impl Into<String>) -> Self {
        Self {
            message: shape.into(),
        }
    }
}

impl fmt::Display for PlanMappingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for PlanMappingError {}

impl From<MirAbiError> for PlanMappingError {
    fn from(error: MirAbiError) -> Self {
        Self::mapping(error.message)
    }
}

#[cfg(test)]
#[path = "device_program_plans_test.rs"]
pub(crate) mod tests;
