use super::*;
use crate::cli::RunArgs;
use faber_cli::package::DeviceChoice;
use radix::module::diagnostics::DiagnosticArg;
use radix::module::mir::RunSourceError;
use radix::program::mir::DeviceSelection;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("faber-run-{label}-{nanos}"));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn resolve_route_selection(
    args: &RunArgs,
    input_path: &Path,
) -> Result<DeviceSelection, Box<radix::module::diagnostics::Diagnostic>> {
    resolve_route_options(args, input_path).map(|(selection, _)| selection)
}

fn run_args(path: PathBuf, device: Option<DeviceChoice>) -> RunArgs {
    RunArgs {
        path,
        library: crate::cli::LibraryHomeArgs::default(),
        target: None,
        locale: None,
        diagnostics_locale: None,
        device,
        threads: None,
        partitions: None,
        safe: false,
        stats: false,
        stats_file: None,
        shuffle_seed: None,
        debug: false,
        no_diagnostics: false,
        deny_warnings: false,
        deny: Vec::new(),
        args: Vec::new(),
    }
}

// ── interpret-path input classification ─────────────────────────────────

#[test]
fn single_fab_file_detection() {
    let fab = PathBuf::from("script.fab");
    assert!(is_single_fab_file(&fab) || !fab.exists());
    assert!(!is_single_fab_file(Path::new(".")));
}

#[test]
fn salve_munde_is_single_fab_file() {
    let fab =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/incipit/salve-munde.fab");
    assert!(is_single_fab_file(&fab));
}

#[test]
fn interpret_package_input_detects_directory_manifest_and_manifest_entry() {
    let dir = temp_dir("package-input");
    let src = dir.join("src");
    std::fs::create_dir_all(&src).expect("create src");
    std::fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "run-policy"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("write manifest");
    let entry = src.join("main.fab");
    std::fs::write(&entry, "incipit { nota 1 }").expect("write entry");

    assert!(is_package_interpret_input(&dir));
    assert!(is_package_interpret_input(&dir.join("faber.toml")));
    assert!(is_package_interpret_input(&entry));
}

#[test]
fn interpret_package_input_detects_manifestless_import_entry() {
    let dir = temp_dir("manifestless-package-input");
    let entry = dir.join("main.fab");
    std::fs::write(
        &entry,
        r#"import from "./thing" thing

main { print thing.label() }
"#,
    )
    .expect("write entry");

    assert!(is_package_interpret_input(&entry));
}

#[test]
fn interpret_package_input_leaves_kernel_imports_as_single_source() {
    let dir = temp_dir("kernel-script-input");
    for (name, import) in [
        ("module.fab", r#"import from "faber:processus" processus"#),
        ("glob.fab", r#"import from "faber:*" faber"#),
    ] {
        let entry = dir.join(name);
        std::fs::write(&entry, format!("{import}\n\nmain {{ print 1 }}\n")).expect("write entry");

        assert!(!is_package_interpret_input(&entry));
    }
}

#[test]
fn interpret_package_input_leaves_manifestless_file_as_single_source() {
    let dir = temp_dir("single-file-input");
    let entry = dir.join("script.fab");
    std::fs::write(&entry, "incipit { nota 1 }").expect("write entry");

    assert!(!is_package_interpret_input(&entry));
}

#[test]
fn nonexistent_path_is_not_single_fab_file() {
    assert!(!is_single_fab_file(Path::new("/nonexistent/path.fab")));
}

#[test]
fn is_package_interpret_input_rejects_nonexistent_path() {
    assert!(!is_package_interpret_input(Path::new("/nonexistent/dir")));
    assert!(!is_package_interpret_input(Path::new(
        "/nonexistent/file.fab"
    )));
}

#[test]
fn is_package_interpret_input_accepts_empty_directory_for_package_route() {
    let dir = temp_dir("empty-dir");
    assert!(is_package_interpret_input(&dir));
}

#[test]
fn is_package_interpret_input_accepts_directory_without_manifest_or_entry_for_package_route() {
    let dir = temp_dir("no-manifest");
    let src = dir.join("src");
    std::fs::create_dir_all(&src).expect("create src");
    // No faber.toml, no .fab files
    assert!(is_package_interpret_input(&dir));
}

// ── S1-5 device selection: precedence (CLI > manifest > auto) ────────────

fn write_manifest(dir: &Path, device_value: Option<&str>) {
    let device_line = match device_value {
        Some(device) => format!("device = \"{device}\"\n"),
        None => String::new(),
    };
    std::fs::write(
        dir.join("faber.toml"),
        format!(
            "[package]\nname = \"device-precedence\"\nversion = \"0.1.0\"\nedition = \"1\"\n\n[paths]\nentry = \"main.fab\"\n\n[build]\nkind = \"bin\"\n\n[device]\n{device_line}"
        ),
    )
    .expect("write manifest");
}

#[test]
fn route_selection_cli_flag_overrides_manifest() {
    let dir = temp_dir("device-cli-overrides-manifest");
    std::fs::write(dir.join("main.fab"), "incipit {\n}\n").expect("write entry");
    write_manifest(&dir, Some("metal"));
    let args = run_args(dir.clone(), Some(DeviceChoice::Auto));
    let selection = resolve_route_selection(&args, &dir).expect("resolves");
    assert_eq!(selection, DeviceSelection::Auto);
}

#[test]
fn route_selection_manifest_used_when_no_cli_flag() {
    let dir = temp_dir("device-manifest-used");
    std::fs::write(dir.join("main.fab"), "incipit {\n}\n").expect("write entry");
    write_manifest(&dir, Some("cuda"));
    let args = run_args(dir.clone(), None);
    let selection = resolve_route_selection(&args, &dir).expect("resolves");
    assert_eq!(selection, DeviceSelection::Cuda);
}

#[test]
fn route_selection_defaults_to_auto_without_manifest_key() {
    let dir = temp_dir("device-default-auto");
    std::fs::write(dir.join("main.fab"), "incipit {\n}\n").expect("write entry");
    write_manifest(&dir, None);
    let args = run_args(dir.clone(), None);
    let selection = resolve_route_selection(&args, &dir).expect("resolves");
    assert_eq!(selection, DeviceSelection::Auto);
}

#[test]
fn route_selection_defaults_to_auto_without_manifest() {
    let dir = temp_dir("device-no-manifest");
    std::fs::write(dir.join("main.fab"), "incipit {\n}\n").expect("write entry");
    let args = run_args(dir.clone(), None);
    let selection = resolve_route_selection(&args, &dir).expect("resolves");
    assert_eq!(selection, DeviceSelection::Auto);
}

#[test]
fn route_selection_rejects_invalid_manifest_device() {
    let dir = temp_dir("device-invalid-manifest");
    std::fs::write(dir.join("main.fab"), "incipit {\n}\n").expect("write entry");
    write_manifest(&dir, Some("rocm"));
    let args = run_args(dir.clone(), None);
    let err = resolve_route_selection(&args, &dir).expect_err("invalid value must fail closed");
    assert!(err.args.contains(&DiagnosticArg::new("device", "rocm")));
    assert!(
        err.args
            .contains(&DiagnosticArg::new("key", "device.device"))
    );
}

#[test]
fn route_selection_invalid_manifest_with_cli_flag_is_ignored() {
    // Precedence: the CLI flag wins, so an invalid manifest value never
    // blocks a package run that names the device explicitly.
    let dir = temp_dir("device-cli-overrides-invalid-manifest");
    std::fs::write(dir.join("main.fab"), "incipit {\n}\n").expect("write entry");
    write_manifest(&dir, Some("rocm"));
    let args = run_args(dir.clone(), Some(DeviceChoice::Auto));
    let selection = resolve_route_selection(&args, &dir).expect("CLI flag wins");
    assert_eq!(selection, DeviceSelection::Auto);
}

#[test]
fn route_selection_cpu_flag_selects_the_cpu_route() {
    let dir = temp_dir("device-cpu-cli");
    std::fs::write(dir.join("main.fab"), "incipit {\n}\n").expect("write entry");
    write_manifest(&dir, None);
    let args = run_args(dir.clone(), Some(DeviceChoice::Cpu));
    let selection = resolve_route_selection(&args, &dir).expect("cpu resolves");
    assert_eq!(selection, DeviceSelection::Cpu);
}

#[test]
fn route_selection_reference_flag_selects_the_reference_route() {
    let dir = temp_dir("device-reference-cli");
    std::fs::write(dir.join("main.fab"), "incipit {\n}\n").expect("write entry");
    write_manifest(&dir, None);
    let args = run_args(dir.clone(), Some(DeviceChoice::Reference));
    let selection = resolve_route_selection(&args, &dir).expect("reference resolves");
    assert_eq!(selection, DeviceSelection::Reference);
    // Same route semantics as `cpu` today.
    assert_eq!(
        selection.kernel_lowering(),
        DeviceSelection::Cpu.kernel_lowering()
    );
    assert_eq!(
        selection.kernel_policy(),
        DeviceSelection::Cpu.kernel_policy()
    );

    let manifest_dir = temp_dir("device-reference-manifest");
    std::fs::write(manifest_dir.join("main.fab"), "incipit {\n}\n").expect("write entry");
    write_manifest(&manifest_dir, Some("reference"));
    let manifest_args = run_args(manifest_dir.clone(), None);
    assert_eq!(
        resolve_route_selection(&manifest_args, &manifest_dir).expect("manifest reference"),
        DeviceSelection::Reference
    );
}

#[test]
fn route_selection_manifest_cpu_selects_the_cpu_route() {
    let dir = temp_dir("device-cpu-manifest");
    std::fs::write(dir.join("main.fab"), "incipit {\n}\n").expect("write entry");
    write_manifest(&dir, Some("cpu"));
    let args = run_args(dir.clone(), None);
    let selection = resolve_route_selection(&args, &dir).expect("manifest cpu resolves");
    assert_eq!(selection, DeviceSelection::Cpu);

    // The CLI flag still wins over a manifest `cpu`.
    let cuda_args = run_args(dir.clone(), Some(DeviceChoice::Cuda));
    assert_eq!(
        resolve_route_selection(&cuda_args, &dir).expect("CLI flag wins"),
        DeviceSelection::Cuda
    );
}

#[test]
fn route_selection_retired_backend_key_is_refused_naming_device() {
    let dir = temp_dir("device-retired-backend-key");
    std::fs::write(dir.join("main.fab"), "incipit {\n}\n").expect("write entry");
    std::fs::write(
        dir.join("faber.toml"),
        "[package]\nname = \"retired-key\"\nversion = \"0.1.0\"\nedition = \"1\"\n\n[paths]\nentry = \"main.fab\"\n\n[build]\nkind = \"bin\"\n\n[device]\nbackend = \"auto\"\n",
    )
    .expect("write manifest");
    let args = run_args(dir.clone(), None);
    let err = resolve_route_selection(&args, &dir).expect_err("retired key must fail closed");
    assert_eq!(err.issue(), Some("package_device_backend_renamed"));
    assert!(
        err.args
            .contains(&DiagnosticArg::new("replacement_key", "device")),
        "names the new key: {:?}",
        err.args
    );
}

#[test]
fn run_rejects_the_retired_backend_flag() {
    let dir = temp_dir("device-retired-backend-flag");
    std::fs::write(dir.join("main.fab"), "main { print 1 }\n").expect("write entry");
    let output = run_faber_binary(&dir, &["--backend", "metal"]);
    assert_eq!(output.status.code(), Some(64), "clap usage error");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unexpected argument '--backend'"),
        "unknown-argument error: {stderr}"
    );
}

// ── RSM-4: fail-closed stats + device-route refusal (CLI hook) ───────────

#[test]
fn route_selection_allows_stats_with_explicit_device() {
    // gpu-reset C2-U6c (D4): a dispatching run reports host rows and device
    // receipt data, so the stats request no longer refuses at route resolution.
    let dir = temp_dir("stats-device-allowed");
    std::fs::write(dir.join("main.fab"), "incipit {\n}\n").expect("write entry");
    write_manifest(&dir, Some("metal"));

    let mut args = run_args(dir.clone(), Some(DeviceChoice::Metal));
    args.stats = true;
    assert_eq!(
        resolve_route_selection(&args, &dir).expect("stats + metal resolves"),
        DeviceSelection::Metal
    );

    let mut file_args = run_args(dir.clone(), Some(DeviceChoice::Cuda));
    file_args.stats_file = Some(dir.join("allowed.json"));
    assert_eq!(
        resolve_route_selection(&file_args, &dir).expect("stats-file + cuda resolves"),
        DeviceSelection::Cuda
    );
}

#[test]
fn route_selection_allows_stats_with_auto_or_absent_device() {
    let dir = temp_dir("stats-auto-allowed");
    std::fs::write(dir.join("main.fab"), "incipit {\n}\n").expect("write entry");
    write_manifest(&dir, None);

    let mut auto_args = run_args(dir.clone(), Some(DeviceChoice::Auto));
    auto_args.stats = true;
    assert_eq!(
        resolve_route_selection(&auto_args, &dir).expect("auto stays on the CPU route"),
        DeviceSelection::Auto
    );

    let mut manifest_args = run_args(dir.clone(), None);
    manifest_args.stats = true;
    assert_eq!(
        resolve_route_selection(&manifest_args, &dir).expect("absent device resolves"),
        DeviceSelection::Auto
    );

    // `cpu` runs the interpreter the meter measures, so stats are allowed.
    let mut cpu_args = run_args(dir.clone(), Some(DeviceChoice::Cpu));
    cpu_args.stats = true;
    assert_eq!(
        resolve_route_selection(&cpu_args, &dir).expect("cpu keeps the interpreted route"),
        DeviceSelection::Cpu
    );

    // `reference` is the same interpreted route.
    let mut reference_args = run_args(dir.clone(), Some(DeviceChoice::Reference));
    reference_args.stats = true;
    assert_eq!(
        resolve_route_selection(&reference_args, &dir)
            .expect("reference keeps the interpreted route"),
        DeviceSelection::Reference
    );
}

// ── `--threads` worker-count route contract (need, CD-1) ────────

#[test]
fn run_threads_requires_the_cpu_route() {
    let dir = temp_dir("threads-requires-cpu");
    std::fs::write(dir.join("main.fab"), "incipit {\n}\n").expect("write entry");
    write_manifest(&dir, None);
    let mut args = run_args(dir.clone(), Some(DeviceChoice::Reference));
    args.threads = Some(2);
    let err =
        resolve_route_selection(&args, &dir).expect_err("threads with reference must fail closed");
    assert_eq!(err.issue(), Some("E_THREADS_REQUIRES_CPU"));
    assert!(
        err.args
            .contains(&DiagnosticArg::new("device", "reference"))
    );
}

#[test]
fn run_threads_passes_on_the_cpu_route() {
    let dir = temp_dir("threads-cpu-route");
    std::fs::write(dir.join("main.fab"), "incipit {\n}\n").expect("write entry");
    write_manifest(&dir, None);
    let mut args = run_args(dir.clone(), Some(DeviceChoice::Cpu));
    args.threads = Some(2);
    assert_eq!(
        resolve_route_options(&args, &dir).expect("route options resolve"),
        (DeviceSelection::Cpu, std::num::NonZeroU32::new(2))
    );
}

// ── `--partitions` route contract (rule 18 CPU spanning) ──────────────────

#[test]
fn run_partitions_requires_the_cpu_route() {
    let dir = temp_dir("partitions-requires-cpu");
    std::fs::write(dir.join("main.fab"), "incipit {\n}\n").expect("write entry");
    write_manifest(&dir, None);
    let mut args = run_args(dir.clone(), Some(DeviceChoice::Reference));
    args.partitions = Some(2);
    let err = resolve_route_selection(&args, &dir)
        .expect_err("partitions with reference must fail closed");
    assert_eq!(err.issue(), Some("E_PARTITIONS_REQUIRES_CPU"));
    assert!(
        err.args
            .contains(&DiagnosticArg::new("device", "reference"))
    );
}

#[test]
fn run_partitions_pass_on_the_cpu_route_and_shape_the_device() {
    let dir = temp_dir("partitions-cpu-route");
    std::fs::write(dir.join("main.fab"), "incipit {\n}\n").expect("write entry");
    write_manifest(&dir, None);
    let mut args = run_args(dir.clone(), Some(DeviceChoice::Cpu));
    assert_eq!(
        cpu_device_shape(&args, None),
        package::mir::CpuDeviceShape::default(),
        "no flags: the route default workers over one partition"
    );
    args.threads = Some(6);
    args.partitions = Some(3);
    assert_eq!(
        resolve_route_selection(&args, &dir).expect("partitions with cpu passes"),
        DeviceSelection::Cpu
    );
    // No manifest key here, so the resolved count is the flag's.
    let shape = cpu_device_shape(&args, std::num::NonZeroU32::new(6));
    assert_eq!(shape.workers, std::num::NonZeroU32::new(6));
    assert_eq!(shape.partitions.get(), 3);
}

#[test]
fn run_partitions_pass_on_the_cuda_route_and_still_refuse_auto() {
    let dir = temp_dir("partitions-cuda-route");
    std::fs::write(dir.join("main.fab"), "incipit {\n}\n").expect("write entry");
    write_manifest(&dir, None);
    let mut args = run_args(dir.clone(), Some(DeviceChoice::Cuda));
    args.partitions = Some(2);
    assert_eq!(
        resolve_route_selection(&args, &dir).expect("partitions with cuda passes"),
        DeviceSelection::Cuda
    );
    assert_eq!(cpu_device_shape(&args, None).partitions.get(), 2);
    // `auto` is not a spanning selection: the flag is refused, not ignored.
    let mut auto = run_args(dir.clone(), Some(DeviceChoice::Auto));
    auto.partitions = Some(2);
    let err = resolve_route_selection(&auto, &dir).expect_err("auto refuses --partitions");
    assert_eq!(err.issue(), Some("E_PARTITIONS_REQUIRES_CPU"));
}

#[test]
fn run_threads_clap_accepts_positive_and_rejects_zero() {
    let parsed = <crate::cli::Cli as clap::Parser>::try_parse_from([
        "faber",
        "run",
        "x.fab",
        "--threads",
        "3",
    ])
    .expect("parse --threads 3");
    let Some(crate::cli::Command::Run(args)) = parsed.command else {
        panic!("expected run subcommand");
    };
    assert_eq!(args.threads, Some(3));

    let error = <crate::cli::Cli as clap::Parser>::try_parse_from([
        "faber",
        "run",
        "x.fab",
        "--threads",
        "0",
    ])
    .expect_err("--threads 0 is out of range");
    assert_eq!(error.kind(), clap::error::ErrorKind::ValueValidation);
}

/// Drive the cargo-built `faber` binary (never PATH) for the process-level
/// refusal proofs: nonzero exit, naming diagnostic, and no stats artifact.
fn run_faber_binary(package: &Path, flags: &[&str]) -> std::process::Output {
    Command::new(crate::commands::format_test::faber_binary())
        .arg("run")
        .args(flags)
        .arg(package)
        .output()
        .expect("run faber binary")
}

fn assert_no_stats_artifact(output: &std::process::Output, stats_file: &Path) {
    assert!(
        !output
            .stdout
            .windows(crate::commands::run_stats::STATS_MARKER.len())
            .any(|window| window == crate::commands::run_stats::STATS_MARKER.as_bytes()),
        "refusal must emit no stdout stats"
    );
    assert!(
        !stats_file.exists(),
        "refusal must emit no stats file: {}",
        stats_file.display()
    );
}

/// D4 (gpu-reset C2): stats with an explicit device are no longer refused for
/// being stats. The binary either dispatches (hosts with an admitted device;
/// host rows plus the `device: dispatched (measured)` marker) or fails
/// closed for the device (no admitted device, or a build without
/// `device-runtime`); it never emits the retired stats refusal.
fn assert_stats_device_run_follows_d4(output: &std::process::Output, stats_file: &Path) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("run_stats_device_route_unsupported")
            && !stderr.contains("require the interpreted CPU route"),
        "the retired stats/device refusal must not appear: {stderr}"
    );
    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("device: dispatched (measured)") || stats_file.exists(),
            "a dispatching stats run must report the D4 marker: {stdout}"
        );
    } else {
        assert_no_stats_artifact(output, stats_file);
    }
}

#[test]
fn route_stats_cli_device_run_follows_d4_without_the_retired_refusal() {
    let dir = temp_dir("stats-cli-device-metal-cli-d4");
    std::fs::write(dir.join("main.fab"), "main { print 1 }\n").expect("write entry");

    let stats_file = dir.join("stats.json");
    let file_arg = stats_file.to_str().expect("UTF-8 stats path");
    let output = run_faber_binary(
        &dir,
        &["--stats", "--stats-file", file_arg, "--device", "metal"],
    );
    assert_stats_device_run_follows_d4(&output, &stats_file);
}

#[test]
fn route_stats_manifest_device_run_follows_d4_without_the_retired_refusal() {
    let dir = temp_dir("stats-manifest-metal-d4");
    std::fs::create_dir_all(dir.join("src")).expect("create src");
    std::fs::write(dir.join("src/main.fab"), "main { print 1 }\n").expect("write entry");
    std::fs::write(
        dir.join("faber.toml"),
        r#"[package]
name = "stats-manifest-metal"
version = "0.1.0"
edition = "2026"

[paths]
source = "src"
entry = "main.fab"

[build]
kind = "bin"

[device]
device = "metal"
"#,
    )
    .expect("write manifest");

    let stats_file = dir.join("stats.json");
    let file_arg = stats_file.to_str().expect("UTF-8 stats path");
    let output = run_faber_binary(&dir, &["--stats", "--stats-file", file_arg]);
    assert_stats_device_run_follows_d4(&output, &stats_file);
}

// ── the kernel safe switch (operator ruling 2026-10-28) ──────────────────

fn safe_package(label: &str, device_table: &str) -> PathBuf {
    let dir = temp_dir(label);
    std::fs::create_dir_all(dir.join("src")).expect("create src");
    std::fs::write(dir.join("src/main.fab"), "main { print 1 }\n").expect("write entry");
    std::fs::write(
        dir.join("faber.toml"),
        format!(
            "[package]\nname = \"safe-switch\"\nversion = \"0.1.0\"\nedition = \"2026\"\n\n\
             [paths]\nsource = \"src\"\nentry = \"main.fab\"\n\n[build]\nkind = \"bin\"\n\n{device_table}"
        ),
    )
    .expect("write manifest");
    dir
}

#[test]
fn the_safe_flag_lowers_kernels_to_the_default_tier() {
    let dir = safe_package("safe-flag", "");
    let unsafe_config = script_package_config(&dir, None, None, false)
        .expect("config")
        .0;
    assert!(!unsafe_config.deny_kernel_tiers);
    let safe_config = script_package_config(&dir, None, None, true)
        .expect("config")
        .0;
    assert!(safe_config.deny_kernel_tiers);
}

#[test]
fn the_manifest_device_safe_key_lowers_kernels_to_the_default_tier() {
    let on = safe_package("safe-manifest-on", "[device]\nsafe = true\n");
    let config = script_package_config(&on, None, None, false)
        .expect("config")
        .0;
    assert!(config.deny_kernel_tiers);

    let off = safe_package("safe-manifest-off", "[device]\nsafe = false\n");
    let config = script_package_config(&off, None, None, false)
        .expect("config")
        .0;
    assert!(!config.deny_kernel_tiers);
}

#[test]
fn the_safe_flag_can_only_lower_a_tier_never_raise_one() {
    // `safe = false` in the manifest does not undo `--safe`.
    let off = safe_package("safe-flag-over-manifest", "[device]\nsafe = false\n");
    let config = script_package_config(&off, None, None, true)
        .expect("config")
        .0;
    assert!(config.deny_kernel_tiers);
}

// ── run dispatch: the package-MIR runner with a selection ────────────────

#[test]
fn package_mir_run_with_selection_auto_stays_on_cpu_route() {
    let dir = temp_dir("selection-auto-cpu");
    let entry = dir.join("main.fab");
    std::fs::write(&entry, "main { print \"cpu\" }\n").expect("write entry");

    let mut host = radix::module::mir::BufferHost::default();
    radix::program::mir::run_package_mir_with_selection(
        &radix::module::driver::Config::default(),
        &entry,
        DeviceSelection::Auto,
        &faber_cli::package::mir::faber_mir_seams(),
        &mut host,
    )
    .expect("auto selection must keep the CPU route");

    assert_eq!(host.stdout_lines, vec!["cpu".to_owned()]);
    assert!(!dir.join("target/faber/Cargo.toml").exists());
}

// ── single-file runner ExitCode ──────────────────────────────────────────

fn run_single_file(path: &Path) -> Result<ExitCode, RunSourceError> {
    run_single_file_under(path, DeviceSelection::Auto, Vec::new()).0
}

fn run_single_file_under(
    path: &Path,
    selection: DeviceSelection,
    arguments: Vec<String>,
) -> (Result<ExitCode, RunSourceError>, Vec<String>) {
    let source = std::fs::read_to_string(path).expect("read single-file source");
    let config = script_package_config(path, None, None, false)
        .expect("single-file config")
        .0;
    let config = match selection.kernel_lowering() {
        radix::module::mir::KernelLowering::Host => config.with_host_kernels(),
        radix::module::mir::KernelLowering::Device => config,
    };
    let session = radix::module::driver::Session::new(config);
    let mut host = radix::module::mir::BufferHost::with_argumenta(arguments);
    let result = crate::script::run_with_session(
        &session,
        &path.display().to_string(),
        &source,
        &mut host,
        selection,
    );
    (result, host.stdout_lines)
}

#[test]
fn single_file_kernel_runs_under_selection() {
    let dir = temp_dir("single-file-kernel-selection");
    let entry = dir.join("softmax.fab");
    std::fs::write(
        &entry,
        r#"+++
locale = "en"
+++

@ kernel
@ public
fn sm<size R, size C>(tensor<f32, [R, C]> x) → tensor<f32, [R, C]> {
    return x.softmax()
}

@ cli "softmax-runtime"
@ operand int rows
@ operand int cols
main args args {
    let int rows ← args.rows
    let int cols ← args.cols
    const size R ← rows
    const size C ← cols
    mut tensor<f32, [R, C]> x ← tf32[R, C] {}
    mut tensor<f32, [R, C]> y ← sm(x)
    print y[[0, 0]]
}
"#,
    )
    .expect("write single-file kernel");

    for selection in [DeviceSelection::Cpu] {
        let (result, stdout) =
            run_single_file_under(&entry, selection, vec!["2".to_owned(), "8".to_owned()]);
        assert!(result.is_ok());
        assert!(stdout.join("\n").contains("0.125"));
    }
}

#[test]
fn single_file_run_process_code_propagates_runner_exit() {
    let dir = temp_dir("single-file-exit");
    let entry = dir.join("gate.fab");
    std::fs::write(
        &entry,
        r#"
import from "norma:process" process

main {
    process.exit_process(7)
}
"#,
    )
    .expect("write single-file gate");

    assert!(!is_package_interpret_input(&entry));
    let result = run_single_file(&entry);
    assert_eq!(single_file_run_process_code(result), 7);
}

#[test]
fn single_file_run_process_code_success_is_zero() {
    let dir = temp_dir("single-file-success");
    let entry = dir.join("ok.fab");
    std::fs::write(&entry, "main { print 1 }\n").expect("write single-file success");

    assert!(!is_package_interpret_input(&entry));
    let result = run_single_file(&entry);
    assert_eq!(single_file_run_process_code(result), 0);
}

#[test]
fn single_file_run_process_code_frontend_error_is_data_error() {
    let diagnostic = Diagnostic::error("unknown name")
        .with_phase(radix::module::diagnostics::DiagnosticPhase::Resolve);
    let error = RunSourceError::Frontend(vec![diagnostic]);
    assert_eq!(single_file_run_process_code(Err(error)), 65);
}

#[test]
fn single_file_run_process_code_unclassed_failure_is_one() {
    let error = RunSourceError::Frontend(Vec::new());
    assert_eq!(single_file_run_process_code(Err(error)), 1);
}

// ── single-file @ cli operand binding ────────────────────

/// The shared `@ cli` fixture both process-level tests run: a single file that
/// must route through the same CLI analysis package mode uses.
fn write_single_file_cli_tool(dir: &Path) -> PathBuf {
    let entry = dir.join("tool.fab");
    std::fs::write(
        &entry,
        "+++\nlocale = \"en\"\n+++\n\n@ cli \"greeter\"\n@ operand string who\nmain args args {\n  print args.who\n}\n",
    )
    .expect("write single-file @ cli tool");
    entry
}

/// The single-file run must bind `@ cli` operands through the shared package
/// CLI planning: `-- Anna` after the entry path reaches `args.who`.
#[test]
fn single_file_cli_run_binds_operands_after_double_dash() {
    let dir = temp_dir("single-file-cli");
    let entry = write_single_file_cli_tool(&dir);
    let entry = entry.to_str().expect("UTF-8 entry path");

    let output = Command::new(crate::commands::format_test::faber_binary())
        .args(["run", entry, "--", "Anna"])
        .output()
        .expect("run faber binary");

    assert!(
        output.status.success(),
        "single-file @ cli run must bind the operand: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "Anna\n");
}

/// A single-file `@ cli` run with no operand value must refuse, not silently
/// run with an unbound operand. The identity is asserted by code and issue
/// (reader-locale D7 renders the compact identity; message prose is in the
/// pack and `--no-diagnostics` preserves that compact form), naming the CLI
/// surface that shared package planning rejected.
#[test]
fn single_file_cli_run_missing_operand_refuses_by_name() {
    let dir = temp_dir("single-file-cli");
    let entry = write_single_file_cli_tool(&dir);
    let entry = entry.to_str().expect("UTF-8 entry path");

    let output = Command::new(crate::commands::format_test::faber_binary())
        .args(["run", entry])
        .output()
        .expect("run faber binary");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "a missing CLI operand must refuse, got stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    // ALLOW_DIAGNOSTIC_RENDER_TEXT: subprocess CLI stderr renderer contract.
    assert!(
        stderr.contains("PKG001") && stderr.contains("package_mir_cli_surface_unsupported"),
        "missing operand must refuse with the named CLI-surface diagnostic: {stderr}"
    );
}

// ── single-file run exit codes end to end ────────────────

/// A plain single-file run (`main exit N`, no `@ cli`) must apply the entry
/// exit code as the process status, with the body running first.
#[test]
fn single_file_exit_code_literal() {
    let dir = temp_dir("single-file-exit-literal");
    let entry = dir.join("gate.fab");
    std::fs::write(
        &entry,
        "+++\nlocale = \"en\"\n+++\n\nmain exit 3 { print \"hi\" }\n",
    )
    .expect("write single-file literal exit");
    let entry = entry.to_str().expect("UTF-8 entry path");

    let output = Command::new(crate::commands::format_test::faber_binary())
        .args(["run", entry])
        .output()
        .expect("run faber binary");

    assert_eq!(output.status.code(), Some(3));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "hi\n");
}

/// A single-file `@ cli` run whose entry exit is an expression (`main args a
/// exit code() + 1`) must evaluate the expression and apply it as the process
/// status, not drop it or fall back to the planned constant.
#[test]
fn single_file_exit_code_cli_expression() {
    let dir = temp_dir("single-file-exit-expr");
    let entry = dir.join("expr.fab");
    std::fs::write(
        &entry,
        "+++\nlocale = \"en\"\n+++\n\nfn code() \u{2192} int { return 5 }\n\n@ cli { name = \"x\" }\n\nmain args a exit code() + 1 {\n  print \"cli\"\n}\n",
    )
    .expect("write single-file CLI exit expression");
    let entry = entry.to_str().expect("UTF-8 entry path");

    let output = Command::new(crate::commands::format_test::faber_binary())
        .args(["run", entry])
        .output()
        .expect("run faber binary");

    assert_eq!(output.status.code(), Some(6));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "cli\n");
}

// ── source admission: record-element tensor ──────────────

/// A `tensor<Block, [n]>` source fixture must pass admission, lower, and run
/// on the reference runner: the host constructs the tensor of genus values,
/// indexes one element and reads its narrow scalar field and an element of
/// its fixed-array field. No GPU is needed.
#[test]
fn record_tensor_index_and_field_read_runs_in_reference() {
    let dir = temp_dir("record-tensor-reference");
    let entry = dir.join("blocks.fab");
    std::fs::write(
        &entry,
        r#"+++
locale = "en"
+++

@ public
class Block {
    let f16 scale
    let list<int, size 2> values
}

@ public
fn read_blocks(tensor<Block, [2]> blocks, mut tensor<f32, [2]> out) → void {
    for from blocks at [i] let b {
        out[[i]] ← (b.scale ↦ f32) + (b.values[0] ↦ f32)
    }
}

main {
    let list<Block> rows ← [Block { scale = 1.5 ↦ f16, values = [3, 4] ↦ list<int, size 2> }, Block { scale = 2.5 ↦ f16, values = [5, 6] ↦ list<int, size 2> }]
    mut tensor<Block, [2]> blocks ← rows ↦ tensor<Block, [2]>
    mut tensor<f32, [2]> out ← tf32[2] {}
    read_blocks(blocks, mut out)
    print out[[0]]
    print out[[1]]
}
"#,
    )
    .expect("write record-tensor fixture");

    let (result, stdout) = run_single_file_under(&entry, DeviceSelection::Reference, Vec::new());
    assert!(result.is_ok(), "reference run failed: {result:?}");
    assert_eq!(stdout, vec!["4.5".to_owned(), "7.5".to_owned()]);
}

#[test]
fn fmir_image_input_error_names_the_image_runner_for_binary_images() {
    let dir = temp_dir("fmir-image-input");
    let binary = dir.join("image.fmir");
    let text = dir.join("image.fmir.txt");
    let source = dir.join("main.fab");
    for path in [&binary, &text, &source] {
        std::fs::write(path, b"x").expect("write file");
    }
    let message = fmir_image_input_error(&binary).expect("binary image refused");
    assert!(message.contains("not a runnable input for `faber run`"));
    assert!(message.contains("faber __fmir-run"));
    let message = fmir_image_input_error(&text).expect("text image refused");
    assert!(message.contains("not a runnable input for `faber run`"));
    assert!(!message.contains("__fmir-run"));
    assert!(fmir_image_input_error(&source).is_none());
    assert!(fmir_image_input_error(&dir.join("missing.fmir")).is_none());
}
