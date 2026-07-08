//! `faber test` — compile and run the package Cargo test harness.

use crate::cli::TestArgs;
use crate::package;

/// Builds the package test harness and maps Faber-level selectors to Cargo test flags.
pub(super) fn cmd_test(args: TestArgs) {
    use std::path::PathBuf;

    let input_path = PathBuf::from(&args.path);
    let test_selection = radix::codegen::rust::TestSelection {
        name: args.name.clone(),
        suite: args.suite.clone(),
        tag: args.tag.clone(),
    };
    let test_selection = if test_selection.name.is_some()
        || test_selection.suite.is_some()
        || test_selection.tag.is_some()
    {
        Some(test_selection)
    } else {
        None
    };

    // POLICY: tests are package-scoped so generated harness metadata and source
    // selection stay aligned.
    let config = radix::driver::Config::default().with_target(radix::codegen::Target::Rust);
    let result =
        package::compile_package_with_test_selection(&config, &input_path, test_selection.as_ref());

    super::eprint_compile_diagnostics(&result.diagnostics);

    let Some(output) = result.output else {
        eprintln!("compilation failed");
        std::process::exit(1);
    };

    let layout = match package::discover_build_layout(&input_path) {
        Ok(l) => l,
        Err(d) => {
            eprintln!("error: {}", d.message);
            std::process::exit(1);
        }
    };

    let meta = if layout.manifest_path.exists() {
        package::read_manifest(&layout.manifest_path).ok()
    } else {
        None
    };

    let code_string = match output {
        radix::Output::Rust(r) => r.code,
        _ => {
            eprintln!("error: test only supports Rust backend packages");
            std::process::exit(1);
        }
    };

    if let Err(d) = package::emit_generated_crate(&layout, &code_string, meta.as_ref()) {
        eprintln!("error emitting: {}", d.message);
        std::process::exit(1);
    }

    // CONTRACT: Cargo's harness expects the name filter before `--`; the
    // remaining flags are passed through as test-harness arguments.
    let mut harness_args: Vec<String> = Vec::new();
    if args.exact {
        harness_args.push("--exact".to_string());
    }
    if args.nocapture {
        harness_args.push("--nocapture".to_string());
    }
    if let Some(n) = args.test_threads {
        harness_args.push("--test-threads".to_string());
        harness_args.push(n.to_string());
    }

    // INVARIANT: clap enforces mutual exclusion before this command handler runs.
    if args.ignored {
        harness_args.push("--ignored".to_string());
    }
    if args.include_ignored {
        harness_args.push("--include-ignored".to_string());
    }

    let status = match package::invoke_cargo_test(&layout, args.filter.as_deref(), &harness_args) {
        Ok(s) => s,
        Err(d) => {
            eprintln!("error: {}", d.message);
            std::process::exit(1);
        }
    };

    if let Some(code) = status.code() {
        std::process::exit(code);
    } else {
        std::process::exit(1);
    }
}
