use super::*;
use radix::codegen::Target;
use radix::driver::WarnPolicy;
use radix::mir::BufferHost;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

// ── run_target_name ───────────────────────────────────────────────────────

#[test]
fn run_target_name_maps_rust() {
    assert_eq!(run_target_name(Target::HirRust), "rust");
}

#[test]
fn run_target_name_maps_typescript() {
    assert_eq!(run_target_name(Target::HirTypeScript), "ts");
}

#[test]
fn run_target_name_maps_go() {
    assert_eq!(run_target_name(Target::HirGo), "go");
}

#[test]
fn run_target_name_maps_faber() {
    assert_eq!(run_target_name(Target::HirFaber), "faber");
}

#[test]
fn run_target_name_maps_wasm_text() {
    assert_eq!(run_target_name(Target::MirWasm), "wasm-text");
}

#[test]
fn run_target_name_maps_wasm() {
    assert_eq!(run_target_name(Target::MirWasmBinary), "wasm");
}

#[test]
fn run_target_name_maps_llvm_text() {
    assert_eq!(run_target_name(Target::MirLlvm), "llvm-text");
}

#[test]
fn run_target_name_maps_metal_text() {
    assert_eq!(run_target_name(Target::MirMetal), "metal-text");
}

#[test]
fn run_target_name_maps_wgsl_text() {
    assert_eq!(run_target_name(Target::MirWgsl), "wgsl-text");
}

#[test]
fn run_target_name_maps_sexp() {
    assert_eq!(run_target_name(Target::MirSexp), "sexp");
}

#[test]
fn run_target_name_maps_scena() {
    assert_eq!(run_target_name(Target::MirScena), "scena");
}

#[test]
fn run_target_name_maps_fmir_text() {
    assert_eq!(run_target_name(Target::MirFmir), "fmir-text");
}

#[test]
fn run_target_name_maps_fmir() {
    assert_eq!(run_target_name(Target::MirFmirBinary), "fmir");
}

#[test]
fn run_target_name_maps_fmir_bin() {
    assert_eq!(run_target_name(Target::MirFmirBundle), "fmir-bin");
}

#[test]
fn run_target_name_maps_swift() {
    assert_eq!(run_target_name(Target::HirSwift), "swift");
}

// ── should_interpret — interpret flag override ────────────────────────────

#[test]
fn interpret_flag_overrides_package_directory() {
    let dir = temp_dir("interpret-flag-override");
    let args = run_args(
        dir.clone(),
        true,
        false,
        None,
        radix::tool::CliTarget::HirRust,
    );
    // Even though `dir` is a directory, `--interpret` forces interpreted mode.
    assert!(should_interpret(&args, &dir));
}

#[test]
fn compile_flag_takes_precedence_over_interpret_flag() {
    let fab = PathBuf::from("script.fab");
    let args = run_args(
        fab.clone(),
        true,
        true,
        None,
        radix::tool::CliTarget::HirRust,
    );
    // `--compile` gates at line 28 return false before `--interpret` is checked.
    assert!(!should_interpret(&args, &fab));
}

#[test]
fn locale_takes_precedence_over_interpret_flag() {
    let fab = PathBuf::from("script.fab");
    let args = run_args(
        fab.clone(),
        true,
        false,
        Some("zh-Hans".to_owned()),
        radix::tool::CliTarget::HirRust,
    );
    // locale gate at line 23 returns false before `--interpret` is checked.
    assert!(!should_interpret(&args, &fab));
}

#[test]
fn non_rust_target_takes_precedence_over_interpret_flag() {
    let fab = PathBuf::from("script.fab");
    let args = run_args(
        fab.clone(),
        true,
        false,
        None,
        radix::tool::CliTarget::MirScena,
    );
    // Target gate at line 25 returns false before `--interpret` is checked.
    assert!(!should_interpret(&args, &fab));
}

fn temp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("faber-run-{label}-{nanos}"));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn run_args(
    path: PathBuf,
    interpret: bool,
    compile: bool,
    locale: Option<String>,
    target: radix::tool::CliTarget,
) -> RunArgs {
    RunArgs {
        path,
        locale,
        diagnostics_locale: None,
        target: Some(target),
        backend: None,
        release: false,
        interpret,
        compile,
        deny_warnings: false,
        deny: Vec::new(),
        args: Vec::new(),
    }
}

#[test]
fn interpret_policy_defaults_to_single_fab_file() {
    let fab =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../radix/corpus/incipit/salve-munde.fab");
    let args = run_args(
        fab.clone(),
        false,
        false,
        None,
        radix::tool::CliTarget::HirRust,
    );
    assert!(should_interpret(&args, &fab));
}

#[test]
fn compile_flag_overrides_single_fab_file() {
    let fab = PathBuf::from("script.fab");
    let args = run_args(
        fab.clone(),
        false,
        true,
        None,
        radix::tool::CliTarget::HirRust,
    );
    assert!(!should_interpret(&args, &fab));
}

#[test]
fn package_directory_defaults_to_compiled_run_policy() {
    let dir = temp_dir("compiled-package-policy");
    let args = run_args(
        dir.clone(),
        false,
        false,
        None,
        radix::tool::CliTarget::HirRust,
    );

    assert!(!should_interpret(&args, &dir));
}

#[test]
fn nonexistent_path_does_not_interpret() {
    let dir = tempfile::tempdir().expect("temp dir");
    let missing = dir.path().join("nonexistent_script.fab");
    let args = run_args(
        missing.clone(),
        false,
        false,
        None,
        radix::tool::CliTarget::HirRust,
    );
    assert!(!should_interpret(&args, &missing));
}

#[test]
fn nonexistent_path_with_interpret_flag_returns_true() {
    let dir = tempfile::tempdir().expect("temp dir");
    let missing = dir.path().join("nonexistent_script.fab");
    let args = run_args(
        missing.clone(),
        true,
        false,
        None,
        radix::tool::CliTarget::HirRust,
    );
    assert!(should_interpret(&args, &missing));
}

#[test]
fn scena_target_never_uses_script_interpret_policy() {
    let fab = PathBuf::from("script.fab");
    let args = run_args(
        fab.clone(),
        false,
        false,
        None,
        radix::tool::CliTarget::MirScena,
    );

    assert!(!should_interpret(&args, &fab));
}

#[test]
fn locale_forces_compiled_run_policy_for_single_fab_file() {
    let fab = PathBuf::from("script.fab");
    let args = run_args(
        fab.clone(),
        false,
        false,
        Some("zh-Hans".to_owned()),
        radix::tool::CliTarget::HirRust,
    );

    assert!(!should_interpret(&args, &fab));
}

#[test]
fn run_config_loads_locale_pack_for_go_targets() {
    let example = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/reader-locale/th-TH");

    let config = run_config(
        Target::HirGo,
        &example,
        Some("th-TH"),
        None,
        WarnPolicy::default(),
    )
    .expect("run config");

    assert_eq!(config.target, Target::HirGo);
    assert_eq!(
        config
            .locale_pack
            .as_ref()
            .map(|pack| pack.metadata.id.as_str()),
        Some("th-TH")
    );
}

#[test]
fn run_config_uses_manifest_locale_for_non_rust_targets() {
    let example = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/reader-locale/th-TH");

    let config = run_config(Target::MirFmir, &example, None, None, WarnPolicy::default())
        .expect("run config");

    assert_eq!(config.target, Target::MirFmir);
    assert_eq!(
        config
            .locale_pack
            .as_ref()
            .map(|pack| pack.metadata.id.as_str()),
        Some("th-TH")
    );
}

#[test]
fn run_scena_package_forwards_argv_through_artifact() {
    let dir = temp_dir("scena-package-run");
    let entry = dir.join("main.fab");
    std::fs::write(
        &entry,
        r#"
@ cli "tool"
@ operandus textus name
incipit argumenta args {
  nota args.name
}
"#,
    )
    .expect("write entry");

    let mut host = BufferHost::with_argumenta(vec!["Ian".to_owned()]);
    let result = run_scena_package_with_host(&entry, &["Ian".to_owned()], &mut host);

    assert!(
        result.is_ok(),
        "expected scena artifact run success, got {:?}",
        result
            .err()
            .unwrap_or_default()
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    assert_eq!(host.stdout_lines, vec!["Ian".to_owned()]);
    assert!(dir.join("target/faber-mir/image.toml").exists());
    assert!(!dir.join("target/faber/Cargo.toml").exists());
}

// ── S1-5 backend selection: precedence (CLI > manifest > auto) ───────────

fn run_args_with_backend(path: PathBuf, backend: Option<crate::cli::BackendSelection>) -> RunArgs {
    let mut args = run_args(
        path.clone(),
        false,
        false,
        None,
        radix::tool::CliTarget::HirFhir,
    );
    args.backend = backend;
    args
}

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
    let args = run_args_with_backend(dir.clone(), Some(crate::cli::BackendSelection::Auto));
    let selection = resolve_route_selection(&args, &dir).expect("resolves");
    assert_eq!(selection, DeviceSelection::Auto);
}

#[test]
fn route_selection_manifest_used_when_no_cli_flag() {
    let dir = temp_dir("backend-manifest-used");
    std::fs::write(dir.join("main.fab"), "incipit {\n}\n").expect("write entry");
    write_manifest(&dir, Some("cuda"));
    let args = run_args_with_backend(dir.clone(), None);
    let selection = resolve_route_selection(&args, &dir).expect("resolves");
    assert_eq!(selection, DeviceSelection::Cuda);
}

#[test]
fn route_selection_defaults_to_auto_without_manifest_key() {
    let dir = temp_dir("backend-default-auto");
    std::fs::write(dir.join("main.fab"), "incipit {\n}\n").expect("write entry");
    write_manifest(&dir, None);
    let args = run_args_with_backend(dir.clone(), None);
    let selection = resolve_route_selection(&args, &dir).expect("resolves");
    assert_eq!(selection, DeviceSelection::Auto);
}

#[test]
fn route_selection_defaults_to_auto_without_manifest() {
    let dir = temp_dir("backend-no-manifest");
    std::fs::write(dir.join("main.fab"), "incipit {\n}\n").expect("write entry");
    let args = run_args_with_backend(dir.clone(), None);
    let selection = resolve_route_selection(&args, &dir).expect("resolves");
    assert_eq!(selection, DeviceSelection::Auto);
}

#[test]
fn route_selection_rejects_invalid_manifest_backend() {
    let dir = temp_dir("backend-invalid-manifest");
    std::fs::write(dir.join("main.fab"), "incipit {\n}\n").expect("write entry");
    write_manifest(&dir, Some("rocm"));
    let args = run_args_with_backend(dir.clone(), None);
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
    let args = run_args_with_backend(dir.clone(), Some(crate::cli::BackendSelection::Auto));
    let selection = resolve_route_selection(&args, &dir).expect("CLI flag wins");
    assert_eq!(selection, DeviceSelection::Auto);
}

// ── S1-5 route decision: CPU route stays unchanged ───────────────────────

#[test]
fn cpu_route_decision_returns_none_for_auto_without_device_program() {
    // `auto` + no device program resolves to the CPU-only route on any
    // machine (the admitted-list probe is irrelevant when no device program
    // is required).
    let backend = resolve_route_backend_or_exit(DeviceSelection::Auto, false);
    assert_eq!(backend, None);
}
