//! HIR→MIR lowering for the Faber compiler.
//!
//! Extracted from the monolith's `mir` module (goal `radix-crate-dag`, unit
//! C-M1). The lowering is a self-contained pass over an analyzed application
//! view: it reads HIR, semantic types, and MIR types, and it holds no driver
//! reference. Its input contract is
//! [`radix_semantic::AnalyzedModuleApplicationView`], which is what lets the
//! pass compile outside the monolith.
//!
//! The monolith keeps a shim at `radix_module::mir::lower` (and the fragment
//! modules) so existing paths keep resolving, and the companion tests stay in
//! `radix-module`.

pub mod fragment_composition;
pub mod fragment_instantiation;
pub mod lower;
