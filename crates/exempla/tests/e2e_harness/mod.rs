//! Slow exempla end-to-end harness modules (integration test binary only).

#![allow(dead_code)]

#[path = "../../src/exempla_e2e/common.rs"]
pub(crate) mod common;
#[path = "../../src/exempla_e2e/types.rs"]
pub(crate) mod types;

#[path = "../../src/exempla_e2e/go.rs"]
pub mod go;
#[path = "../../src/exempla_e2e/llvm.rs"]
pub mod llvm;
#[path = "../../src/exempla_e2e/llvm_host.rs"]
pub mod llvm_host;
#[path = "../../src/exempla_e2e/llvm_runtime.rs"]
pub(crate) mod llvm_runtime;
#[path = "../../src/exempla_e2e/oracle.rs"]
pub(crate) mod oracle;
#[path = "../../src/exempla_e2e/roundtrip.rs"]
pub mod roundtrip;
#[path = "../../src/exempla_e2e/rust.rs"]
pub mod rust;
#[path = "../../src/exempla_e2e/rust_canonical.rs"]
pub mod rust_canonical;
#[path = "../../src/exempla_e2e/sexp.rs"]
pub mod sexp;
#[path = "../../src/exempla_e2e/swift.rs"]
pub mod swift;
#[path = "../../src/exempla_e2e/ts.rs"]
pub mod ts;
#[path = "../../src/exempla_e2e/wasm.rs"]
pub mod wasm;
#[path = "../../src/exempla_e2e/wasm_behavior_fixtures.rs"]
pub(crate) mod wasm_behavior_fixtures;
#[path = "../../src/exempla_e2e/wasm_expectations.rs"]
pub(crate) mod wasm_expectations;
#[path = "../../src/exempla_e2e/wasm_external.rs"]
pub(crate) mod wasm_external;
