//! Shared MIR device roles and device-safe lowerability gate.
//!
//! TARGET: device-facing MIR consumers (`metal-text`, future `wgsl-text`,
//! NVVM/PTX, and LLVM's device-entry proof comments).
//!
//! WHY: device roles were first extracted inside `llvm_text`, while `metal_text`
//! carried a duplicate role context and local rejection policy. MIR GPU Stage 1
//! hoists the role context and adds one conservative pre-emission gate so
//! device emitters reject host-only MIR shapes before target-specific printing.

pub use crate::device_semantics::{
    LosslessMirCompanionEntry, MirCompanionSelectedInput, MirCompanionSelectedOutput,
};

mod companion;
mod roles;
mod safe;

pub use companion::*;
pub use roles::*;
pub use safe::*;

#[cfg(test)]
mod tests;
