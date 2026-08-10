//! Fast exempla harness surface: shared helpers, parity fixtures, MIR matrix.
//!
//! Slow backend corpus harnesses live in `tests/e2e_harness.rs`.
//!
//! Backend lane modules are `#[cfg(feature = "<lane>")]`-gated on their radix
//! target feature (per-lane-e2e-validation decision 3): a feature-limited
//! build compiles only its lane, so "feature absent" never masquerades as a
//! test failure. Shared helpers (`common`, `types`, `oracle`, `parity`) stay
//! ungated. `mir` / `mir_target_matrix` are the no-backend minimal lane
//! (decision 2) and compile in the default build only.

#![allow(dead_code)]

#[cfg(feature = "mir-llvm")]
mod cli_parity;
mod common;
mod conversio_target_matrix;
mod gpu_workload;
mod hir_target_matrix;
// Shared LLVM-host classification + Stage 8 builder-parity proof. The module
// is included here (lib test tree) so the S8.6/S8.7 ratchet proof runs under
// `cargo test -p exempla --lib` while the slow e2e_harness integration binary
// carries the full pairwise corpus run.
#[cfg(feature = "mir-llvm")]
mod llvm;
#[cfg(feature = "mir-llvm")]
mod llvm_runtime;
#[cfg(feature = "default")]
mod mir;
#[cfg(feature = "default")]
mod mir_target_matrix;
// Module-boundary full profile (emit-lane parity on generated consumers,
// wave 1 rust lane — see delivery MB-U4 / goal AC5). Opt-in ignored tests;
// nothing here runs in the default suite.
mod module_boundary;
mod oracle;
mod parity;
#[cfg(feature = "hir-rust")]
mod rust;
mod script;
mod tensor_package;
mod tensor_systems_closeout;
mod tensor_workload_proof;
mod types;
#[cfg(feature = "mir-wasm")]
mod wasm_behavior_fixtures;
#[cfg(feature = "mir-wasm")]
mod wasm_external;

#[cfg(test)]
#[path = "gpu_workload_test.rs"]
mod gpu_workload_tests;

#[cfg(test)]
#[path = "script_test.rs"]
mod script_tests;
