//! Per-lane expected-outcome tables (delivery EL-4).
//!
//! Every backend lane owns its expectation surface in exactly one module:
//!
//! | lane      | module                  | owned tables |
//! | ---       | ---                     | --- |
//! | go        | `expectations::go`      | `GO_EXPECTED_FAILURES`, runtime/compile/declaration tables, pass floors, failure ceiling |
//! | ts        | `expectations::ts`      | `TS_EXPECTED_OUTCOMES`, tier floors |
//! | wasm      | `expectations::wasm`    | `WASM_EXPECTED_TIER_FLOORS`, aggregate tier floors |
//! | rust      | `expectations::rust`    | `KNOWN_FAILURES`, ceiling (shared oracle classifications stay in `super::oracle`) |
//! | swift     | `expectations::swift`   | `SWIFT_EXPECTED_FAILURES`, compile-failure table, pass floors, ceiling |
//! | sexp      | `expectations::sexp`    | `SEXP_EXPECTED_FAILURES`, racket floors |
//! | llvm      | `expectations::llvm`    | tier floors, unsupported-diagnostic ceiling |
//! | mir       | `expectations::mir`     | MIR tier floors |
//! | roundtrip | `expectations::roundtrip` | `FABER_ROUNDTRIP_EXPECTED_FAILURES` |
//!
//! Ownership rule (the grep guard): a lane harness module consumes only its
//! own table via `super::expectations::<lane>::…`. The same corpus path may
//! legitimately appear in several lanes' tables — a fixture can fail on
//! several backends — but a row belongs to a lane only while the fixture
//! fails *on that lane*; a lane must never absorb another lane's ledger rows.

#[cfg(feature = "hir-go")]
pub(crate) mod go;
#[cfg(feature = "mir-llvm")]
pub(crate) mod llvm;
#[cfg(feature = "default")]
pub(crate) mod mir;
#[cfg(feature = "default")]
pub(crate) mod roundtrip;
#[cfg(feature = "hir-rust")]
pub(crate) mod rust;
#[cfg(feature = "mir-sexp")]
pub(crate) mod sexp;
#[cfg(feature = "hir-swift")]
pub(crate) mod swift;
#[cfg(feature = "hir-ts")]
pub(crate) mod ts;
#[cfg(feature = "mir-wasm")]
pub(crate) mod wasm;
