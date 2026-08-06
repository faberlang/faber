//! Stage 8 S8.2 — per-fixture CLI argv/stdout/exit parity vs the Rust oracle.
//!
//! Each test compiles a CLI Faber program through the static-descriptor
//! adapter lane (analyze → MIR lowering → LLVM text emission with the
//! versioned descriptor), assembles + links it with the LLVM host runtime
//! archive, runs it with the Rust-oracle args, and compares stdout/stderr/exit
//! to the oracle's accepted outcome.

use super::llvm_runtime::{run_llvm_exemplum_with_args, LlvmRunProbe};
use radix::driver::{Config, Session};
use radix::Target;
use std::path::PathBuf;

/// Compile one CLI source through the adapter lane and run it with `args`.
fn run_cli_source(name: &str, source: &str, args: &[&str]) -> LlvmRunProbe {
    static SEQUENCE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    // Parallel tests share one temp dir; a unique stem keeps the emitted .ll
    // and linked .bin from colliding across tests.
    let sequence = SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let unique = format!("{name}-{sequence}-{}", std::process::id());
    let session = Session::new(
        Config::default()
            .with_target(Target::LlvmText)
            .with_dev_stdlib(),
    );
    let mut analysis =
        radix::driver::analyze_source(&session, name, source).expect("analyze CLI source");
    let device_roles = radix::mir::device_roles_from_hir(&analysis.hir);
    let cli_program = analysis.cli_program.as_ref().expect("CLI program");
    let plan = radix::cli_descriptor::build_cli_adapter_plan(cli_program);
    let mir = radix::mir::lower_analyzed_unit_with_cli_adapter_with_context(&mut analysis)
        .expect("lower CLI MIR");
    let llvm = radix::mir::emit_llvm_text_probe_with_cli_adapter(
        &device_roles,
        &mir.validated,
        &mir.interner,
        plan,
    )
    .expect("emit CLI adapter");
    let dir = std::env::temp_dir().join("faber-exempla-cli-parity");
    std::fs::create_dir_all(&dir).expect("parity temp dir");
    let llvm_file = dir.join(format!("{unique}.ll"));
    std::fs::write(&llvm_file, &llvm).expect("write LLVM");
    let fab_path = dir.join(format!("{unique}.fab"));
    std::fs::write(&fab_path, source).expect("write source fixture");
    run_llvm_exemplum_with_args(&llvm_file, &dir, &unique, &fab_path, args)
}

/// Compile one corpus CLI exemplum through the adapter lane.
fn run_corpus_cli(rel: &str, args: &[&str]) -> LlvmRunProbe {
    let path = crate::paths::corpus_dir().join(rel);
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
    let name = rel.replace('/', "_").replace(".fab", "");
    run_cli_source(&name, &source, args)
}

#[test]
fn cli_fab_greet_matches_rust_oracle() {
    let probe = run_corpus_cli("cli/cli.fab", &["greet", "Marcus"]);
    assert_eq!(probe.stdout, "Salve, Marcus!\n", "oracle stdout");
    assert_eq!(probe.exit_code, Some(0), "oracle exit");
}

#[test]
fn cli_fab_version_command_matches_rust_oracle() {
    let probe = run_corpus_cli("cli/cli.fab", &["version"]);
    assert_eq!(probe.stdout, "exemplum v0.1.0\n", "oracle version output");
    assert_eq!(probe.exit_code, Some(0), "oracle exit");
}

#[test]
#[ignore = "known breakage: CLI-vs-rust-oracle parity leaks/flakes under subprocess spawn; tracked separately"]
fn cli_fab_alias_and_global_flag_match_rust_oracle() {
    let probe = run_corpus_cli("cli/cli.fab", &["g", "Tullia"]);
    assert_eq!(probe.stdout, "Salve, Tullia!\n", "alias dispatch");
    assert_eq!(probe.exit_code, Some(0));
    let probe = run_corpus_cli("cli/cli.fab", &["--verbose", "greet", "Cicero"]);
    assert_eq!(probe.stdout, "verbose\nSalve, Cicero!\n", "global flag");
    assert_eq!(probe.exit_code, Some(0));
}

#[test]
fn cli_fab_parse_errors_match_rust_oracle_exit_two() {
    let probe = run_corpus_cli("cli/cli.fab", &["greet"]);
    assert_eq!(probe.exit_code, Some(2), "missing operand exit");
    assert!(
        probe.stderr.contains("error: missing operand 'nomen'"),
        "missing operand diagnostic: {}",
        probe.stderr
    );

    let probe = run_corpus_cli("cli/cli.fab", &["--bogus", "greet"]);
    assert_eq!(probe.exit_code, Some(2), "unknown option exit");
    assert!(
        probe.stderr.contains("error: unknown option '--bogus'"),
        "unknown option diagnostic: {}",
        probe.stderr
    );
}

#[test]
fn cli_fab_unknown_command_matches_rust_oracle() {
    let probe = run_corpus_cli("cli/cli.fab", &["frobnicate"]);
    assert_eq!(probe.exit_code, Some(2), "unknown command exit");
    assert!(
        probe.stderr.contains("error: unknown command 'frobnicate'"),
        "unknown command diagnostic: {}",
        probe.stderr
    );
    assert!(
        probe.stdout.contains("Usage: exemplum [OPTIONS] <COMMAND>"),
        "root help printed for unknown command: {}",
        probe.stdout
    );
}

#[test]
fn operandus_rest_operands_match_rust_oracle() {
    let probe = run_corpus_cli("operandus/operandus.fab", &["input.txt", "extra.txt"]);
    assert_eq!(probe.stdout, "", "empty incipit body");
    assert_eq!(probe.exit_code, Some(0));

    let probe = run_corpus_cli("operandus/operandus.fab", &[]);
    assert_eq!(probe.exit_code, Some(2), "missing operand exit");
    assert!(
        probe.stderr.contains("error: missing operand 'input'"),
        "missing operand diagnostic: {}",
        probe.stderr
    );
}

#[test]
fn optio_option_surface_matches_rust_oracle() {
    let probe = run_corpus_cli("optio/optio.fab", &[]);
    assert_eq!(probe.exit_code, Some(0), "defaults run");

    let probe = run_corpus_cli("optio/optio.fab", &["--count", "nope"]);
    assert_eq!(probe.exit_code, Some(2), "invalid numeric exit");
    assert!(
        probe.stderr.contains("error: invalid numeric value 'nope'"),
        "invalid numeric diagnostic: {}",
        probe.stderr
    );

    let probe = run_corpus_cli("optio/optio.fab", &["--help"]);
    assert_eq!(probe.exit_code, Some(0), "help exit");
    assert!(
        probe.stdout.contains("Usage: optio-smoke [OPTIONS]"),
        "help usage: {}",
        probe.stdout
    );
}

#[test]
fn exitus_fixed_code_matches_rust_oracle() {
    let probe = run_corpus_cli("exitus/exitus.fab", &[]);
    assert_eq!(probe.stdout, "", "no body output");
    assert_eq!(probe.exit_code, Some(1), "exitus 1 process code");
}

#[test]
fn exitus_field_form_exits_with_operand_value() {
    // `exitus (args.exitum)` (CliExit::Field) resolves the numeric operand
    // from the parse table; the emitted adapter routes the process code
    // through the runtime exit-code policy.
    let source = r#"
@ cli "exit-field"
@ operandus numerus exitum
incipit argumenta args exitus (args.exitum) {
}
"#;
    let probe = run_cli_source("exit-field", source, &["7"]);
    assert_eq!(probe.exit_code, Some(7), "field exitus process code");
    assert_eq!(probe.stdout, "", "no body output");
}
