//! Command handlers for the `faber` binary.
//!
//! Package and radix-delegated commands are dispatched here; Faber-specific
//! handlers live in [`init`], [`explain`], [`run`], and [`test`].

mod archive;
mod convert;
mod emit;
mod explain;
mod format;
#[cfg(test)]
#[path = "format_corpus_test.rs"]
mod format_corpus_test;
#[cfg(test)]
#[path = "format_test.rs"]
pub(crate) mod format_test;
pub mod host;
pub(crate) mod init;
mod model;
mod run;
pub(crate) mod self_update;
#[cfg(test)]
#[path = "self_update_test.rs"]
mod self_update_test;
pub(crate) mod targets;
#[cfg(feature = "runner")]
mod test;

use crate::cli::Command;
use crate::input_shape::{locale_without_package_error, verify_input_is_package_shaped};
use crate::package;
use clap::Parser;
use radix_module::tool::{
    self, BuildCommand, CheckCommand, DiagnosticMode, EmitCommand, VerifyCommand,
};

use convert::cmd_convert;
use explain::cmd_explain;
use format::cmd_format;
use host::cmd_host;
use init::cmd_init;
use model::cmd_model;
use run::{cmd_eval, cmd_fmir_run_image, cmd_repl, cmd_run};
use self_update::cmd_self;
use targets::cmd_targets;
#[cfg(feature = "runner")]
use test::cmd_test;

fn diagnostic_mode(enabled: bool) -> DiagnosticMode {
    if enabled {
        DiagnosticMode::Diagnostics
    } else {
        DiagnosticMode::Normal
    }
}

/// Validate that every `--deny <CODE>` entry is a known diagnostic catalog code.
/// Exits with a clear message on the first unknown code.
pub(super) fn validate_deny_codes(codes: &[String]) {
    for code in codes {
        if !radix_module::is_known_diagnostic_code(code) {
            eprintln!("error: unknown diagnostic code '{code}' passed to --deny");
            std::process::exit(1);
        }
    }
}

/// Print package compile diagnostics in the normal CLI style.
pub(super) fn eprint_compile_diagnostics(diagnostics: &[radix_module::diagnostics::Diagnostic]) {
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
        cmd_eval(&source, cli.eval_args);
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
                args.locale.as_deref(),
                std::slice::from_ref(&args.input),
                args.package,
            );
            let target_explicit = args.target.is_some();
            validate_deny_codes(&args.deny);
            package::cmd_build(
                BuildCommand {
                    input: args.input,
                    out_dir: args.out_dir,
                    package: args.package,
                    release: args.release,
                    target: args
                        .target
                        .unwrap_or(radix_module::tool::CliTarget::HirRust)
                        .into(),
                    target_explicit,
                    deny_warnings: args.deny_warnings,
                    deny_codes: args.deny,
                    locale: args.locale,
                    diagnostics_locale: args.diagnostics_locale,
                },
                args.format,
                args.linter,
            );
        }
        Command::Targets => cmd_targets(),
        Command::Check(args) => {
            reject_reader_locale_without_package(args.locale.as_deref(), &args.input, args.package);
            validate_deny_codes(&args.deny);
            if args.package || package::should_treat_as_package_from_args(&args.input) {
                package::cmd_check_package(CheckCommand {
                    input: args.input,
                    package: args.package,
                    permissive: args.permissive,
                    deny_warnings: args.deny_warnings,
                    deny_codes: args.deny,
                    diagnostic_mode: diagnostic_mode(args.diagnostics),
                    diagnostics_locale_pack: None,
                    locale: args.locale,
                    diagnostics_locale: args.diagnostics_locale,
                });
            } else {
                tool::cmd_check(CheckCommand {
                    input: args.input,
                    package: args.package,
                    permissive: args.permissive,
                    deny_warnings: args.deny_warnings,
                    deny_codes: args.deny,
                    diagnostic_mode: diagnostic_mode(args.diagnostics),
                    diagnostics_locale_pack: None,
                    locale: None,
                    diagnostics_locale: args.diagnostics_locale,
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
        Command::Explain(args) => cmd_explain(args),
        Command::Run(args) => cmd_run(args),
        Command::FmirRun(args) => cmd_fmir_run_image(args),
        Command::Repl(args) => cmd_repl(args),
        #[cfg(feature = "runner")]
        Command::Test(args) => cmd_test(&args),
        #[cfg(not(feature = "runner"))]
        Command::Test(_) => {
            eprintln!("error: test requires the runner feature");
            std::process::exit(1);
        }
        Command::Lex(args) => tool::cmd_lex(&args.input),
        Command::Parse(args) => tool::cmd_parse(&args.input),
        Command::Hir(args) => tool::cmd_hir(&args.input, None),
        Command::Mir(args) => tool::cmd_mir(&args.input),
        Command::CliIr(args) => tool::cmd_cli_ir(&args.input),
        Command::Emit(args) => {
            let emit_command = EmitCommand {
                input: args.input,
                package: args.package,
                target: args.target.to_radix(),
                reflection: args.reflection,
                output: args.output,
                diagnostic_mode: diagnostic_mode(args.diagnostics),
                diagnostics_locale_pack: None,
                locale: args.locale,
                diagnostics_locale: args.diagnostics_locale,
                output_mode: radix_module::codegen::OutputMode::Application,
                module_name: None,
                #[cfg(feature = "mir-llvm")]
                cuda_descriptor: None,
            };
            if emit::is_faber_emit(args.target) {
                emit::cmd_emit_faber(emit_command, args.format, args.linter);
            } else if package::use_package_compiler_from_args(
                emit_command.target,
                &emit_command.input,
                args.package,
            ) {
                reject_reader_locale_without_package(
                    emit_command.locale.as_deref(),
                    &emit_command.input,
                    emit_command.package,
                );
                package::cmd_emit_package(emit_command, args.format, args.linter);
            } else {
                reject_reader_locale_without_package(
                    emit_command.locale.as_deref(),
                    &emit_command.input,
                    emit_command.package,
                );
                emit::cmd_emit_with_locale(emit_command, args.format, args.linter);
            }
        }
        Command::Convert(args) => cmd_convert(&convert::ConvertCommand {
            paths: args.paths,
            to: args.to,
            from: args.from,
            check: args.check,
            stdout: args.stdout,
            stdin: args.stdin,
        }),
        Command::Format(args) => cmd_format(&format::FormatCommand {
            paths: args.paths,
            check: args.check,
            stdout: args.stdout,
            write: args.write,
            stdin: args.stdin,
            policy: args.policy,
            config: args.config,
        }),
        Command::Host(args) => cmd_host(args.command),
        Command::Model(args) => cmd_model(args.command),
        Command::SelfManage(args) => cmd_self(&args),
    }
}

fn cmd_verify_library(args: crate::cli::VerifyLibraryArgs) {
    let root = if args
        .input
        .file_name()
        .is_some_and(|name| name == "faber.toml")
    {
        args.input.parent().map_or_else(
            || std::path::PathBuf::from("."),
            std::path::Path::to_path_buf,
        )
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
    locale: Option<&str>,
    input: &[String],
    force_package: bool,
) {
    if let Some(message) = locale_without_package_error(locale, input, force_package) {
        eprintln!("error: {message}");
        std::process::exit(1);
    }
}
