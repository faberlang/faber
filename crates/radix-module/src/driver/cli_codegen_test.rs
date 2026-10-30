//! CLI codegen tests — all spawn external toolchains and are #[ignore] by default.

use super::compile;
use crate::codegen::Target;
use crate::driver::ModuleCompileRequest;
use crate::test_support::test_session_la;

// ── single-command CLI ──

#[ignore = "slow: spawns cargo build of generated Rust binary; run with --ignored"]
#[test]
fn rust_single_command_cli_generates_compilable_parser_and_runtime_behavior() {
    let session = test_session_la(Target::HirRust);
    let source = r#"
@ cli "tool"
incipit argumenta args {
  si args.longitudo() ≻ 0 {
    nota args[0]
  }
}
"#;
    let result = compile(&session, "cli.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "expected compile success: {:?}", result.diagnostics);
    let output = super::rust_output(result);
    assert!(output.code.contains("fn entry()"), "expected entry function");
    assert!(output.code.contains("clap::Parser"), "expected clap parser derive");

    // Prove the emitted Rust compiles:
    let binary = super::compile_rust_source_with_rustc(&output.code, "single-command-cli");
    assert!(binary.exists(), "binary must exist after successful compilation");

    // Run the binary to check parse behavior.
    let binary_path = binary.to_string_lossy().to_string();
    let empty = std::process::Command::new(&binary_path)
        .output()
        .expect("launch single-command cli");
    assert!(empty.status.success(), "empty args must succeed");

    let with_arg = std::process::Command::new(&binary_path)
        .arg("salve")
        .output()
        .expect("launch single-command cli with arg");
    assert!(with_arg.status.success(), "with arg must succeed");
    let stdout = String::from_utf8_lossy(&with_arg.stdout);
    assert!(stdout.contains("salve"), "expected output 'salve': {stdout}");
}

#[ignore = "slow: spawns cargo build of generated Rust binary; run with --ignored"]
#[test]
fn rust_single_command_cli_reports_parse_errors() {
    let session = test_session_la(Target::HirRust);
    let source = r#"
@ cli "tool"
incipit argumenta args {
  si args.longitudo() ≻ 0 {
    nota args[0]
  }
}
"#;
    let result = compile(&session, "cli.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "expected compile success: {:?}", result.diagnostics);
    let output = super::rust_output(result);
    let binary = super::compile_rust_source_with_rustc(&output.code, "cli-parse-errors");
    assert!(binary.exists(), "binary must exist");

    // Extra args should be treated as positional (clap allows them in default
    // mode). Verify the binary at least runs with extra input.
    let extra = std::process::Command::new(binary.to_string_lossy().to_string())
        .arg("hello")
        .arg("world")
        .output()
        .expect("launch with extra args");
    assert!(extra.status.success(), "extra args should parse: {extra:?}");
}

#[ignore = "slow: spawns cargo build of generated Rust binary; run with --ignored"]
#[test]
fn rust_single_command_cli_supports_fixed_exitus() {
    let session = test_session_la(Target::HirRust);
    let source = r#"
@ cli "tool"
incipit argumenta args {
  sit _ code ← "0"
  nota code
} exitu numerus
"#;
    let result = compile(&session, "cli.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "expected compile success: {:?}", result.diagnostics);
    let output = super::rust_output(result);
    assert!(output.code.contains("-> i64"), "expected fixed exitus return type");

    let binary = super::compile_rust_source_with_rustc(&output.code, "cli-fixed-exitus");
    assert!(binary.exists(), "binary must exist");

    let run = std::process::Command::new(binary.to_string_lossy().to_string())
        .output()
        .expect("launch fixed-exitus cli");
    assert!(run.status.success(), "fixed exitus must succeed: {run:?}");
}

#[ignore = "slow: spawns cargo build of generated Rust binary; run with --ignored"]
#[test]
fn rust_cli_dynamic_exit_uses_the_hygienic_entry_args_binding() {
    let session = test_session_la(Target::HirRust);
    let source = r#"
@ cli "tool"
@ operandus numerus status
incipit argumenta argv exitus (argv.status) {
}
"#;
    let result = compile(&session, "cli-dynamic-exitus.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "expected compile success: {:?}", result.diagnostics);
    let output = super::rust_output(result);
    let binary = super::compile_rust_source_with_rustc(&output.code, "cli-dynamic-exitus");
    assert!(binary.exists(), "binary must exist after successful compilation");

    let run = std::process::Command::new(binary)
        .arg("23")
        .output()
        .expect("launch dynamic-exitus cli");
    assert_eq!(run.status.code(), Some(23), "dynamic CLI exit must use the parsed operand");
}

#[test]
fn cli_codegen_gates_non_rust_targets() {
    let source = r#"@ cli "tool"
incipit {
  nota "ok"
}"#;
    // CLI codegen is currently Rust-only; other targets should produce a clear error.
    for target in &[Target::HirTypeScript, Target::HirGo] {
        let session = test_session_la(*target);
        let result = compile(&session, "cli.fab", source, ModuleCompileRequest::default());

        assert!(!result.success(), "CLI should fail for {target:?}");
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == Some("SEM009") && d.issue() == Some("missing_entry_argument")),
            "expected SEM009 missing_entry_argument for {target:?}: {:?}",
            result.diagnostics,
        );
    }
}

// ── subcommand CLI ──

#[ignore = "slow: spawns cargo build of generated Rust binary; run with --ignored"]
#[test]
fn rust_subcommand_cli_dispatches_aliases_nested_commands_and_command_args() {
    let session = test_session_la(Target::HirRust);
    let source = r#"
@ cli "tool"
incipit {
  nota "base"
}

@ cli.command "greet"
functio greet(textus name) {
  nota "salve, " + name
}

@ cli.command "config"
incipit {
  nota "config"
}

@ cli.command "config.get"
functio config_get(textus key) {
  nota key
}

@ cli.command "config.set"
functio config_set(textus key, textus value) {
  nota key + " = " + value
}
"#;
    let result = compile(&session, "cli.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "expected compile success: {:?}", result.diagnostics);
    let output = super::rust_output(result);
    assert!(output.code.contains("clap::Parser"), "expected clap");
    assert!(output.code.contains("subcommand"), "expected subcommand enum");

    let binary = super::compile_rust_source_with_rustc(&output.code, "subcommand-cli");
    assert!(binary.exists(), "binary must exist");

    let bin = binary.to_string_lossy().to_string();

    // Base command
    let base = std::process::Command::new(&bin).output().expect("base cmd");
    assert!(base.status.success(), "base must succeed");

    // Alias "greet"
    let greet = std::process::Command::new(&bin)
        .arg("greet")
        .arg("Marcus")
        .output()
        .expect("greet cmd");
    assert!(greet.status.success(), "greet must succeed");
    let greet_out = String::from_utf8_lossy(&greet.stdout);
    assert!(greet_out.contains("salve"), "expected 'salve': {greet_out}");

    // Nested "config get"
    let config_get = std::process::Command::new(&bin)
        .arg("config")
        .arg("get")
        .arg("port")
        .output()
        .expect("config get cmd");
    assert!(config_get.status.success(), "config get must succeed");
    let cg_out = String::from_utf8_lossy(&config_get.stdout);
    assert!(cg_out.contains("port"), "expected 'port': {cg_out}");

    // Nested "config set"
    let config_set = std::process::Command::new(&bin)
        .arg("config")
        .arg("set")
        .arg("port")
        .arg("8080")
        .output()
        .expect("config set cmd");
    assert!(config_set.status.success(), "config set must succeed");
    let cs_out = String::from_utf8_lossy(&config_set.stdout);
    assert!(cs_out.contains("port = 8080"), "expected 'port = 8080': {cs_out}");
}

#[ignore = "slow: spawns cargo build of generated Rust binary; run with --ignored"]
#[test]
fn rust_subcommand_cli_prefers_longest_matching_command_path() {
    let session = test_session_la(Target::HirRust);
    let source = r#"
@ cli "tool"
incipit {
  nota "base"
}

@ cli.command "run"
incipit {
  nota "run"
}

@ cli.command "run.remote"
functio run_remote(textus host) {
  nota host
}
"#;
    let result = compile(&session, "cli.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "expected success: {:?}", result.diagnostics);
    let output = super::rust_output(result);

    let binary = super::compile_rust_source_with_rustc(&output.code, "cli-longest-path");
    assert!(binary.exists());

    let bin = binary.to_string_lossy().to_string();

    // "run" dispatches to the `run` command, not as prefix to `run.remote`
    let run = std::process::Command::new(&bin)
        .arg("run")
        .output()
        .expect("run cmd");
    assert!(run.status.success(), "run must succeed");

    // "run remote host" dispatches to `run.remote`
    let remote = std::process::Command::new(&bin)
        .arg("run")
        .arg("remote")
        .arg("example.com")
        .output()
        .expect("run remote cmd");
    assert!(remote.status.success(), "run remote must succeed");
    let remote_out = String::from_utf8_lossy(&remote.stdout);
    assert!(remote_out.contains("example.com"), "expected host: {remote_out}");
}

#[ignore = "slow: spawns cargo build of generated Rust binary; run with --ignored"]
#[test]
fn rust_subcommand_cli_sanitizes_generated_helper_names_without_changing_command_paths() {
    let session = test_session_la(Target::HirRust);
    let source = r#"
@ cli "tool"
incipit {
  nota "base"
}

@ cli.command "run-remote"
functio run_remote(textus host) {
  nota host
}

@ cli.command "run:local"
functio run_local(textus path) {
  nota path
}

@ cli.command "config.ssl-verify"
functio config_ssl_verify(bivalens enabled) {
  if enabled { nota "on" }
  else { nota "off" }
}
"#;
    let result = compile(&session, "cli.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "expected success: {:?}", result.diagnostics);
    let output = super::rust_output(result);

    // Verify sanitized helper function names in the emitted code.
    assert!(
        output.code.contains("fn run_remote(host: String)"),
        "command name must be sanitized: run-remote -> run_remote"
    );
    assert!(
        output.code.contains("fn run_local(path: String)"),
        "command name must be sanitized: run:local -> run_local"
    );
    assert!(
        output.code.contains("fn config_ssl_verify(enabled: bool)"),
        "command name must be sanitized: config.ssl-verify -> config_ssl_verify"
    );

    // But the struct field names and CLI dispatch use the original path.
    assert!(
        output.code.contains("\"run-remote\""),
        "CLI path should be derivable from sanitized name"
    );

    let binary = super::compile_rust_source_with_rustc(&output.code, "cli-sanitize-names");
    assert!(binary.exists());

    let bin = binary.to_string_lossy().to_string();
    let run = std::process::Command::new(&bin)
        .arg("run-remote")
        .arg("myhost")
        .output()
        .expect("run-remote cmd");
    assert!(run.status.success(), "run-remote must succeed");
    let out = String::from_utf8_lossy(&run.stdout);
    assert!(out.contains("myhost"), "expected host: {out}");
}

#[cfg(feature = "mir-wasm")]
#[test]
fn wasm_cli_program_routes_through_the_ordinary_emit_path() {
    // wasm-host-parity P1 residual: the shared MIR CLI-program lowering is
    // already in MIR (P1 a5e4b69f1), so the Wasm/WasmText targets route CLI
    // programs through the ordinary emit path instead of the previous
    // `cli_codegen_rust_only` rejection. The ten P1 CLI fixtures emit and
    // wasm-tools validate.
    let session = test_session_la(Target::MirWasm);
    let source = r#"@ cli "tool"
incipit argumenta args {
  nota "ok"
}"#;
    let result = compile(&session, "cli.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "expected wasm-text CLI success: {:?}", result.diagnostics);
    let crate::Output::WasmText(crate::WasmTextOutput { code }) = result.output.unwrap() else {
        panic!("expected wasm-text output");
    };
    assert!(code.contains("(module"), "expected a wasm module:\n{code}");
    // The emitted module must be valid WAT (the shared CLI MIR shapes emit
    // through the same validator the binary probe uses).
    crate::backend_smoke::assert_mir_wasm_text_smoke(&code, "cli-wasm-text");
}
