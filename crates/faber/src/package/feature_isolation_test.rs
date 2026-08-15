//! Small-build feature-isolation proof (DDPP1-U2, C2 gate) + default-build
//! faber-runtime-free regression (S3-U2 cut).
//!
//! This test module compiles and runs in every build **without**
//! `device-runtime`:
//!   - the C2 small build (`--no-default-features --features hir-rust`), and
//!   - the default build after the S3-U2 cut (`device-runtime` and
//!     `host-macos-arm64` left `full-targets`, so the ordinary product graph
//!     has no `faber-runtime` edge — gate bullet 1).
//!
//! Its existence is the compile-level proof that the route-selection plumbing
//! carries its own selection type and that the packaged `faber-runtime` /
//! Hosts leaves / GPU emitters are excluded from the build it runs in: the
//! module cannot reference any of them because they are not compiled.

use crate::package::DeviceSelection;

/// C2 small-build proof. Runs only outside `full-targets` (the
/// `--no-default-features --features hir-rust` build): the GPU emitters and
/// the wasm host leaf are also excluded there. The default build's
/// exclusions are asserted by [`default_build_is_faber_runtime_free`]
/// instead.
#[cfg(not(feature = "full-targets"))]
#[test]
fn small_build_compiles_no_device_runtime_surface() {
    // The five gated dependency classes are all off in the small build.
    assert!(!cfg!(feature = "device-runtime"));
    assert!(!cfg!(feature = "mir-metal"));
    assert!(!cfg!(feature = "mir-llvm"));
    assert!(!cfg!(feature = "host-macos-arm64"));
    assert!(!cfg!(feature = "host-wasm"));
}

/// S3-U2 default-build regression: under the default build the cut holds —
/// `device-runtime`, `host-macos-arm64`, and `host-wasm` are off (no
/// `faber-runtime` edge, no native host leaf, no wasm host leaf) while the
/// LLVM emit surface stays compiled.
///
/// The faber-runtime-free graph claim is compile-level: the `faber-runtime`
/// package enters this crate only through the optional dep behind
/// `device-runtime` (`crates/faber/Cargo.toml:40`, `:111`), so this module
/// compiling and running in the default build without that feature is the
/// in-test proof; the transitive proof is `cargo tree -p faber -e normal`
/// (unit validation), which must show no `faber-runtime`, no
/// `faber-host-macos-arm64`, and no `faber-host-wasm` (host-wasm left
/// full-targets at the U2 dependency-removal cut — it stays an explicit
/// opt-in feature).
#[cfg(feature = "full-targets")]
#[test]
fn default_build_is_faber_runtime_free() {
    assert!(!cfg!(feature = "device-runtime"));
    assert!(!cfg!(feature = "host-macos-arm64"));
    // host-wasm left full-targets at the U2 cut (wasm execution is external
    // tools now); the LLVM emit surface stays default.
    assert!(!cfg!(feature = "host-wasm"));
    assert!(cfg!(feature = "mir-llvm"));
}

#[test]
fn route_selection_spellings_parse_without_runtime() {
    // Manifest `[device] backend` spellings parse against the local selection
    // type; unknown spellings fail closed (never silently guessed), matching
    // the packaged runtime's spelling surface.
    assert_eq!(
        DeviceSelection::from_spelling("auto"),
        Some(DeviceSelection::Auto)
    );
    assert_eq!(
        DeviceSelection::from_spelling("metal"),
        Some(DeviceSelection::Metal)
    );
    assert_eq!(
        DeviceSelection::from_spelling("cuda"),
        Some(DeviceSelection::Cuda)
    );
    assert_eq!(DeviceSelection::from_spelling("nope"), None);
}
