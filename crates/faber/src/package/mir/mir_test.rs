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
use radix::module::diagnostics::Diagnostic;
use radix::program::mir::DeviceSelection;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

// ── Constant / configuration tests ──────────────────────────────────────────

#[test]
fn fmir_bin_entrypoint_and_runner_names_are_configured() {
    assert_eq!(radix::program::mir::FMIR_BIN_ENTRYPOINT_FILE, "run");
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

    let workspace = radix::module::paths::faberlang_home().expect("faberlang container root");
    let exemplum = workspace.join("examples/training/bert-tiny-fragment");
    let dir = crate::package::test_support::test_temp_dir("bert-tiny-seed");
    copy_package_dir_skipping_target(&exemplum, dir.path());

    let config = radix::module::driver::Config::default()
        .with_library_home(workspace)
        .with_target(radix::module::codegen::Target::MirFmirBinary);
    let image = radix::program::mir::build_package_fmir_image(
        &config,
        dir.path(),
        &[],
        &super::faber_mir_seams(),
    )
    .expect("the BERT-tiny image builds");
    let bytes = std::fs::read(&image.image_path).expect("read the image");
    let loaded = radix::program::mir::load_fmir_image(
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

// ── RSM-4: fail-closed stats + device-route refusal (seam hook) ──────────

/// The `(image, selection, stats_requested)` fact observed at the faber
/// `host_route` seam, recorded before the body's device decision.
struct SeamObservation {
    device_bearing: bool,
    selection: DeviceSelection,
    stats_requested: bool,
}

thread_local! {
    static SEAM_OBSERVATIONS: std::cell::RefCell<Vec<SeamObservation>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

fn spying_host_route(
    image: &radix::program::mir::FmirPackageImage,
    selection: DeviceSelection,
    stats_requested: bool,
) -> Result<Option<()>, Vec<Diagnostic>> {
    SEAM_OBSERVATIONS.with(|observations| {
        observations.borrow_mut().push(SeamObservation {
            device_bearing: image.device.is_some(),
            selection,
            stats_requested,
        })
    });
    host_route(image, selection, stats_requested)
}

/// The real faber seam bundle with a recording wrapper around the seam body:
/// the observation proves the transported fact, the forwarded body proves
/// the faber-owned refusal decision.
fn spying_mir_seams() -> radix::program::mir::MirSeams {
    let mut seams = faber_mir_seams();
    seams.host_route = spying_host_route;
    seams
}

fn take_seam_observations() -> Vec<SeamObservation> {
    SEAM_OBSERVATIONS.with(|observations| std::mem::take(&mut *observations.borrow_mut()))
}

fn write_stats_route_package(label: &str, source: &str) -> crate::package::test_support::TestDir {
    let dir = crate::package::test_support::test_temp_dir(label);
    std::fs::write(dir.path().join("main.fab"), source).expect("write entry");
    dir
}

#[test]
fn host_route_seam_refuses_stats_requested_before_device_route() {
    let dir = write_stats_route_package("seam-stats-device-refused", "main { print 1 }\n");
    let entry = dir.path().join("main.fab");
    let mut host = radix::module::mir::BufferHost::default();

    let err = radix::program::mir::run_package_mir_with_selection_and_stats(
        &radix::module::driver::Config::default(),
        &entry,
        DeviceSelection::Metal,
        true,
        &spying_mir_seams(),
        &mut host,
    )
    .expect_err("stats + a device route must refuse at the seam");

    assert!(
        err.iter()
            .any(|diag| diag.issue() == Some("run_stats_device_route_unsupported")),
        "the seam must refuse with the stats identity, not a device failure: {err:?}"
    );

    let observations = take_seam_observations();
    assert_eq!(observations.len(), 1, "exactly one seam observation");
    assert!(
        observations[0].stats_requested,
        "the explicit stats fact reaches the seam"
    );
    assert_eq!(observations[0].selection, DeviceSelection::Metal);
}

#[test]
fn host_route_seam_stats_on_cpu_auto_keeps_interpreter_and_collects() {
    let dir = write_stats_route_package(
        "seam-stats-cpu-collects",
        "fn labor() → int {\n    return 42\n}\n\nmain {\n    print labor()\n}\n",
    );
    let entry = dir.path().join("main.fab");
    let mut host = radix::module::mir::BufferHost::default();

    let outcome = radix::program::mir::run_package_mir_with_selection_and_stats(
        &radix::module::driver::Config::default(),
        &entry,
        DeviceSelection::Auto,
        true,
        &spying_mir_seams(),
        &mut host,
    )
    .expect("CPU/auto with stats stays on the interpreter");

    let observations = take_seam_observations();
    assert_eq!(observations.len(), 1, "exactly one seam observation");
    assert!(observations[0].stats_requested);
    assert!(
        !observations[0].device_bearing,
        "the CPU image carries no device route"
    );
    assert_eq!(observations[0].selection, DeviceSelection::Auto);

    assert_eq!(
        host.stdout_lines,
        vec!["42".to_owned()],
        "the interpreter ran the program"
    );
    assert!(
        outcome
            .stats
            .functions
            .iter()
            .any(|row| row.name == "labor" && row.calls == 1),
        "the meter collected the called function: {:?}",
        outcome.stats.functions
    );
    assert!(outcome.result.is_ok());
}

#[test]
fn host_route_seam_no_stats_callers_pass_false_and_keep_device_semantics() {
    let dir = write_stats_route_package("seam-no-stats-false", "main { print 1 }\n");
    let entry = dir.path().join("main.fab");

    // The unstatsed selection route is an existing caller: it passes the
    // explicit `false` and the CPU/auto route stays on the interpreter.
    let mut host = radix::module::mir::BufferHost::default();
    radix::program::mir::run_package_mir_with_selection(
        &radix::module::driver::Config::default(),
        &entry,
        DeviceSelection::Auto,
        &spying_mir_seams(),
        &mut host,
    )
    .expect("the unstatsed CPU/auto route keeps the interpreter");
    let observations = take_seam_observations();
    assert_eq!(observations.len(), 1);
    assert!(
        !observations[0].stats_requested,
        "unstatsed callers pass false"
    );
    assert_eq!(host.stdout_lines, vec!["1".to_owned()]);

    // The stats-capable route with an explicit `false` reproduces the
    // unstatsed device semantics: the fail-closed refusal keeps the
    // device identity, never the stats identity.
    let mut device_host = radix::module::mir::BufferHost::default();
    let err = radix::program::mir::run_package_mir_with_selection_and_stats(
        &radix::module::driver::Config::default(),
        &entry,
        DeviceSelection::Metal,
        false,
        &spying_mir_seams(),
        &mut device_host,
    )
    .expect_err("an explicit device selection fails closed (no `device-runtime` in this build)");
    assert!(
        !err.iter()
            .any(|diag| diag.issue() == Some("run_stats_device_route_unsupported")),
        "a false stats fact must not produce the stats refusal: {err:?}"
    );
    let observations = take_seam_observations();
    assert_eq!(observations.len(), 1);
    assert!(!observations[0].stats_requested);
    assert_eq!(observations[0].selection, DeviceSelection::Metal);
}
