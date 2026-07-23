use radix::{codegen::Target, tool::compile_cli_path, Output};
use std::path::Path;

/// Compile a single Faber exemplum to Swift via the single-file emit path.
fn compile_swift_exemplum(file: &Path) -> Result<String, String> {
    let result = compile_cli_path(file, false, Target::Swift);
    match result.output {
        Some(Output::Swift(output)) => Ok(output.code),
        Some(_) => Err("compiler did not produce Swift output".to_owned()),
        None => {
            let diagnostics = super::common::format_diagnostics(&result);
            Err(format!("compile failed: {diagnostics}"))
        }
    }
}

/// Returns whether `swiftc` is on PATH and responds to `--version`.
fn swift_available() -> bool {
    super::common::command_available("swiftc", &["--version"])
}

#[test]
#[ignore = "slow swift e2e; run: cargo test -p exempla --test e2e_harness exempla_swift_e2e -- --ignored --nocapture"]
fn exempla_swift_e2e() {
    if !swift_available() {
        eprintln!("swiftc not found on PATH; skipping Swift exempla end-to-end harness");
        return;
    }

    // SC-000: harness shell exists but no exempla corpus is listed yet.
    // SC-001 adds scalar exempla to an expected-pass list.
    eprintln!("Swift e2e harness shell: swiftc available, no exempla listed (SC-001 fills)");
}
