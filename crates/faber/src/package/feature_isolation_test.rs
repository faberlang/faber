//! Small-build feature-isolation proof (DDPP1-U2, C2 gate).
//!
//! This test module compiles and runs only in builds **without**
//! `device-runtime`: the C2 small build
//! (`--no-default-features --features hir-rust`). `device-runtime` is part of
//! `full-targets` again (gpu-reset A2 closeout: the ordinary `faber run
//! --device metal` must dispatch), so the default build does not compile this
//! module; the default build's device-runtime row is asserted by
//! `commands::targets_test` and `cli_test`.
//!
//! Its existence is the compile-level proof that the route-selection plumbing
//! carries its own selection type and that the host leaf / GPU emitters are
//! excluded from the build it runs in: the module cannot reference any of
//! them because they are not compiled.

use radix::program::mir::DeviceSelection;

/// C2 small-build proof. Runs only outside `full-targets` (the
/// `--no-default-features --features hir-rust` build): the GPU emitters and
/// the wasm host leaf are also excluded there.
#[cfg(not(feature = "full-targets"))]
#[test]
fn small_build_compiles_no_device_runtime_surface() {
    // The gated dependency classes are all off in the small build.
    assert!(!cfg!(feature = "device-runtime"));
    assert!(!cfg!(feature = "mir-metal"));
    assert!(!cfg!(feature = "mir-llvm"));
}

#[test]
fn route_selection_spellings_parse_without_runtime() {
    // Manifest `[device] device` spellings parse against the local selection
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
