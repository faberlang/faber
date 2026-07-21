//! Fast exempla harness surface: shared helpers, parity fixtures, MIR matrix.
//!
//! Slow backend corpus harnesses live in `tests/e2e_harness.rs`.

#![allow(dead_code)]

mod common;
mod conversio_target_matrix;
mod gpu_workload;
mod hir_target_matrix;
mod llvm_runtime;
mod mir;
mod mir_target_matrix;
mod oracle;
mod parity;
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
