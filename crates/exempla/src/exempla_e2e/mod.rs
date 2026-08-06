//! Fast exempla harness surface: shared helpers, parity fixtures, MIR matrix.
//!
//! Slow backend corpus harnesses live in `tests/e2e_harness.rs`.

#![allow(dead_code)]

mod cli_parity;
mod common;
mod conversio_target_matrix;
mod gpu_workload;
mod hir_target_matrix;
// Shared LLVM-host classification + Stage 8 builder-parity proof. The module
// is included here (lib test tree) so the S8.6/S8.7 ratchet proof runs under
// `cargo test -p exempla --lib` while the slow e2e_harness integration binary
// carries the full pairwise corpus run.
mod llvm;
mod llvm_runtime;
mod mir;
mod mir_target_matrix;
mod oracle;
mod parity;
mod rust;
mod script;
mod tensor_package;
mod tensor_systems_closeout;
mod tensor_workload_proof;
mod types;
mod wasm_behavior_fixtures;
mod wasm_external;

#[cfg(test)]
#[path = "gpu_workload_test.rs"]
mod gpu_workload_tests;

#[cfg(test)]
#[path = "script_test.rs"]
mod script_tests;
