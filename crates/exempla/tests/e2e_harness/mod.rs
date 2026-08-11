//! Slow exempla end-to-end harness modules (integration test binary only).
//!
//! Backend lane modules are `#[cfg(feature = "<lane>")]`-gated on their radix
//! target feature (per-lane-e2e-validation decision 3): `cargo test -p exempla
//! --no-default-features --features <lane> --test e2e_harness -- --ignored`
//! builds only that lane. Shared helpers (`common`, `types`, `oracle`) stay
//! ungated. `roundtrip` is the no-backend minimal lane (decision 2) and
//! compiles in the default build only.

#![allow(dead_code)]

#[path = "../../src/exempla_e2e/common.rs"]
pub(crate) mod common;
#[path = "../../src/exempla_e2e/types.rs"]
pub(crate) mod types;

// Per-lane expected-outcome tables (EL-4), shared with the lib test tree:
// each lane harness module below consumes only its own lane's table via
// `super::expectations::<lane>::…`.
#[path = "../../src/exempla_e2e/expectations/mod.rs"]
pub(crate) mod expectations;

#[cfg(feature = "hir-go")]
#[path = "../../src/exempla_e2e/go.rs"]
pub mod go;
#[cfg(feature = "mir-llvm")]
#[path = "../../src/exempla_e2e/llvm.rs"]
pub mod llvm;
#[cfg(feature = "mir-llvm")]
#[cfg(feature = "hir-rust")]
#[path = "../../src/exempla_e2e/llvm_host.rs"]
pub mod llvm_host;
#[cfg(feature = "mir-llvm")]
#[path = "../../src/exempla_e2e/llvm_runtime.rs"]
pub(crate) mod llvm_runtime;
#[path = "../../src/exempla_e2e/oracle.rs"]
pub(crate) mod oracle;
#[cfg(feature = "default")]
#[path = "../../src/exempla_e2e/roundtrip.rs"]
pub mod roundtrip;
#[cfg(feature = "hir-rust")]
#[path = "../../src/exempla_e2e/rust.rs"]
pub mod rust;
#[cfg(feature = "hir-rust")]
#[path = "../../src/exempla_e2e/rust_canonical.rs"]
pub mod rust_canonical;
#[cfg(feature = "mir-sexp")]
#[path = "../../src/exempla_e2e/sexp.rs"]
pub mod sexp;
#[cfg(feature = "hir-swift")]
#[path = "../../src/exempla_e2e/swift.rs"]
pub mod swift;
#[cfg(feature = "hir-ts")]
#[path = "../../src/exempla_e2e/ts.rs"]
pub mod ts;
#[cfg(feature = "mir-wasm")]
#[path = "../../src/exempla_e2e/wasm.rs"]
pub mod wasm;
#[cfg(feature = "mir-wasm")]
#[path = "../../src/exempla_e2e/wasm_behavior_fixtures.rs"]
pub(crate) mod wasm_behavior_fixtures;
#[cfg(feature = "mir-wasm")]
#[path = "../../src/exempla_e2e/wasm_external.rs"]
pub(crate) mod wasm_external;
#[cfg(feature = "mir-wasm")]
#[path = "../../src/exempla_e2e/wasm_ledger.rs"]
pub(crate) mod wasm_ledger;
#[cfg(feature = "mir-wasm")]
#[path = "../../src/exempla_e2e/wasm_package.rs"]
pub(crate) mod wasm_package;
#[cfg(feature = "mir-wasm")]
#[path = "../../src/exempla_e2e/wasm_product.rs"]
pub(crate) mod wasm_product;
