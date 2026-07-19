use super::*;
use radix::codegen::Target;
use radix::mir::BufferHost;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

// ── run_target_name ───────────────────────────────────────────────────────

#[test]
fn run_target_name_maps_rust() {
    assert_eq!(run_target_name(Target::Rust), "rust");
}

#[test]
fn run_target_name_maps_typescript() {
    assert_eq!(run_target_name(Target::TypeScript), "ts");
}

#[test]
fn run_target_name_maps_go() {
    assert_eq!(run_target_name(Target::Go), "go");
}

#[test]
fn run_target_name_maps_faber() {
    assert_eq!(run_target_name(Target::Faber), "faber");
}

#[test]
fn run_target_name_maps_wasm_variants() {
    assert_eq!(run_target_name(Target::WasmText), "wasm-text");
    assert_eq!(run_target_name(Target::Wasm), "wasm");
}

#[test]
fn run_target_name_maps_text_based_targets() {
    assert_eq!(run_target_name(Target::LlvmText), "llvm-text");
    assert_eq!(run_target_name(Target::MetalText), "metal-text");
    assert_eq!(run_target_name(Target::WgslText), "wgsl-text");
}

#[test]
fn run_target_name_maps_sexp() {
    assert_eq!(run_target_name(Target::Sexp), "sexp");
}

#[test]
fn run_target_name_maps_scena_and_fmir_variants() {
    assert_eq!(run_target_name(Target::Scena), "scena");
    assert_eq!(run_target_name(Target::FmirText), "fmir-text");
    assert_eq!(run_target_name(Target::Fmir), "fmir");
    assert_eq!(run_target_name(Target::FmirBin), "fmir-bin");
}

// ── should_interpret — interpret flag override ────────────────────────────

#[test]
fn interpret_flag_overrides_package_directory() {
    let dir = temp_dir("interpret-flag-override");
    let args = RunArgs {
        path: dir.clone(),
        reader_locale: None,
        target: radix::tool::CliTarget::Rust,
        release: false,
        interpret: true,
        compile: false,
        args: Vec::new(),
    };
    // Even though `dir` is a directory, `--interpret` forces interpreted mode.
    assert!(should_interpret(&args, &dir));
}

#[test]
fn compile_flag_takes_precedence_over_interpret_flag() {
    let fab = PathBuf::from("script.fab");
    let args = RunArgs {
        path: fab.clone(),
        reader_locale: None,
        target: radix::tool::CliTarget::Rust,
        release: false,
        interpret: true,
        compile: true,
        args: Vec::new(),
    };
    // `--compile` gates at line 28 return false before `--interpret` is checked.
    assert!(!should_interpret(&args, &fab));
}

#[test]
fn reader_locale_takes_precedence_over_interpret_flag() {
    let fab = PathBuf::from("script.fab");
    let args = RunArgs {
        path: fab.clone(),
        reader_locale: Some("zh-Hans".to_owned()),
        target: radix::tool::CliTarget::Rust,
        release: false,
        interpret: true,
        compile: false,
        args: Vec::new(),
    };
    // reader_locale gate at line 23 returns false before `--interpret` is checked.
    assert!(!should_interpret(&args, &fab));
}

#[test]
fn non_rust_target_takes_precedence_over_interpret_flag() {
    let fab = PathBuf::from("script.fab");
    let args = RunArgs {
        path: fab.clone(),
        reader_locale: None,
        target: radix::tool::CliTarget::Scena,
        release: false,
        interpret: true,
        compile: false,
        args: Vec::new(),
    };
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

#[test]
fn interpret_policy_defaults_to_single_fab_file() {
    let fab = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../examples/corpus/incipit/salve-munde.fab");
    let args = RunArgs {
        path: fab.clone(),
        reader_locale: None,
        target: radix::tool::CliTarget::Rust,
        release: false,
        interpret: false,
        compile: false,
        args: Vec::new(),
    };
    assert!(should_interpret(&args, &fab));
}

#[test]
fn compile_flag_overrides_single_fab_file() {
    let fab = PathBuf::from("script.fab");
    let args = RunArgs {
        path: fab.clone(),
        reader_locale: None,
        target: radix::tool::CliTarget::Rust,
        release: false,
        interpret: false,
        compile: true,
        args: Vec::new(),
    };
    assert!(!should_interpret(&args, &fab));
}

#[test]
fn package_directory_defaults_to_compiled_run_policy() {
    let dir = temp_dir("compiled-package-policy");
    let args = RunArgs {
        path: dir.clone(),
        reader_locale: None,
        target: radix::tool::CliTarget::Rust,
        release: false,
        interpret: false,
        compile: false,
        args: Vec::new(),
    };

    assert!(!should_interpret(&args, &dir));
}

#[test]
fn scena_target_never_uses_script_interpret_policy() {
    let fab = PathBuf::from("script.fab");
    let args = RunArgs {
        path: fab.clone(),
        reader_locale: None,
        target: radix::tool::CliTarget::Scena,
        release: false,
        interpret: false,
        compile: false,
        args: Vec::new(),
    };

    assert!(!should_interpret(&args, &fab));
}

#[test]
fn reader_locale_forces_compiled_run_policy_for_single_fab_file() {
    let fab = PathBuf::from("script.fab");
    let args = RunArgs {
        path: fab.clone(),
        reader_locale: Some("zh-Hans".to_owned()),
        target: radix::tool::CliTarget::Rust,
        release: false,
        interpret: false,
        compile: false,
        args: Vec::new(),
    };

    assert!(!should_interpret(&args, &fab));
}

#[test]
fn run_config_loads_reader_locale_pack_for_go_targets() {
    let example = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/reader-locale/th-TH");

    let config = run_config(Target::Go, &example, Some("th-TH")).expect("run config");

    assert_eq!(config.target, Target::Go);
    assert_eq!(
        config
            .reader_pack
            .as_ref()
            .map(|pack| pack.metadata.id.as_str()),
        Some("th-TH")
    );
}

#[test]
fn run_config_uses_manifest_reader_locale_for_non_rust_targets() {
    let example = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/reader-locale/th-TH");

    let config = run_config(Target::FmirText, &example, None).expect("run config");

    assert_eq!(config.target, Target::FmirText);
    assert_eq!(
        config
            .reader_pack
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
