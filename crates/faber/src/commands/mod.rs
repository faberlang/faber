//! Command handlers for the `faber` binary.
//!
//! Package and radix-delegated commands are dispatched here; Faber-specific
//! handlers live in [`init`], [`explain`], [`run`], and [`test`].

mod archive;
mod check_air;
#[cfg(test)]
#[path = "check_air_test.rs"]
mod check_air_test;
mod check_library;
#[cfg(test)]
#[path = "check_library_test.rs"]
mod check_library_test;
#[cfg(test)]
#[path = "check_test.rs"]
mod check_test;
mod convert;
mod corpus;
mod cost;
#[cfg(test)]
#[path = "cost_test.rs"]
mod cost_test;
mod doc;
mod doctor;
#[cfg(test)]
#[path = "doctor_test.rs"]
mod doctor_test;
mod emit;
#[cfg(test)]
#[path = "emit_test.rs"]
mod emit_test;
mod explain;
mod field_modifier_codemod;
mod format;
#[cfg(test)]
#[path = "format_corpus_test.rs"]
mod format_corpus_test;
#[cfg(test)]
#[path = "format_test.rs"]
pub(crate) mod format_test;
pub mod host;
pub(crate) mod init;
#[cfg(test)]
#[path = "locale_lanes_test.rs"]
mod locale_lanes_test;
#[cfg(test)]
#[path = "mir_dump_test.rs"]
mod mir_dump_test;
mod model;
#[cfg(test)]
#[path = "regex_literal_format_test.rs"]
mod regex_literal_format_test;
#[cfg(feature = "runner")]
mod run;
#[cfg(feature = "runner")]
mod run_stats;
#[cfg(all(feature = "runner", test))]
#[path = "run_stats_test.rs"]
mod run_stats_test;
#[cfg(all(feature = "runner", feature = "device-runtime", test))]
mod span_a2_hw_test;
pub(crate) mod targets;
#[cfg(feature = "runner")]
mod test;
#[cfg(feature = "runner")]
mod test_stats;
#[cfg(all(feature = "runner", test))]
#[path = "test_stats_test.rs"]
mod test_stats_test;
#[cfg(test)]
#[path = "typed_zero_format_test.rs"]
mod typed_zero_format_test;
pub(crate) mod update;
#[cfg(test)]
#[path = "update_test.rs"]
mod update_test;

use crate::cli::Command;
use faber_cli::exit_status;
use faber_cli::input_shape::{locale_without_package_error, verify_input_is_package_shaped};
use faber_cli::package;
use radix::services::{BuildCommand, CheckCommand, DiagnosticMode, EmitCommand, VerifyCommand};

use check_library::cmd_check_library;
use convert::cmd_convert;
use corpus::cmd_corpus;
use cost::cmd_cost;
use doc::cmd_doc;
use doctor::cmd_doctor;
use explain::cmd_explain;
use field_modifier_codemod::cmd_field_modifier_codemod;
use format::cmd_format;
use host::cmd_host;
use init::cmd_init;
use model::cmd_model;
#[cfg(feature = "runner")]
use run::{cmd_eval, cmd_fmir_run_image, cmd_repl, cmd_run};
use targets::cmd_targets;
#[cfg(feature = "runner")]
use test::cmd_test;
use update::{cmd_uninstall, cmd_update, cmd_versions};

pub(crate) const PACKAGE_CHECK_MESSAGE: &str =
    "error: package checking is owned by the `faber` tool; rerun with `faber check --package`";

#[cfg(not(feature = "runner"))]
fn exit_runner_feature_unavailable(command: &str) -> ! {
    eprintln!("error: {command} requires the runner feature; rebuild with feature `runner`");
    exit_status::exit_unavailable();
}

/// `faber check --air --json` machine rows (schema version 2).
///
/// One JSON object per report row, one row per line. The contract is
/// versioned from day one (`schema_version`): the near-miss contract is meant
/// for LLM/tooling consumption and must not churn silently. Version 2 splits
/// the admissible verdict into the census tier labels; the key set is
/// otherwise unchanged from version 1. The whole report is computed first;
/// rows are emitted as complete lines, not streamed.
mod air_json {
    use super::check_air::AirRow;
    use serde::Serialize;

    /// Schema version of the advisory AIR `--json` rows.
    pub(super) const AIR_SCHEMA_VERSION: u32 = 2;

    #[derive(Serialize)]
    pub(super) struct AirRowJson {
        schema_version: u32,
        name: String,
        verdict: String,
        annotated: bool,
        reasons: Vec<AirReasonJson>,
        note: Option<String>,
    }

    #[derive(Serialize)]
    pub(super) struct AirReasonJson {
        rule: String,
        span: AirSpanJson,
        via_callee: bool,
    }

    /// Half-open byte span of the offending construct.
    #[derive(Serialize)]
    pub(super) struct AirSpanJson {
        start: u32,
        end: u32,
    }

    pub(super) fn render_json_rows(
        out: &mut dyn std::io::Write,
        rows: &[AirRow],
    ) -> std::io::Result<()> {
        for row in rows {
            let view = AirRowJson {
                schema_version: AIR_SCHEMA_VERSION,
                name: row.name.clone(),
                verdict: row.verdict.census_label(row.annotated).to_owned(),
                annotated: row.annotated,
                reasons: row
                    .reasons
                    .iter()
                    .map(|reason| AirReasonJson {
                        rule: reason.rule.clone(),
                        span: AirSpanJson {
                            start: reason.start,
                            end: reason.end,
                        },
                        via_callee: reason.via_callee,
                    })
                    .collect(),
                note: row.note.clone(),
            };
            let json = serde_json::to_string(&view).map_err(std::io::Error::other)?;
            writeln!(out, "{json}")?;
        }
        Ok(())
    }
}

fn diagnostic_mode(no_diagnostics: bool) -> DiagnosticMode {
    if no_diagnostics {
        DiagnosticMode::Normal
    } else {
        DiagnosticMode::Diagnostics
    }
}

/// Validate that every `--deny <CODE>` entry is a known diagnostic catalog code.
/// Exits with a clear message on the first unknown code.
pub(super) fn validate_deny_codes(codes: &[String]) {
    for code in codes {
        if !radix::module::is_known_diagnostic_code(code) {
            eprintln!("error: unknown diagnostic code '{code}' passed to --deny");
            exit_status::exit_usage();
        }
    }
}

/// Print package compile diagnostics in the normal CLI style.
pub(super) fn eprint_compile_diagnostics(diagnostics: &[radix::module::diagnostics::Diagnostic]) {
    for diag in diagnostics {
        // A diagnostic that mapped a physical source line names `file:line`;
        // byte-offset-only spans stay off the plain surface (they are not
        // lines and would mislead).
        let location = if diag.line.is_some() {
            format!("{}: ", diag.display_location())
        } else {
            String::new()
        };
        if diag.is_error() {
            eprintln!("error: {location}{}", diag.message);
        } else {
            eprintln!("warning: {location}{}", diag.message);
        }
    }
}

/// Exit with a structured missing-path diagnostic (U1 command-boundary contract).
///
/// `Diagnostic::io_error` keeps the absolutized path out of the message so the
/// renderer names it once, and carries `path` / `base` / `artifact` as facts.
/// This is the read side: the exit status is `NO_INPUT`.
pub(super) fn eprint_io_error(path: &std::path::Path, err: &std::io::Error) -> ! {
    let path = package::absolutize_path(path);
    let diagnostic = radix::module::diagnostics::Diagnostic::io_error(&path, err);
    eprintln!("{}", radix::module::diagnostics::render_plain(&diagnostic));
    exit_status::exit_for_diagnostic(&diagnostic);
}

/// Exit with a structured write-failure diagnostic: an output file, a
/// directory or a standard stream that could not be written. The exit status is
/// `CANT_CREATE`.
pub(super) fn eprint_write_error(path: &std::path::Path, err: &std::io::Error) -> ! {
    let diagnostic = exit_status::write_diagnostic_error(path, err);
    eprintln!("{}", radix::module::diagnostics::render_plain(&diagnostic));
    exit_status::exit_for_diagnostic(&diagnostic);
}

/// Parse argv and dispatch to the selected command handler.
pub fn run() {
    let cli = exit_status::parse_cli::<crate::cli::Cli>();
    #[cfg(feature = "runner")]
    if let Some(source) = cli.eval_source {
        cmd_eval(&source, cli.eval_args);
        return;
    }
    #[cfg(not(feature = "runner"))]
    if cli.eval_source.is_some() {
        exit_runner_feature_unavailable("-c/--command");
    }
    let Some(command) = cli.command else {
        eprintln!("error: no subcommand provided; use `faber --help` for usage");
        exit_status::exit_usage();
    };
    dispatch(command);
}

/// Refuse an engine that runs in memory (`clif-jit`, `runner`) where an
/// artifact is asked for: neither compiles an output to write.
fn reject_in_memory_engine(target: Option<crate::cli::FaberCliTarget>) {
    let Some(target) = target.filter(|target| target.is_in_memory_engine()) else {
        return;
    };
    match target {
        crate::cli::FaberCliTarget::ClifJit => {
            eprintln!("error: clif-jit runs in memory; use --target clif to build an executable");
        }
        crate::cli::FaberCliTarget::Runner => {
            eprintln!(
                "error: runner is the in-memory interpreter; build needs a compiled target (see `faber targets`)"
            );
        }
        _ => {}
    }
    exit_status::exit_usage();
}

fn dispatch(command: Command) {
    let (flag, probe) = command.library_home_inputs();
    faber_cli::library::configure_library_home(flag.as_deref(), probe.as_deref());
    if command.installs_embedded_library_fallback() {
        faber_cli::library::configure_embedded_library_home_if_unset();
    }
    match command {
        Command::Build(args) => {
            reject_in_memory_engine(args.target);
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
                    diagnostic_mode: diagnostic_mode(args.no_diagnostics),
                },
                args.format,
                args.linter,
                args.safe,
            );
        }
        Command::Corpus(args) => cmd_corpus(&args),
        Command::Targets => cmd_targets(),
        Command::CheckLibrary(args) => cmd_check_library(args),
        Command::Doctor(_) => cmd_doctor(),
        Command::Doc(args) => cmd_doc(&doc::DocCommand {
            input: args.input,
            json: args.json,
        }),
        Command::Cost(args) => {
            reject_reader_locale_without_package(args.locale.as_deref(), &args.input, args.package);
            cmd_cost(args);
        }
        Command::Check(args) => cmd_check(args),
        Command::Verify(args) => {
            if verify_input_is_package_shaped(&args.input, args.package) {
                eprintln!(
                    "error: package verification is not supported yet; use `faber verify <single-file.fab>`"
                );
                exit_status::exit_usage();
            }
            cmd_verify_file(VerifyCommand {
                input: args.input,
                package: args.package,
            });
        }
        Command::VerifyLibrary(args) => cmd_verify_library(args),
        Command::Init(args) => cmd_init(args),
        Command::Explain(args) => cmd_explain(args),
        #[cfg(feature = "runner")]
        Command::Run(args) => cmd_run(args),
        #[cfg(not(feature = "runner"))]
        Command::Run(_) => exit_runner_feature_unavailable("run"),
        #[cfg(feature = "runner")]
        Command::FmirRun(args) => cmd_fmir_run_image(args),
        #[cfg(not(feature = "runner"))]
        Command::FmirRun(_) => exit_runner_feature_unavailable("__fmir-run"),
        #[cfg(feature = "runner")]
        Command::Repl(args) => cmd_repl(args),
        #[cfg(not(feature = "runner"))]
        Command::Repl(_) => exit_runner_feature_unavailable("repl"),
        #[cfg(feature = "runner")]
        Command::Test(args) => cmd_test(&args),
        #[cfg(not(feature = "runner"))]
        Command::Test(_) => {
            eprintln!("error: test requires the runner feature");
            exit_status::exit_unavailable();
        }
        Command::Lex(args) => cmd_inspect_lex(&args.input),
        Command::Parse(args) => cmd_inspect_parse(&args.input),
        Command::Hir(args) => cmd_inspect_hir(&args.input, args.load),
        #[cfg(feature = "runner")]
        Command::Mir(args) => cmd_inspect_mir(&args.input),
        #[cfg(not(feature = "runner"))]
        Command::Mir(_) => exit_runner_feature_unavailable("mir"),
        Command::CliIr(args) => cmd_inspect_cli_ir(&args.input),
        Command::Emit(args) => {
            reject_in_memory_engine(Some(args.target));
            let emit_command = EmitCommand {
                input: args.input,
                package: args.package,
                target: args.target.to_radix(),
                reflection: args.reflection,
                output: args.output,
                diagnostic_mode: diagnostic_mode(args.no_diagnostics),
                diagnostics_locale_pack: None,
                locale: args.locale,
                diagnostics_locale: args.diagnostics_locale,
                output_mode: radix::module::codegen::OutputMode::Application,
                module_name: None,
                #[cfg(feature = "mir-llvm")]
                cuda_descriptor: None,
            };
            if !package::target_gate::target_compiled_in(emit_command.target) {
                package::target_gate::exit_target_not_in_this_build(emit_command.target);
            }
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
            diagnostic_mode: diagnostic_mode(args.no_diagnostics),
            paths: args.paths,
            to: args.to,
            from: args.from,
            check: args.check,
            stdout: args.stdout,
            write: args.write,
            stdin: args.stdin,
        }),
        Command::Format(args) => cmd_format(&format::FormatCommand {
            diagnostic_mode: diagnostic_mode(args.no_diagnostics),
            paths: args.paths,
            check: args.check,
            stdout: args.stdout,
            write: args.write,
            stdin: args.stdin,
            width: args.width.and_then(std::num::NonZeroU32::new),
        }),
        Command::FieldModifierCodemod(args) => {
            cmd_field_modifier_codemod(&field_modifier_codemod::FieldModifierCodemodCommand {
                paths: args.paths,
                check: args.check,
                stdout: args.stdout,
                write: args.write,
            });
        }
        Command::Host(args) => cmd_host(args.command),
        Command::Model(args) => cmd_model(args.command),
        Command::Update(args) => cmd_update(&args),
        Command::Uninstall(args) => cmd_uninstall(&args),
        Command::Versions(args) => cmd_versions(&args),
    }
}

fn cmd_check(args: crate::cli::CheckArgs) {
    reject_reader_locale_without_package(args.locale.as_deref(), &args.input, args.package);
    validate_deny_codes(&args.deny);
    let package_shaped = args.package || package::should_treat_as_package_from_args(&args.input);
    if args.lower && !package_shaped {
        eprintln!("error: `check --lower` needs a file or package path, not stdin");
        exit_status::exit_usage();
    }
    if args.device.is_some() && !package_shaped {
        eprintln!("error: `check --device` needs a file or package path, not stdin");
        exit_status::exit_usage();
    }
    if args.air {
        // ALA-1: the advisory AIR report is single-file only. Plain
        // `check` routes bare `.fab` files through the package loader;
        // under `--air` a plain file stays in file mode so the report
        // rides the single-file pipeline. Directory/manifest-shaped
        // inputs stay unsupported.
        let input_is_plain_file = if args.package {
            false
        } else if package_shaped {
            !args.input.is_empty() && std::path::Path::new(&args.input[0]).is_file()
        } else {
            true
        };
        if !input_is_plain_file {
            let message = check_air::PACKAGE_AIR_MESSAGE;
            eprintln!("{message}");
            exit_status::exit_usage();
        }
        cmd_check_file(
            CheckCommand {
                input: args.input,
                package: args.package,
                permissive: args.permissive,
                deny_warnings: args.deny_warnings,
                deny_codes: args.deny,
                diagnostic_mode: diagnostic_mode(args.no_diagnostics),
                diagnostics_locale_pack: None,
                locale: None,
                diagnostics_locale: args.diagnostics_locale,
            },
            true,
            args.json,
        );
    } else if package_shaped {
        package::cmd_check_package(
            CheckCommand {
                input: args.input,
                package: args.package,
                permissive: args.permissive,
                deny_warnings: args.deny_warnings,
                deny_codes: args.deny,
                diagnostic_mode: diagnostic_mode(args.no_diagnostics),
                diagnostics_locale_pack: None,
                locale: args.locale,
                diagnostics_locale: args.diagnostics_locale,
            },
            args.complexity_budget,
            args.kernel_complexity_budget,
            args.lower,
            args.device.map(|device| package::DeviceAdviceRequest {
                device,
                partitions: args.partitions,
            }),
        );
    } else {
        cmd_check_file(
            CheckCommand {
                input: args.input,
                package: args.package,
                permissive: args.permissive,
                deny_warnings: args.deny_warnings,
                deny_codes: args.deny,
                diagnostic_mode: diagnostic_mode(args.no_diagnostics),
                diagnostics_locale_pack: None,
                locale: None,
                diagnostics_locale: args.diagnostics_locale,
            },
            false,
            false,
        );
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
            exit_status::exit_for_diagnostics(&diagnostics);
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
        exit_status::exit_usage();
    }
}

fn print_inspection(result: radix::services::SourceInspection) {
    if !result.text.is_empty() {
        print!("{}", result.text);
    }
    for error in &result.errors {
        eprintln!("{error}");
    }
    if !result.success {
        exit_status::exit_data_error();
    }
}

fn read_source(args: &[String]) -> (String, String) {
    if args.is_empty() || args[0] == "-" {
        let mut source = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut source).unwrap_or_else(|err| {
            eprintln!("error: failed to read stdin: {err}");
            exit_status::exit_no_input();
        });
        ("<stdin>".to_owned(), source)
    } else {
        let path = std::path::PathBuf::from(&args[0]);
        let source = std::fs::read_to_string(&path).unwrap_or_else(|e| eprint_io_error(&path, &e));
        (args[0].clone(), source)
    }
}

fn cmd_inspect_lex(input: &[String]) {
    let (name, source) = read_source(input);
    print_inspection(radix::services::lex_inspection_for_source_with_config(
        &name,
        &source,
        &single_file_locale_config(input),
    ));
}

fn cmd_inspect_parse(input: &[String]) {
    let (name, source) = read_source(input);
    print_inspection(radix::services::parse_inspection_for_source_with_config(
        &name,
        &source,
        &single_file_locale_config(input),
    ));
}

fn cmd_inspect_hir(input: &[String], load: Option<std::path::PathBuf>) {
    if load.is_some() {
        eprintln!("error: `faber hir --load` is not supported; use `radix hir --load`");
        exit_status::exit_usage();
    }
    let (name, source) = read_source(input);
    print_inspection(radix::services::hir_inspection_for_source_with_config(
        &name,
        &source,
        &single_file_locale_config(input),
    ));
}

#[cfg(feature = "runner")]
fn cmd_inspect_mir(input: &[String]) {
    let (name, source) = read_source(input);
    match radix::services::mir_output_for_source_with_config(&name, &source, mir_dump_config(input))
    {
        Ok(output) => print!("{output}"),
        Err(messages) => {
            for message in messages {
                eprintln!("{message}");
            }
            exit_status::exit_data_error();
        }
    }
}

/// Reader-locale config for `faber mir`, the same selection `check`, `build`
/// and `run` make ([`single_file_locale_config`]): the owning manifest's
/// locale, else the product default `en`; a file's own frontmatter `locale`
/// still wins during analysis.
#[cfg(feature = "runner")]
fn mir_dump_config(input: &[String]) -> radix::module::Config {
    single_file_locale_config(input)
}

fn cmd_inspect_cli_ir(input: &[String]) {
    let (name, source) = read_source(input);
    print_inspection(radix::services::cli_ir_inspection_for_source_with_config(
        &name,
        &source,
        &single_file_locale_config(input),
    ));
}

/// Reader-locale config for the single-file read lanes (`check`, `lex`,
/// `parse`, `hir`, `cli-ir`, `verify`): the owning manifest's locale, else
/// the product default `en`, plus the embedded reader packs as the
/// frontmatter-locale loader — a file's own `+++ locale = "…" +++` still
/// wins during analysis, and an installed binary resolves it with no
/// workspace `locale/` tree beside it.
fn single_file_locale_config(input: &[String]) -> radix::module::Config {
    let target = radix::module::Config::default().target;
    let result = if input.is_empty() || input[0] == "-" {
        package::default_config_with_locale(target)
    } else {
        let path = package::absolutize_path(std::path::Path::new(&input[0]));
        package::config_with_locale(target, &path, None, None).map(|(config, _)| config)
    };
    match result {
        Ok(config) => config.with_dev_locale_root(),
        Err(diagnostic) => {
            let diagnostic = package::attach_path_diagnostic_context(diagnostic);
            eprintln!("{}", radix::module::diagnostics::render_plain(&diagnostic));
            exit_status::exit_for_diagnostic(&diagnostic);
        }
    }
}

fn cmd_check_file(command: CheckCommand, air: bool, json: bool) {
    if command.package {
        // Cross-reference: must stay verbatim with
        // crates/radix-cli/src/commands/check.rs `PACKAGE_CHECK_MESSAGE`.
        eprintln!("{PACKAGE_CHECK_MESSAGE}");
        exit_status::exit_usage();
    }
    let (name, source) = read_source(&command.input);
    let locale_config = single_file_locale_config(&command.input);
    let mut result = radix::services::check_diagnostics_for_source_with_config(
        &name,
        &source,
        command.permissive,
        None,
        None,
        &locale_config,
    );
    let warn_policy = radix::module::driver::WarnPolicy {
        deny_all_warnings: command.deny_warnings,
        deny_codes: command.deny_codes.clone(),
    };
    radix::module::driver::apply_warn_policy(&mut result.diagnostics, &warn_policy);
    faber_cli::postprocess::print_diagnostics(&result.diagnostics, command.diagnostic_mode, None);
    if air && let Some(rows) = check_air::advisory_rows(&name, &source) {
        let mut out = std::io::stdout().lock();
        if json {
            if let Err(err) = air_json::render_json_rows(&mut out, &rows) {
                eprint_write_error(std::path::Path::new("<stdout>"), &err);
            }
        } else if let Err(err) = check_air::render_report(&mut out, &rows) {
            eprint_write_error(std::path::Path::new("<stdout>"), &err);
        }
    }
    let success = result.success
        && !result
            .diagnostics
            .iter()
            .any(radix::module::diagnostics::Diagnostic::is_error);
    if success {
        eprintln!("ok: {name}");
    } else {
        exit_status::exit_data_error();
    }
}

fn cmd_verify_file(command: VerifyCommand) {
    let (name, source) = read_source(&command.input);
    let result = radix::services::verify_source_with_config(
        &name,
        &source,
        None,
        &single_file_locale_config(&command.input),
    );
    if !result.diagnostics.is_empty() {
        eprint!(
            "{}",
            radix::module::diagnostics::render_expanded_diagnostics(&result.diagnostics)
        );
    }
    if let Some(coverage) = &result.coverage {
        print!(
            "{}",
            radix::module::aspect::render_hir_aspect_coverage(coverage)
        );
    }
    if !result.success {
        exit_status::exit_data_error();
    }
}
