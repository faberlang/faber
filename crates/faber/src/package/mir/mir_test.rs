//! Faber-side remainder of the package-MIR tests (RP5).
//!
//! The compiler-owned tests moved to `radix-program/src/mir/mir_test.rs`
//! (constants, FNV-1a, manifest helpers, CLI planning, requirements, manifest
//! validation, MIR append/merge). These tests stay faber-side because they
//! exercise faber product surfaces the compiler subset never sees: the
//! generated fmir-bin runner crate (`bin_runner.rs`) and the `[device]`
//! payload (the `MirSeams::device_section_builder` seam body).

use super::bin_runner::{
    make_fmir_bin_entrypoint_executable, render_fmir_bin_runner_main_rs, toml_basic_string_path,
};
use super::*;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

// ── Constant / configuration tests ──────────────────────────────────────────

#[test]
fn fmir_bin_entrypoint_and_runner_names_are_configured() {
    assert_eq!(radix_program::mir::FMIR_BIN_ENTRYPOINT_FILE, "run");
    assert_eq!(FMIR_BIN_RUNNER_CRATE_DIR, "runner");
    assert_eq!(FMIR_BIN_RUNNER_PACKAGE_NAME, "faber-fmir-bin-runner");
}

#[test]
fn fmir_bin_runner_source_fingerprints_embedded_image() {
    let first = render_fmir_bin_runner_main_rs(b"first image");
    let second = render_fmir_bin_runner_main_rs(b"second image");

    assert_ne!(
        first, second,
        "shared cargo target builds must see image changes as source changes"
    );
    assert!(first.contains("embedded image fnv64:"));
    assert!(first.contains("include_bytes!(\"../../image.fmir\")"));
}

// ── toml_basic_string_path ──────────────────────────────────────────────────

#[test]
fn toml_basic_string_path_passes_through_simple_path() {
    let path = Path::new("src/main.fab");
    assert_eq!(toml_basic_string_path(path), "src/main.fab");
}

#[test]
fn toml_basic_string_path_escapes_backslashes() {
    let path = Path::new("windows\\path\\file.fab");
    assert_eq!(toml_basic_string_path(path), "windows\\\\path\\\\file.fab");
}

#[test]
fn toml_basic_string_path_escapes_double_quotes() {
    let path = Path::new("path/with\"quote");
    assert_eq!(toml_basic_string_path(path), "path/with\\\"quote");
}

// ── make_fmir_bin_entrypoint_executable (unix) ─────────────────────────────

#[cfg(unix)]
#[test]
fn make_fmir_bin_entrypoint_executable_sets_755_permissions() {
    let dir = std::env::temp_dir().join(format!(
        "faber-mir-entrypoint-test-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let entrypoint = dir.join("run");
    std::fs::write(&entrypoint, "#!/bin/sh\necho hello").expect("write entrypoint");

    let result = make_fmir_bin_entrypoint_executable(&entrypoint, &dir);
    assert!(
        result.is_ok(),
        "make_fmir_bin_entrypoint_executable should succeed"
    );

    let metadata = std::fs::metadata(&entrypoint).expect("read metadata");
    let mode = metadata.permissions().mode();
    assert_eq!(mode & 0o777, 0o755, "entrypoint should be executable (755)");

    let _ = std::fs::remove_dir_all(&dir);
}

// ── S6-U8: decomposed-companion reverse-AD seed provisioning ──────────────

/// Recursively copy a package directory, skipping `target/` build artifacts.
fn copy_package_dir_skipping_target(src: &Path, dest: &Path) {
    for entry in std::fs::read_dir(src).expect("read package dir") {
        let entry = entry.expect("read dir entry");
        let dest_path = dest.join(entry.file_name());
        if entry.path().is_dir() {
            if entry.file_name() == "target" {
                continue;
            }
            std::fs::create_dir_all(&dest_path).expect("create sub dir");
            copy_package_dir_skipping_target(&entry.path(), &dest_path);
        } else {
            std::fs::copy(entry.path(), dest_path).expect("copy file");
        }
    }
}

/// The S6-U8 reverse-AD upstream-seed regression (the bert-tiny oracle RUN
/// blocker): the tensor-return forward's companion carries a 1-element f32
/// upstream seed whose input buffer rides a DECOMPOSED companion subchain
/// kernel — the whole-function signature does not derive for a decomposed
/// kernel, so the old seed scan skipped those kernels entirely and never
/// provisioned the seed (the `RepeatingStep` once-init check then failed at
/// run time: `input_4` is not provided). The seed provisioning must cover
/// the decomposed kernels regardless of the signature scan: the image's
/// declared once-init inputs include the 1-element seed with 1.0.
// S3-U2 cut: this proof requires the device-section builder, which compiles
// only under `device-runtime` (now opt-in, not in `full-targets`). Gated to
// the device feature; exercised by the opt-in runner proof (S3-U4).
#[cfg(feature = "device-runtime")]
#[test]
fn package_fmir_image_provisions_decomposed_companion_upstream_seed() {
    use radix_mir_fmir::schema::{WireBufferLifetime, WireBufferRole};

    let workspace = radix_module::paths::faberlang_home().expect("faberlang container root");
    let exemplum = workspace.join("examples/training/bert-tiny-fragment");
    let dir = crate::package::test_support::test_temp_dir("bert-tiny-seed");
    copy_package_dir_skipping_target(&exemplum, dir.path());

    let config = radix_module::driver::Config::default()
        .with_stdlib(workspace)
        .with_target(radix_module::codegen::Target::MirFmirBinary);
    let image = radix_program::mir::build_package_fmir_image(
        &config,
        dir.path(),
        &[],
        &super::faber_mir_seams(),
    )
    .expect("the BERT-tiny image builds");
    let bytes = std::fs::read(&image.image_path).expect("read the image");
    let loaded = radix_program::mir::load_fmir_image(
        &bytes,
        &image.image_path,
        PACKAGE_MIR_TOOLCHAIN_VERSION,
    )
    .expect("the image loads");
    let device = loaded
        .device
        .as_ref()
        .expect("the image carries a device section");

    // The program carries the companion's 1-element PerProgram input (the
    // reverse-AD seed) — proof the seed reached the device program even
    // though its kernel is a decomposed subchain.
    let program = &device.device_program.program;
    let seed_slots: Vec<_> = program
        .kernels
        .iter()
        .flat_map(|kernel| kernel.resources.iter())
        .filter(|resource| {
            resource.version.element_count == 1
                && resource.buffer.lifetime == WireBufferLifetime::PerProgram
                && resource.buffer.role == WireBufferRole::Input
        })
        .collect();
    assert!(
        !seed_slots.is_empty(),
        "the companion's 1-element upstream seed rides the decomposed program"
    );

    // The once-init provisioning declares the seed with 1.0 — the host's
    // `RepeatingStep` once-init check receives the value and passes.
    let seed = device
        .declared_inputs
        .iter()
        .find(|input| input.values.len() == 1 && input.name.starts_with("input_"))
        .expect("the reverse-AD seed is provisioned at once-init");
    assert_eq!(seed.values, vec![1.0]);
}
