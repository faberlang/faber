use super::*;
use radix::mir::BufferHost;
use std::path::PathBuf;
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

#[test]
fn interpret_policy_defaults_to_single_fab_file() {
    let fab = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../examples/corpus/incipit/salve-munde.fab");
    let args = RunArgs {
        path: fab.clone(),
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
        target: radix::tool::CliTarget::Scena,
        release: false,
        interpret: false,
        compile: false,
        args: Vec::new(),
    };

    assert!(!should_interpret(&args, &fab));
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
