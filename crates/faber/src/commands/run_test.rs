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

fn run_args(path: PathBuf, device: Option<DeviceChoice>) -> RunArgs {
    RunArgs {
        path,
        locale: None,
        diagnostics_locale: None,
        device,
        stats: false,
        stats_file: None,
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
    assert_eq!(output.status.code(), Some(2), "clap usage error");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unexpected argument '--backend'"),
        "unknown-argument error: {stderr}"
    );
}

// ── RSM-4: fail-closed stats + device-route refusal (CLI hook) ───────────

#[test]
fn route_selection_allows_stats_with_explicit_device() {
    // gpu-reset C2-U6c (D4): a dispatching run reports host rows only, so the
    // stats request no longer refuses at route resolution.
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
/// host rows plus the `device: dispatched (not measured)` marker) or fails
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
            stdout.contains("device: dispatched (not measured)") || stats_file.exists(),
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
    let source = std::fs::read_to_string(path).expect("read single-file source");
    let config = script_package_config(path, None, None).expect("single-file config");
    let session = radix::module::driver::Session::new(config);
    let mut host = radix::module::mir::BufferHost::default();
    crate::script::run_with_session(&session, &path.display().to_string(), &source, &mut host)
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
fn single_file_run_process_code_frontend_error_is_one() {
    let error = RunSourceError::Frontend(Vec::new());
    assert_eq!(single_file_run_process_code(Err(error)), 1);
}
