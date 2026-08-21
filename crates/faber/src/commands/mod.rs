//! Command handlers for the `faber` binary.
//!
//! Package and radix-delegated commands are dispatched here; Faber-specific
//! handlers live in [`init`], [`explain`], [`run`], and [`test`].

mod archive;
mod check_library;
#[cfg(test)]
#[path = "check_library_test.rs"]
mod check_library_test;
#[cfg(test)]
#[path = "check_test.rs"]
mod check_test;
mod convert;
mod doctor;
#[cfg(test)]
#[path = "doctor_test.rs"]
mod doctor_test;
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
use radix_module::services::{
    BuildCommand, CheckCommand, DiagnosticMode, EmitCommand, VerifyCommand,
};

use check_library::cmd_check_library;
use convert::cmd_convert;
use doctor::cmd_doctor;
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
                        .unwrap_or(crate::cli::FaberCliTarget::HirRust)
                        .to_radix(),
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
        Command::CheckLibrary(args) => cmd_check_library(args),
        Command::Doctor => cmd_doctor(),
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
                cmd_check_file(CheckCommand {
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
            cmd_verify_file(VerifyCommand {
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
        Command::Lex(args) => cmd_inspect_lex(&args.input),
        Command::Parse(args) => cmd_inspect_parse(&args.input),
        Command::Hir(args) => cmd_inspect_hir(&args.input, args.load),
        Command::Mir(args) => cmd_inspect_mir(&args.input),
        Command::CliIr(args) => cmd_inspect_cli_ir(&args.input),
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

fn print_inspection(result: radix_module::services::SourceInspection) {
    if !result.text.is_empty() {
        print!("{}", result.text);
    }
    for error in &result.errors {
        eprintln!("{error}");
    }
    if !result.success {
        std::process::exit(1);
    }
}

fn read_source(args: &[String]) -> (String, String) {
    if args.is_empty() || args[0] == "-" {
        let mut source = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut source).unwrap_or_else(|err| {
            eprintln!("error: failed to read stdin: {err}");
            std::process::exit(1);
        });
        ("<stdin>".to_owned(), source)
    } else {
        let path = std::path::PathBuf::from(&args[0]);
        let source = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            eprintln!("error: cannot read '{}': {}", path.display(), e);
            std::process::exit(1);
        });
        (args[0].clone(), source)
    }
}

fn cmd_inspect_lex(input: &[String]) {
    let (name, source) = read_source(input);
    print_inspection(radix_module::services::lex_inspection_for_source(
        &name, &source,
    ));
}

fn cmd_inspect_parse(input: &[String]) {
    let (name, source) = read_source(input);
    print_inspection(radix_module::services::parse_inspection_for_source(
        &name, &source,
    ));
}

fn cmd_inspect_hir(input: &[String], load: Option<std::path::PathBuf>) {
    if let Some(path) = load {
        eprintln!("error: `faber hir --load` is not supported; use `radix hir --load`");
        drop(path);
        std::process::exit(1);
    }
    let (name, source) = read_source(input);
    print_inspection(radix_module::services::hir_inspection_for_source(
        &name, &source,
    ));
}

fn cmd_inspect_mir(input: &[String]) {
    let (name, source) = read_source(input);
    match radix_module::services::mir_output_for_source(&name, &source) {
        Ok(output) => print!("{output}"),
        Err(messages) => {
            for message in messages {
                eprintln!("{message}");
            }
            std::process::exit(1);
        }
    }
}

fn cmd_inspect_cli_ir(input: &[String]) {
    let (name, source) = read_source(input);
    print_inspection(radix_module::services::cli_ir_inspection_for_source(
        &name, &source,
    ));
}

fn cmd_check_file(command: CheckCommand) {
    if command.package {
        eprintln!("error: package checking is owned by the `faber` tool; rerun with `faber check --package`");
        std::process::exit(1);
    }
    let (name, source) = read_source(&command.input);
    let mut result =
        radix_module::services::check_diagnostics_for_source(&name, &source, command.permissive);
    let warn_policy = radix_module::driver::WarnPolicy {
        deny_all_warnings: command.deny_warnings,
        deny_codes: command.deny_codes.clone(),
    };
    radix_module::driver::apply_warn_policy(&mut result.diagnostics, &warn_policy);
    crate::postprocess::print_diagnostics(&result.diagnostics, command.diagnostic_mode, None);
    let success = result.success
        && !result
            .diagnostics
            .iter()
            .any(radix_module::diagnostics::Diagnostic::is_error);
    if success {
        eprintln!("ok: {name}");
    } else {
        std::process::exit(1);
    }
}

fn cmd_verify_file(command: VerifyCommand) {
    let (name, source) = read_source(&command.input);
    let result = radix_module::services::verify_source(&name, &source, None);
    if !result.diagnostics.is_empty() {
        eprint!(
            "{}",
            radix_module::diagnostics::render_expanded_diagnostics(&result.diagnostics)
        );
    }
    if let Some(coverage) = &result.coverage {
        print!(
            "{}",
            radix_module::aspect::render_hir_aspect_coverage(coverage)
        );
    }
    if !result.success {
        std::process::exit(1);
    }
}
