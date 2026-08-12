//! Faber runtime package contract material.
//!
//! The faber runtime package is a standalone public crate: it must not depend
//! on the private Radix contract crate (`radix-runtime-contract`) or the
//! private Radix host-ABI crate. The contract items the runtime package needs
//! (tensor shape math + error text, sparsa error text, `FrameStatus`,
//! failable status codes, the cursor-stream ABI symbol) are committed here as
//! faithful copies of the Radix-owned authority. Radix remains the authority
//! for the values; the compiler-side copies in `radix-runtime-contract` /
//! `radix-host-abi` stay the sync source.

pub mod tensor;
pub mod sparsa;
pub mod frame;
pub mod abi;
