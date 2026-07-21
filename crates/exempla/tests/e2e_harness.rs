//! Ignored exempla end-to-end harnesses (`cargo test -p exempla --test e2e_harness`).

// The included e2e modules are shared with the library test tree and refer to
// `crate::paths`; re-export the library path resolver at this integration
// crate root.
pub(crate) use exempla::paths;

#[path = "e2e_harness/mod.rs"]
mod harness;
