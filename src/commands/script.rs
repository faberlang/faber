//! Interpreted Faber source execution: `faber script`, the `-c` one-liner,
//! and the `faber repl` interactive stepper.
//!
//! `script` owns the canonical interpreted-source command path shared with
//! `faber run --interpret` (retained until Stage 6 removal). It routes a path to
//! the single-source stepper, the package-MIR runner, or archive extraction —
//! never to generated Rust or Cargo.

use crate::cli::{ReplArgs, ScriptArgs};
use crate::io_buf::write_prompt;
use crate::package;
use crate::script::{interpret_source, interpret_source_or_exit, print_run_source_error};
use radix::diagnostics::Diagnostic;
use radix::mir::StdioHost;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Run `faber script`: interpret a file, package directory, manifest, or
/// archive without compiling to Rust or invoking Cargo.
pub(super) fn cmd_script(args: &ScriptArgs) {
    let path = PathBuf::from(&args.path);
    interpret_path(&path, &args.args);
}

/// Dispatch an interpreted input path to the package-MIR runner, archive
/// extraction, or single-source stepper. Shared by `faber script` and
/// `faber run --interpret`.
pub(super) fn interpret_path(path: &PathBuf, program_args: &[String]) {
    if super::archive::is_zip_archive(path) {
        let archive = match super::archive::extract_package_archive(path) {
            Ok(archive) => archive,
            Err(diagnostic) => {
                eprint_archive_diagnostics(std::slice::from_ref(diagnostic.as_ref()));
                std::process::exit(1);
            }
        };
        let config = radix::driver::Config::default();
        let mut host = StdioHost::with_argumenta(program_args.to_vec());
        if let Err(mut diagnostics) =
            package::run_package_mir(&config, archive.package_input(), &mut host)
        {
            archive.remap_diagnostics(&mut diagnostics);
            eprint_archive_diagnostics(&diagnostics);
            drop(archive);
            std::process::exit(1);
        }
        return;
    }

    if is_package_interpret_input(path) {
        let config = radix::driver::Config::default();
        let mut host = StdioHost::with_argumenta(program_args.to_vec());
        if let Err(diagnostics) = package::run_package_mir(&config, path, &mut host) {
            super::eprint_compile_diagnostics(&diagnostics);
            std::process::exit(1);
        }
        return;
    }

    let source = match fs::read_to_string(path) {
        Ok(source) => source,
        Err(err) => {
            eprintln!("error: cannot read {}: {err}", path.display());
            std::process::exit(1);
        }
    };

    let mut host = StdioHost::with_argumenta(program_args.to_vec());
    interpret_source_or_exit(&path.display().to_string(), &source, &mut host);
}

/// Whether `path` routes through the package-MIR runner rather than the
/// single-source stepper: manifest-backed or directory packages, or a
/// manifestless `.fab` file that declares a non-kernel import.
pub(super) fn is_package_interpret_input(path: &Path) -> bool {
    package::is_manifest_backed_or_directory_package_input(path)
        || manifestless_file_declares_non_kernel_import(path)
}

fn manifestless_file_declares_non_kernel_import(path: &Path) -> bool {
    if !is_single_fab_file(path) {
        return false;
    }
    let Ok(raw_source) = fs::read_to_string(path) else {
        return false;
    };
    let display_name = path.display().to_string();
    let Ok(peeled) = radix::driver::peel_raw_source(&display_name, &raw_source) else {
        return false;
    };
    let parse = radix::parser::parse(radix::lexer::lex(peeled.body));
    let Some(program) = parse.program else {
        return false;
    };
    program.statements.iter().any(|statement| {
        let radix::syntax::StmtKind::Import(import) = &statement.kind else {
            return false;
        };
        !radix::kernel::is_kernel_import_path(parse.interner.resolve(import.path))
    })
}

/// Whether `path` is a single `.fab` file (the single-source stepper input).
pub(super) fn is_single_fab_file(path: &Path) -> bool {
    path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("fab")
}

fn eprint_archive_diagnostics(diagnostics: &[Diagnostic]) {
    for diagnostic in diagnostics {
        let level = if diagnostic.is_error() {
            "error"
        } else {
            "warning"
        };
        if diagnostic.file.is_empty() {
            eprintln!("{level}: {}", diagnostic.message);
        } else {
            eprintln!("{level}: {}: {}", diagnostic.file, diagnostic.message);
        }
    }
}

/// Execute `-c` / `--command` source via the MIR stepper.
pub(super) fn cmd_eval(source: &str, args: Vec<String>) {
    let mut host = StdioHost::with_argumenta(args);
    if let Err(error) = interpret_source("command-line", &source, &mut host) {
        print_run_source_error(&error);
        std::process::exit(1);
    }
}

/// Interactive REPL: accumulate cells and re-lower MIR on each input line.
pub(super) fn cmd_repl(args: ReplArgs) {
    let mut host = StdioHost::with_argumenta(args.args);
    let mut buffer = String::new();
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    loop {
        write_prompt(&mut stdout, "faber> ");
        let mut line = String::new();
        match stdin.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(err) => {
                eprintln!("error: cannot read repl input: {err}");
                std::process::exit(1);
            }
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if matches!(trimmed, ":quit" | ":exit") {
            break;
        }

        if !buffer.is_empty() && !buffer.ends_with('\n') {
            buffer.push('\n');
        }
        buffer.push_str(trimmed);
        buffer.push('\n');

        if let Err(error) = interpret_source("repl", &buffer, &mut host) {
            print_run_source_error(&error);
        }
    }
}

#[cfg(test)]
#[path = "script_test.rs"]
mod tests;
