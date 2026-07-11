//! Command handlers for the `faber` binary.
//!
//! Package and radix-delegated commands are dispatched here; Faber-specific
//! handlers live in [`init`], [`explain`], [`run`], and [`test`].

mod archive;
mod emit;
mod explain;
mod format;
#[cfg(test)]
#[path = "format_test.rs"]
mod format_test;
pub mod host;
mod init;
mod install;
#[cfg(test)]
#[path = "install_test.rs"]
mod install_test;
mod run;
mod script;
mod test;

use crate::cli::Command;
use crate::input_shape::{reader_locale_without_package_error, verify_input_is_package_shaped};
use crate::package;
use clap::Parser;
use radix::tool::{self, BuildCommand, CheckCommand, DiagnosticMode, EmitCommand, VerifyCommand};

use explain::cmd_explain;
use format::cmd_format;
use host::cmd_host;
use init::cmd_init;
use install::cmd_install;
use run::{cmd_fmir_run_image, cmd_run};
use script::{cmd_eval, cmd_repl, cmd_script};
use test::cmd_test;

fn diagnostic_mode(enabled: bool) -> DiagnosticMode {
    if enabled {
        DiagnosticMode::Diagnostics
    } else {
        DiagnosticMode::Normal
    }
}

/// Print package compile diagnostics in the normal CLI style.
pub(super) fn eprint_compile_diagnostics(diagnostics: &[radix::diagnostics::Diagnostic]) {
    for diag in diagnostics {
        if diag.is_error() {
            eprintln!("error: {}", diag.message);
        } else {
            eprintln!("warning: {}", diag.message);
        }
    }
}

/// Parse argv and dispatch to the selected command handler.
pub fn run() {
    let cli = crate::cli::Cli::parse();
    if let Some(source) = cli.eval_source {
        cmd_eval(source, cli.eval_args);
        return;
    }
    let Some(command) = cli.command else {
        eprintln!("error: no subcommand provided; use `faber --help` for usage");
        std::process::exit(1);
    };
    dispatch(command);
}

fn dispatch(command: Command) {
    match command {
        Command::Build(args) => {
            reject_reader_locale_without_package(
                args.reader_locale.as_deref(),
                std::slice::from_ref(&args.input),
                args.package,
            );
            let target_explicit = args.target.is_some();
            package::cmd_build(BuildCommand {
                input: args.input,
                out_dir: args.out_dir,
                package: args.package,
                release: args.release,
                target: args.target.unwrap_or(radix::tool::CliTarget::Rust).into(),
                target_explicit,
                format: args.format,
                linter: args.linter,
                reader_locale: args.reader_locale,
            })
        }
        Command::Targets => tool::cmd_targets(),
        Command::Check(args) => {
            reject_reader_locale_without_package(
                args.reader_locale.as_deref(),
                &args.input,
                args.package,
            );
            if args.package || package::should_treat_as_package_from_args(&args.input) {
                package::cmd_check_package(CheckCommand {
                    input: args.input,
                    package: args.package,
                    permissive: args.permissive,
                    diagnostic_mode: diagnostic_mode(args.diagnostics),
                    reader_pack: None,
                    reader_locale: args.reader_locale,
                });
            } else {
                reject_reader_locale_without_package(
                    args.reader_locale.as_deref(),
                    &args.input,
                    args.package,
                );
                tool::cmd_check(CheckCommand {
                    input: args.input,
                    package: args.package,
                    permissive: args.permissive,
                    diagnostic_mode: diagnostic_mode(args.diagnostics),
                    reader_pack: None,
                    reader_locale: None,
                });
            }
        }
        Command::Verify(args) => {
            if verify_input_is_package_shaped(&args.input, args.package) {
                eprintln!(
                    "error: package verification is not supported yet; use `faber verify <single-file.fab>`"
                );
                std::process::exit(1);
            }
            tool::cmd_verify(VerifyCommand {
                input: args.input,
                package: args.package,
            });
        }
        Command::VerifyLibrary(args) => cmd_verify_library(args),
        Command::Init(args) => cmd_init(args),
        Command::Install(args) => cmd_install(args),
        Command::Explain(args) => cmd_explain(args),
        Command::Run(args) => cmd_run(args),
        Command::FmirRun(args) => cmd_fmir_run_image(args),
        Command::Script(args) => cmd_script(args),
        Command::Repl(args) => cmd_repl(args),
        Command::Test(args) => cmd_test(args),
        Command::Lex(args) => tool::cmd_lex(&args.input),
        Command::Parse(args) => tool::cmd_parse(&args.input),
        Command::Hir(args) => tool::cmd_hir(&args.input),
        Command::CliIr(args) => tool::cmd_cli_ir(&args.input),
        Command::Emit(args) => {
            let emit_command = EmitCommand {
                input: args.input,
                package: args.package,
                target: args.target.to_radix(),
                format: args.format,
                linter: args.linter,
                reflection: args.reflection,
                output: args.output,
                diagnostic_mode: diagnostic_mode(args.diagnostics),
                reader_pack: None,
                reader_locale: args.reader_locale,
            };
            if emit::is_faber_emit(args.target) {
                emit::cmd_emit_faber(emit_command);
            } else if package::use_package_compiler_from_args(
                emit_command.target,
                &emit_command.input,
                args.package,
            ) {
                reject_reader_locale_without_package(
                    emit_command.reader_locale.as_deref(),
                    &emit_command.input,
                    emit_command.package,
                );
                package::cmd_emit_package(emit_command);
            } else {
                reject_reader_locale_without_package(
                    emit_command.reader_locale.as_deref(),
                    &emit_command.input,
                    emit_command.package,
                );
                tool::cmd_emit(emit_command);
            }
        }
        Command::Format(args) => cmd_format(format::FormatCommand {
            paths: args.paths,
            canonical: args.canonical,
            reader_locale: args.reader_locale,
            check: args.check,
            stdout: args.stdout,
            config: args.config,
        }),
        Command::Host(args) => cmd_host(args.command),
    }
}

fn cmd_verify_library(args: crate::cli::VerifyLibraryArgs) {
    let root = if args
        .input
        .file_name()
        .is_some_and(|name| name == "faber.toml")
    {
        args.input
            .parent()
            .map(std::path::Path::to_path_buf)
            .unwrap_or_else(|| std::path::PathBuf::from("."))
    } else {
        args.input
    };
    match package::verify_library_bindings(&root, &args.target) {
        Ok(report) => {
            println!(
                "ok: verified {} declarations and {} bindings for target {}",
                report.declarations, report.bindings, args.target
            );
        }
        Err(diagnostics) => {
            eprint_compile_diagnostics(&diagnostics);
            std::process::exit(1);
        }
    }
}

fn reject_reader_locale_without_package(
    reader_locale: Option<&str>,
    input: &[String],
    force_package: bool,
) {
    if let Some(message) = reader_locale_without_package_error(reader_locale, input, force_package)
    {
        eprintln!("error: {message}");
        std::process::exit(1);
    }
}
