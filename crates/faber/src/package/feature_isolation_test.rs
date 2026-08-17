//! Small-build feature-isolation proof (DDPP1-U2, C2 gate) + default-build
//! public-runtime-free regression (S3-U2 cut).
//!
//! This test module compiles and runs in every build **without**
//! `device-runtime`:
//!   - the C2 small build (`--no-default-features --features hir-rust`), and
//!   - the default build after the S3-U2 cut (`device-runtime` and
//!     `host-macos-arm64` left `full-targets`, so the ordinary product graph
//!     has no public runtime edge — gate bullet 1).
//!
//! Its existence is the compile-level proof that the route-selection plumbing
//! carries its own selection type and that the host leaf / GPU emitters are
//! excluded from the build it runs in: the module cannot reference any of
//! them because they are not compiled.

use radix_program::mir::DeviceSelection;

/// C2 small-build proof. Runs only outside `full-targets` (the
/// `--no-default-features --features hir-rust` build): the GPU emitters and
/// the wasm host leaf are also excluded there. The default build's
/// exclusions are asserted by [`default_build_is_faber_runtime_free`]
/// instead.
#[cfg(not(feature = "full-targets"))]
#[test]
fn small_build_compiles_no_device_runtime_surface() {
    // The four gated dependency classes are all off in the small build.
    assert!(!cfg!(feature = "device-runtime"));
    assert!(!cfg!(feature = "mir-metal"));
    assert!(!cfg!(feature = "mir-llvm"));
    assert!(!cfg!(feature = "host-macos-arm64"));
}

/// S3-U2 default-build regression: under the default build the cut holds —
/// `device-runtime` and `host-macos-arm64` are off (no public runtime edge or
/// native host leaf) while the LLVM emit surface stays compiled.
#[cfg(feature = "full-targets")]
#[test]
fn default_build_has_no_public_runtime_or_host_leaf() {
    assert!(!cfg!(feature = "device-runtime"));
    assert!(!cfg!(feature = "host-macos-arm64"));
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
