//! Small-build feature-isolation proof (DDPP1-U2, C2 gate).
//!
//! This test module compiles and runs **only** in a build without
//! `device-runtime` (the `--no-default-features --features hir-rust` small
//! build). Its existence is the compile-level proof that the route-selection
//! plumbing carries its own selection type and that the packaged
//! `faber-runtime` / Hosts leaves / GPU emitters are excluded from that build:
//! the module cannot reference any of them because they are not compiled.

use crate::package::DeviceSelection;

#[test]
fn small_build_compiles_no_device_runtime_surface() {
    // The five gated dependency classes are all off in the small build.
    assert!(!cfg!(feature = "device-runtime"));
    assert!(!cfg!(feature = "mir-metal"));
    assert!(!cfg!(feature = "mir-llvm"));
    assert!(!cfg!(feature = "host-macos-arm64"));
    assert!(!cfg!(feature = "host-wasm"));
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
