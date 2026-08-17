use super::*;
use crate::cli::{BackendSelection, RunArgs};
use radix_module::mir::RunSourceError;
use radix_program::mir::DeviceSelection;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
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

fn run_args(path: PathBuf, backend: Option<BackendSelection>) -> RunArgs {
    RunArgs {
        path,
        locale: None,
        diagnostics_locale: None,
        backend,
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

// ── S1-5 backend selection: precedence (CLI > manifest > auto) ───────────

fn write_manifest(dir: &Path, device_backend: Option<&str>) {
    let backend_line = match device_backend {
        Some(backend) => format!("backend = \"{backend}\"\n"),
        None => String::new(),
    };
    std::fs::write(
        dir.join("faber.toml"),
        format!(
            "[package]\nname = \"backend-precedence\"\nversion = \"0.1.0\"\nedition = \"1\"\n\n[paths]\nentry = \"main.fab\"\n\n[build]\nkind = \"bin\"\n\n[device]\n{backend_line}"
        ),
    )
    .expect("write manifest");
}

#[test]
fn route_selection_cli_flag_overrides_manifest() {
    let dir = temp_dir("backend-cli-overrides-manifest");
    std::fs::write(dir.join("main.fab"), "incipit {\n}\n").expect("write entry");
    write_manifest(&dir, Some("metal"));
    let args = run_args(dir.clone(), Some(BackendSelection::Auto));
    let selection = resolve_route_selection(&args, &dir).expect("resolves");
    assert_eq!(selection, DeviceSelection::Auto);
}

#[test]
fn route_selection_manifest_used_when_no_cli_flag() {
    let dir = temp_dir("backend-manifest-used");
    std::fs::write(dir.join("main.fab"), "incipit {\n}\n").expect("write entry");
    write_manifest(&dir, Some("cuda"));
    let args = run_args(dir.clone(), None);
    let selection = resolve_route_selection(&args, &dir).expect("resolves");
    assert_eq!(selection, DeviceSelection::Cuda);
}

#[test]
fn route_selection_defaults_to_auto_without_manifest_key() {
    let dir = temp_dir("backend-default-auto");
    std::fs::write(dir.join("main.fab"), "incipit {\n}\n").expect("write entry");
    write_manifest(&dir, None);
    let args = run_args(dir.clone(), None);
    let selection = resolve_route_selection(&args, &dir).expect("resolves");
    assert_eq!(selection, DeviceSelection::Auto);
}

#[test]
fn route_selection_defaults_to_auto_without_manifest() {
    let dir = temp_dir("backend-no-manifest");
    std::fs::write(dir.join("main.fab"), "incipit {\n}\n").expect("write entry");
    let args = run_args(dir.clone(), None);
    let selection = resolve_route_selection(&args, &dir).expect("resolves");
    assert_eq!(selection, DeviceSelection::Auto);
}

#[test]
fn route_selection_rejects_invalid_manifest_backend() {
    let dir = temp_dir("backend-invalid-manifest");
    std::fs::write(dir.join("main.fab"), "incipit {\n}\n").expect("write entry");
    write_manifest(&dir, Some("rocm"));
    let args = run_args(dir.clone(), None);
    let err = resolve_route_selection(&args, &dir).expect_err("invalid value must fail closed");
    assert!(err.message.contains("rocm"));
    assert!(err.message.contains("device.backend"));
}

#[test]
fn route_selection_invalid_manifest_with_cli_flag_is_ignored() {
    // Precedence: the CLI flag wins, so an invalid manifest value never
    // blocks a package run that names the backend explicitly.
    let dir = temp_dir("backend-cli-overrides-invalid-manifest");
    std::fs::write(dir.join("main.fab"), "incipit {\n}\n").expect("write entry");
    write_manifest(&dir, Some("rocm"));
    let args = run_args(dir.clone(), Some(BackendSelection::Auto));
    let selection = resolve_route_selection(&args, &dir).expect("CLI flag wins");
    assert_eq!(selection, DeviceSelection::Auto);
}

// ── run dispatch: the package-MIR runner with a selection ────────────────

#[test]
fn package_mir_run_with_selection_auto_stays_on_cpu_route() {
    let dir = temp_dir("selection-auto-cpu");
    let entry = dir.join("main.fab");
    std::fs::write(&entry, "main { print \"cpu\" }\n").expect("write entry");

    let mut host = radix_module::mir::BufferHost::default();
    radix_program::mir::run_package_mir_with_selection(
        &radix_module::driver::Config::default(),
        &entry,
        DeviceSelection::Auto,
        &crate::package::mir::faber_mir_seams(),
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
    let session = radix_module::driver::Session::new(config);
    let mut host = radix_module::mir::BufferHost::default();
    crate::script::run_with_session(&session, &path.display().to_string(), &source, &mut host)
}

#[test]
fn single_file_run_process_code_propagates_runner_exit() {
    let dir = temp_dir("single-file-exit");
    let entry = dir.join("gate.fab");
    std::fs::write(
        &entry,
        r#"
import from "faber:processus" processus

main {
    processus.exit_process(7)
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
