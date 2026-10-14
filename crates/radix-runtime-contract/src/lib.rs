//! Radix-owned runtime contract: the shared definitions (error-message
//! constants, tensor shape math, scalar display helpers, and the `instans`
//! datetime type) that the compiler and the public `faber` crate must agree on.
//!
//! ## Authority
//!
//! Radix is the authority for this contract, mirroring the public `faber`
//! crate's `tensor`, `sparsa`, `display`, `instans`, and `frame` modules. The runtime copies are
//! expected to become generated/committed artifacts in a later phase (the same
//! direction already taken for the C host ABI in [`crate::abi`]). Until then the
//! two copies are pinned equivalent by tests.
//!
//! ## Scope boundary
//!
//! This module exists so the Radix compiler crate has **zero normal external
//! dependencies**. It does not change Layer 2: generated Faber programs still
//! link the public `faber` crate and call `faber::Valor`, `faber::Instans`, etc. at
//! runtime. Those are text emissions, not build-time imports.

/// Decimal carrier scale for `d64`: values are `i64 × 10⁻⁸` (D11.7).
pub const D64_SCALE: i64 = 100_000_000;

pub mod display;
pub mod display_tokens;
pub mod fault_codes;
pub mod frame;
pub mod instans;
pub mod sparsa;
pub mod tensor;
