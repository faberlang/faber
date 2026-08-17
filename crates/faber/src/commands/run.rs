//! `faber run` — interpreted execution, always.
//!
//! `faber run <path>` interprets a single `.fab` file, package directory,
//! `faber.toml` manifest, or archive through the runner — mode is never
//! implicit in input shape and nothing is compiled. Compiled source artifacts
//! are built with `faber build` and are never executed by faber (see
//! `docs/operator/test-e2e-rework.md` §11); device packages whose route
//! requires the host driver keep an execution path through it. Shebang:
//! `#!/usr/bin/env faber run`.
//!
//! The `-c` one-liner and `faber repl` are the other runner entry modes.

use crate::cli::{BackendSelection, FmirRunArgs, ReplArgs, RunArgs};
use crate::input_shape::locale_without_package_error;
use crate::package;
use crate::script::{
    interpret_source, interpret_source_or_exit, print_run_source_error, raw_exit_code,
};
use radix_module::diagnostics::Diagnostic;
use radix_module::mir::StdioHost;
use radix_program::mir::DeviceSelection;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

pub(super) fn cmd_run(args: RunArgs) {
    crate::commands::validate_deny_codes(&args.deny);
    let input_path = PathBuf::from(&args.path);
    if let Some(message) = locale_without_package_error(
        args.locale.as_deref(),
        &[args.path.display().to_string()],
        false,
    ) {
        eprintln!("error: {message}");
        std::process::exit(1);
    }

    // N1.1 precedence: CLI `--backend` > manifest `[device] backend` > `auto`.
    // An invalid manifest value fails closed with a structured diagnostic
    // (never silently ignored).
    let selection = match resolve_route_selection(&args, &input_path) {
        Ok(selection) => selection,
        Err(diagnostic) => {
            eprint_route_diagnostics(&[*diagnostic]);
            std::process::exit(1);
        }
    };

    interpret_path(
        &input_path,
        &args.args,
        args.locale.as_deref(),
        args.diagnostics_locale.as_deref(),
        selection,
    );
}

/// Dispatch an interpreted input path to the package-MIR runner, archive
/// extraction, or single-source runner. `faber run` always routes through
/// this dispatch; nothing here compiles or invokes Cargo.
pub(super) fn interpret_path(
    path: &PathBuf,
    program_args: &[String],
    locale: Option<&str>,
    diagnostics_locale: Option<&str>,
    selection: DeviceSelection,
) {
    if super::archive::is_zip_archive(path) {
        let archive = match super::archive::extract_package_archive(path) {
            Ok(archive) => archive,
            Err(diagnostic) => {
                eprint_archive_diagnostics(std::slice::from_ref(diagnostic.as_ref()));
                std::process::exit(1);
            }
        };
        let config =
            match script_package_config(archive.package_input(), locale, diagnostics_locale) {
                Ok(config) => config,
                Err(diagnostic) => {
                    eprint_archive_diagnostics(std::slice::from_ref(diagnostic.as_ref()));
                    std::process::exit(1);
                }
            };
        let mut host = StdioHost::with_argumenta(program_args.to_vec());
        if let Err(mut diagnostics) = radix_program::mir::run_package_mir_with_selection(
            &config,
            archive.package_input(),
            selection,
            &package::mir::faber_mir_seams(),
            &mut host,
        ) {
            archive.remap_diagnostics(&mut diagnostics);
            eprint_archive_diagnostics(&diagnostics);
            eprint_device_run_note(&diagnostics);
            drop(archive);
            std::process::exit(1);
        }
        return;
    }

    if is_package_interpret_input(path) {
        let config = match script_package_config(path, locale, diagnostics_locale) {
            Ok(config) => config,
            Err(diagnostic) => {
                super::eprint_compile_diagnostics(std::slice::from_ref(diagnostic.as_ref()));
                std::process::exit(1);
            }
        };
        let mut host = StdioHost::with_argumenta(program_args.to_vec());
        if let Err(diagnostics) = radix_program::mir::run_package_mir_with_selection(
            &config,
            path,
            selection,
            &package::mir::faber_mir_seams(),
            &mut host,
        ) {
            super::eprint_compile_diagnostics(&diagnostics);
            eprint_device_run_note(&diagnostics);
            std::process::exit(1);
        }
        return;
    }

    // Single-source runner. A lone `.fab` file carries no package `[device]`
    // section, so an explicit backend request can never be served here
    // (N1.1 fail closed: "package has no device program").
    if selection != DeviceSelection::Auto {
        eprintln!(
            "error: explicit backend selection `{}` cannot be served: a single `.fab` file carries no package device program; device execution requires a package with a `[device]` section",
            selection.spelling()
        );
        std::process::exit(1);
    }

    let source = match fs::read_to_string(path) {
        Ok(source) => source,
        Err(err) => {
            eprintln!("error: cannot read '{}': {}", path.display(), err);
            std::process::exit(1);
        }
    };

    let config = match script_package_config(path, locale, diagnostics_locale) {
        Ok(config) => config,
        Err(diagnostic) => {
            super::eprint_compile_diagnostics(std::slice::from_ref(diagnostic.as_ref()));
            std::process::exit(1);
        }
    };
    let session = radix_module::driver::Session::new(config);
    let mut host = StdioHost::with_argumenta(program_args.to_vec());
    let result =
        crate::script::run_with_session(&session, &path.display().to_string(), &source, &mut host);
    exit_interpreted(result);
}

/// Process status scripta gates observe for a single-file runner result.
///
/// Success yields `0`. An explicit `processus.exit_process(code)` yields that
/// Unix-masked code. Frontend / MIR / runner failures yield `1`.
pub(super) fn single_file_run_process_code(
    result: Result<ExitCode, radix_module::mir::RunSourceError>,
) -> i32 {
    match result {
        Ok(status) => raw_exit_code(status),
        Err(_) => 1,
    }
}

fn exit_interpreted(result: Result<ExitCode, radix_module::mir::RunSourceError>) {
    if let Err(error) = &result {
        print_run_source_error(error);
    }
    let code = single_file_run_process_code(result);
    if code != 0 {
        std::process::exit(code);
    }
}

/// Reader config for interpreted packages: CLI `--locale` / `--diagnostics-locale`
/// over the package `[locale]`, then the product default. Do not use
/// [`package::default_config_with_locale`] here — that installs the English
/// pack and `effective_package_config` will not replace it with the package's
/// declared locale.
fn script_package_config(
    input: &Path,
    locale: Option<&str>,
    diagnostics_locale: Option<&str>,
) -> Result<radix_module::driver::Config, Box<Diagnostic>> {
    package::config_with_locale(
        radix_module::Target::HirTypeScript,
        input,
        locale,
        diagnostics_locale,
    )
    .map(|(config, _)| config.with_dev_stdlib())
}

/// Whether `path` routes through the package-MIR runner rather than the
/// single-source runner: manifest-backed or directory packages, or a
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
    let Ok(peeled) = radix_module::driver::peel_raw_source(&display_name, &raw_source) else {
        return false;
    };
    let lex = match package::default_config_with_locale(radix_module::Target::HirTypeScript) {
        Ok(config) => match config.locale_pack.as_ref() {
            Some(pack) => radix_module::lexer::lex_with_locale_pack(peeled.body, pack),
            None => radix_module::lexer::lex(peeled.body),
        },
        Err(_) => radix_module::lexer::lex(peeled.body),
    };
    let parse = radix_module::parser::parse(lex);
    let Some(program) = parse.program else {
        return false;
    };
    program.statements.iter().any(|statement| {
        let radix_module::syntax::StmtKind::Import(import) = &statement.kind else {
            return false;
        };
        !radix_module::kernel::is_kernel_import_path(parse.interner.resolve(import.path))
    })
}

/// Whether `path` is a single `.fab` file (the single-source runner input).
pub(super) fn is_single_fab_file(path: &Path) -> bool {
    path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("fab")
}

/// Resolve the effective backend selection for `faber run` (N1.1): CLI
/// `--backend` > manifest `[device] backend` > default `auto`. An invalid
/// manifest value fails closed with a structured diagnostic (never silently
/// ignored).
fn resolve_route_selection(
    args: &RunArgs,
    input_path: &Path,
) -> Result<DeviceSelection, Box<Diagnostic>> {
    if let Some(backend) = args.backend {
        return Ok(backend.selection());
    }
    let Ok(layout) = package::discover_build_layout(input_path) else {
        return Ok(DeviceSelection::Auto);
    };
    if !layout.manifest_path.exists() {
        return Ok(DeviceSelection::Auto);
    }
    let manifest = package::read_manifest(&layout.manifest_path)?;
    let manifest_backend = package::manifest_backend_selection(
        manifest.device.backend.as_deref(),
        &layout.manifest_path,
    )?;
    // DDPP1-U2 (C2 feature isolation): with `device-runtime` the portable
    // default is applied by the host factory; without it the same semantics
    // (`cli override > manifest backend > auto`) resolve locally — a small
    // build can never reach a device route.
    #[cfg(feature = "device-runtime")]
    {
        return Ok(package::effective_backend_selection(None, manifest_backend));
    }
    #[cfg(not(feature = "device-runtime"))]
    {
        return Ok(manifest_backend.unwrap_or(DeviceSelection::Auto));
    }
}

/// Render route diagnostics with their stable host codes (N1.4). The
/// composite-host failures carry `E_DEVICE_*` / `E_BACKEND_*` / `E_NO_*`
/// `issue` args; surfacing them in the terminal keeps the fail-before-launch
/// receipts self-describing (the structured code + issue + named args are
/// always present on the [`Diagnostic`] for programmatic consumers).
fn eprint_route_diagnostics(diagnostics: &[Diagnostic]) {
    for diag in diagnostics {
        if diag.is_error() {
            match diag.issue() {
                Some(code)
                    if code.starts_with("E_DEVICE_")
                        || code.starts_with("E_BACKEND_")
                        || code.starts_with("E_NO_") =>
                {
                    eprintln!("error: {code}: {}", diag.message);
                }
                _ => eprintln!("error: {}", diag.message),
            }
        } else {
            eprintln!("warning: {}", diag.message);
        }
    }
}

/// After a failed interpreted run of a device-bearing package, point the user
/// at the build + host-driver route: `faber run` interprets and never
/// compiles, so a non-runnable device package is built with `faber build` and
/// its artifact executed through the host driver (the first-party target
/// runtime).
fn eprint_device_run_note(diagnostics: &[Diagnostic]) {
    let device_failure = diagnostics.iter().any(|diag| {
        let issue = diag.issue().unwrap_or_default();
        issue.starts_with("E_DEVICE_")
            || issue.starts_with("E_BACKEND_")
            || diag.message.contains("device section")
    });
    if device_failure {
        eprintln!(
            "note: this package declares a device program that executes through the host driver; if it cannot be run here, build the artifact with `faber build` and run the generated entrypoint through the host driver"
        );
    }
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

/// Execute `-c` / `--command` source via the runner.
pub(super) fn cmd_eval(source: &str, args: Vec<String>) {
    let mut host = StdioHost::with_argumenta(args);
    interpret_source_or_exit("command-line", source, &mut host);
}

/// Interactive runner REPL: accumulate cells and re-lower MIR on each input
/// line.
pub(super) fn cmd_repl(args: ReplArgs) {
    let mut host = StdioHost::with_argumenta(args.args);
    let mut buffer = String::new();
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    loop {
        crate::io_buf::write_prompt(&mut stdout, "faber> ");
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

pub(super) fn cmd_fmir_run_image(args: FmirRunArgs) {
    // The one host-construction policy on the image-runner route (N1.1/N1.5):
    // the image's device section decides whether the route carries a device
    // program, and the selection request is the CLI `--backend` override
    // falling back to the image's recorded selection.
    let decision = match radix_program::mir::fmir_image_route_decision(
        &args.image,
        &package::mir::faber_mir_seams(),
    ) {
        Ok(decision) => decision,
        Err(diagnostics) => {
            eprint_route_diagnostics(&diagnostics);
            eprintln!("fmir image load failed");
            std::process::exit(1);
        }
    };
    let selection = args
        .backend
        .map(BackendSelection::selection)
        .unwrap_or(decision.declared_selection);
    let mut host = StdioHost::with_argumenta(args.args);
    // The route function applies the one host-construction policy: a
    // device-bearing image runs through the composite host's device route
    // (S1-6 launch seam); anything else runs the CPU/FMIR runner.
    if let Err(diagnostics) = radix_program::mir::run_fmir_image_path_with_selection(
        &args.image,
        Some(selection),
        &package::mir::faber_mir_seams(),
        &mut host,
    ) {
        eprint_route_diagnostics(&diagnostics);
        eprintln!("fmir image execution failed");
        std::process::exit(1);
    }
}

#[cfg(test)]
#[path = "run_test.rs"]
mod tests;
