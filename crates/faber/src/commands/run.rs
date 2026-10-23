#![cfg(feature = "runner")]

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

use crate::cli::{FmirRunArgs, ReplArgs, RunArgs};
use crate::script::{
    interpret_source, interpret_source_or_exit, print_run_source_error, raw_exit_code,
};
use faber_cli::input_shape::locale_without_package_error;
use faber_cli::package::{self, DeviceChoice};
use radix::module::diagnostics::Diagnostic;
use radix::module::mir::StdioHost;
use radix::program::mir::DeviceSelection;
use std::fs;
use std::io;
use std::path::Path;
use std::process::ExitCode;
use std::time::Instant;

use super::run_stats::{self, RunStatsArtifact};

pub(super) fn cmd_run(args: RunArgs) {
    crate::commands::validate_deny_codes(&args.deny);
    // Package discovery and execution require absolute inputs; resolve the
    // caller-relative path before any route selection or package handoff.
    let input_path = package::absolutize_path(&args.path);
    if let Some(message) = locale_without_package_error(
        args.locale.as_deref(),
        &[args.path.display().to_string()],
        false,
    ) {
        eprintln!("error: {message}");
        std::process::exit(1);
    }

    if let Some(seed) = args.shuffle_seed {
        if !radix::module::mir::configure_shuffle_seed(seed) {
            eprintln!("note: shuffle seed already configured; the first value stays in effect");
        }
    }
    #[cfg(all(feature = "device-runtime", target_os = "macos"))]
    if args.debug {
        radix_device_metal::set_report_counters(true);
    }

    // N1.1 precedence: CLI `--device` > manifest `[device] device` > `auto`.
    // An invalid manifest value fails closed with a structured diagnostic
    // (never silently ignored).
    let selection = match resolve_route_selection(&args, &input_path) {
        Ok(selection) => selection,
        Err(diagnostic) => {
            eprint_route_diagnostics(&[*diagnostic]);
            std::process::exit(1);
        }
    };

    // The flag is accepted on the CPU route; say once, before launch, that
    // the count cannot configure anything yet.
    if let Some(note) = threads_no_effect_note(args.threads, selection) {
        eprintln!("{note}");
    }

    interpret_path(
        &input_path,
        &args.args,
        args.locale.as_deref(),
        args.diagnostics_locale.as_deref(),
        selection,
        args.stats,
        args.stats_file.as_ref(),
    );
}

/// Dispatch an interpreted input path to the package-MIR runner, archive
/// extraction, or single-source runner. `faber run` always routes through
/// this dispatch; nothing here compiles or invokes Cargo.
fn interpret_path(
    path: &Path,
    program_args: &[String],
    locale: Option<&str>,
    diagnostics_locale: Option<&str>,
    selection: DeviceSelection,
    stats: bool,
    stats_file: Option<&std::path::PathBuf>,
) {
    let stats_requested = stats || stats_file.is_some();
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
        #[cfg(feature = "device-runtime")]
        if let Some(result) = try_device_session(
            &config,
            archive.package_input(),
            selection,
            &mut host,
            stats,
            stats_file,
        ) {
            if let Err(mut diagnostics) = result {
                archive.remap_diagnostics(&mut diagnostics);
                eprint_archive_diagnostics(&diagnostics);
                drop(archive);
                std::process::exit(1);
            }
            drop(archive);
            return;
        }
        if stats_requested {
            let metadata = match run_stats::package_metadata(archive.package_input()) {
                Ok(metadata) => metadata,
                Err(diagnostic) => {
                    eprint_archive_diagnostics(std::slice::from_ref(diagnostic.as_ref()));
                    drop(archive);
                    std::process::exit(1);
                }
            };
            let started = Instant::now();
            match radix::program::mir::run_package_mir_with_selection_and_stats(
                &config,
                archive.package_input(),
                selection,
                &package::mir::faber_mir_seams(),
                &mut host,
            ) {
                Err(mut diagnostics) => {
                    archive.remap_diagnostics(&mut diagnostics);
                    eprint_archive_diagnostics(&diagnostics);
                    eprint_device_run_note(&diagnostics);
                    drop(archive);
                    std::process::exit(1);
                }
                Ok(outcome) => {
                    let exit = i32::from(outcome.result.is_err());
                    let artifact = RunStatsArtifact::new(
                        metadata,
                        outcome.stats,
                        elapsed_wall_ms(started),
                        exit,
                    );
                    if let Err(error) = run_stats::emit(&artifact, stats, stats_file) {
                        eprintln!("error: could not write run statistics: {error}");
                        drop(archive);
                        std::process::exit(1);
                    }
                    if let Err(mut diagnostics) = outcome.result {
                        archive.remap_diagnostics(&mut diagnostics);
                        eprint_archive_diagnostics(&diagnostics);
                        eprint_device_run_note(&diagnostics);
                        drop(archive);
                        std::process::exit(1);
                    }
                }
            }
        } else if let Err(mut diagnostics) = radix::program::mir::run_package_mir_with_selection(
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
        // gpu-reset C2-U6b/U6c: an explicit GPU run, or an `auto` run of a
        // package that declares kernels, goes through the in-process device
        // session; no child process, no `[device]` table needed.
        #[cfg(feature = "device-runtime")]
        if let Some(result) =
            try_device_session(&config, path, selection, &mut host, stats, stats_file)
        {
            if let Err(diagnostics) = result {
                eprint_route_diagnostics(&diagnostics);
                std::process::exit(1);
            }
            return;
        }
        if stats_requested {
            let metadata = match run_stats::package_metadata(path) {
                Ok(metadata) => metadata,
                Err(diagnostic) => {
                    super::eprint_compile_diagnostics(std::slice::from_ref(diagnostic.as_ref()));
                    std::process::exit(1);
                }
            };
            let started = Instant::now();
            match radix::program::mir::run_package_mir_with_selection_and_stats(
                &config,
                path,
                selection,
                &package::mir::faber_mir_seams(),
                &mut host,
            ) {
                Err(diagnostics) => {
                    super::eprint_compile_diagnostics(&diagnostics);
                    eprint_device_run_note(&diagnostics);
                    std::process::exit(1);
                }
                Ok(outcome) => {
                    let exit = i32::from(outcome.result.is_err());
                    let artifact = RunStatsArtifact::new(
                        metadata,
                        outcome.stats,
                        elapsed_wall_ms(started),
                        exit,
                    );
                    if let Err(error) = run_stats::emit(&artifact, stats, stats_file) {
                        eprintln!("error: could not write run statistics: {error}");
                        std::process::exit(1);
                    }
                    if let Err(diagnostics) = outcome.result {
                        super::eprint_compile_diagnostics(&diagnostics);
                        eprint_device_run_note(&diagnostics);
                        std::process::exit(1);
                    }
                }
            }
        } else if let Err(diagnostics) = radix::program::mir::run_package_mir_with_selection(
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
    // section, and v1 stats are a package-route contract. Refuse before
    // reading or executing the source so a requested artifact is never
    // silently omitted.
    if stats_requested {
        eprintln!(
            "error: run statistics require a package input; stats are not supported for a single `.fab` file"
        );
        std::process::exit(1);
    }
    // An explicit GPU request can never be served here (N1.1 fail closed:
    // "package has no device program"). `reference` and `cpu` need no device program: it runs
    // the file's kernels on the MIR runner.
    #[cfg(not(feature = "device-runtime"))]
    if !matches!(
        selection,
        DeviceSelection::Auto | DeviceSelection::Reference | DeviceSelection::Cpu
    ) {
        eprintln!(
            "error: explicit device selection `{}` cannot be served: a single `.fab` file carries no package device program; device execution requires a package with a `[device]` section",
            selection.spelling()
        );
        std::process::exit(1);
    }

    let source = match fs::read_to_string(path) {
        Ok(source) => source,
        Err(err) => super::eprint_io_error(path, &err),
    };

    let config = match script_package_config(path, locale, diagnostics_locale) {
        Ok(config) => config,
        Err(diagnostic) => {
            super::eprint_compile_diagnostics(std::slice::from_ref(diagnostic.as_ref()));
            std::process::exit(1);
        }
    };
    // gpu-reset C2-U6c: a lone `.fab` file runs on the in-process device
    // session too (explicit GPU, or `auto` with kernels).
    #[cfg(feature = "device-runtime")]
    {
        let mut device_host = StdioHost::with_argumenta(program_args.to_vec());
        if let Some(result) = try_device_session(
            &config,
            path,
            selection,
            &mut device_host,
            stats,
            stats_file,
        ) {
            if let Err(diagnostics) = result {
                eprint_route_diagnostics(&diagnostics);
                std::process::exit(1);
            }
            return;
        }
    }
    let config = match selection.kernel_lowering() {
        radix::module::mir::KernelLowering::Host => config.with_host_kernels(),
        radix::module::mir::KernelLowering::Device => config,
    };
    let session = radix::module::driver::Session::new(config);
    let mut host = StdioHost::with_argumenta(program_args.to_vec());
    let result = crate::script::run_with_session(
        &session,
        &path.display().to_string(),
        &source,
        &mut host,
        selection,
    );
    exit_interpreted(result);
}

/// Process status scripta gates observe for a single-file runner result.
///
/// Success yields `0`. An explicit `process.exit_process(code)` yields that
/// Unix-masked code. Frontend / MIR / runner failures yield `1`.
pub(super) fn single_file_run_process_code(
    result: Result<ExitCode, radix::module::mir::RunSourceError>,
) -> i32 {
    match result {
        Ok(status) => raw_exit_code(status),
        Err(_) => 1,
    }
}

fn exit_interpreted(result: Result<ExitCode, radix::module::mir::RunSourceError>) {
    if let Err(error) = &result {
        print_run_source_error(error);
    }
    let code = single_file_run_process_code(result);
    if code != 0 {
        std::process::exit(code);
    }
}

fn elapsed_wall_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
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
) -> Result<radix::module::driver::Config, Box<Diagnostic>> {
    package::config_with_locale(
        radix::module::Target::HirTypeScript,
        input,
        locale,
        diagnostics_locale,
    )
    .map(|(config, _)| config.with_dev_locale_root())
}

/// Whether `path` routes through the package-MIR runner rather than the
/// single-source runner: manifest-backed or directory packages, or a
/// manifestless `.fab` file that declares a non-kernel, non-`norma:` import.
/// `norma:` imports are linked by `script::run_with_session`.
pub(super) fn is_package_interpret_input(path: &Path) -> bool {
    package::is_manifest_backed_or_directory_package_input(path)
        || manifestless_file_declares_non_kernel_import(path)
}

/// Whether a manifestless `.fab` file declares an import outside the kernel
/// and outside `norma:`, which `script::run_with_session` links itself.
fn manifestless_file_declares_non_kernel_import(path: &Path) -> bool {
    if !is_single_fab_file(path) {
        return false;
    }
    let Ok(raw_source) = fs::read_to_string(path) else {
        return false;
    };
    let display_name = path.display().to_string();
    let Ok(peeled) = radix::module::driver::peel_raw_source(&display_name, &raw_source) else {
        return false;
    };
    // TEMPORARY STAGING: the pack-less fallback reads the
    // no-locale default while `RADIX_DEFAULT_CODE_LOCALE=en`; unset keeps the
    // historical pack-less (Latin) reading.
    let code_pack = package::default_config_with_locale(radix::module::Target::HirTypeScript)
        .ok()
        .and_then(|config| config.locale_pack)
        .or_else(|| {
            radix::module::driver::default_code_pack(&radix::module::driver::Config::default())
        });
    let lex = match code_pack.as_ref() {
        Some(pack) => radix::module::lexer::lex_with_locale_pack(peeled.body, pack),
        None => radix::module::lexer::lex(peeled.body),
    };
    let parse = radix::module::parser::parse(lex);
    let Some(program) = parse.program else {
        return false;
    };
    program.statements.iter().any(|statement| {
        let radix::module::syntax::StmtKind::Import(import) = &statement.kind else {
            return false;
        };
        let path = parse.interner.resolve(import.path);
        !radix::module::kernel::is_kernel_import_path(path) && !path.starts_with("norma:")
    })
}

/// Whether `path` is a single `.fab` file (the single-source runner input).
pub(super) fn is_single_fab_file(path: &Path) -> bool {
    path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("fab")
}

/// Whether this run takes the in-process device session: an explicit GPU, or
/// `auto` over a package that declares kernels (a GPU is admitted, or the run
/// fails closed with `E_DEVICE_NONE_ADMITTED`; `auto` never picks the CPU for
/// a kernel package). `auto` over a package without kernels, and `reference` / `cpu`, run
/// the CPU route.
///
/// The `auto` answer prepares the whole package, so it is a stand-alone
/// question only on the fallback paths of [`run_auto_on_device_session_with`];
/// the run itself decides from the one preparation it executes.
#[cfg(feature = "device-runtime")]
fn device_session_selected(
    config: &radix::module::driver::Config,
    input: &Path,
    selection: DeviceSelection,
) -> bool {
    match selection {
        DeviceSelection::Metal | DeviceSelection::Cuda => true,
        DeviceSelection::Reference | DeviceSelection::Cpu => false,
        DeviceSelection::Auto => {
            package::mir::package_declares_kernels(config, input).unwrap_or(false)
        }
    }
}

/// Run `input` on the in-process device session when `selection` takes it
/// ([`device_session_selected`]); `None` means the CPU route runs instead.
/// `auto` prepares the package once: the lowered unit that proves it declares
/// kernels is the one the device run executes.
#[cfg(feature = "device-runtime")]
fn try_device_session(
    config: &radix::module::driver::Config,
    input: &Path,
    selection: DeviceSelection,
    host: &mut StdioHost,
    stats: bool,
    stats_file: Option<&std::path::PathBuf>,
) -> Option<Result<(), Vec<Diagnostic>>> {
    match selection {
        DeviceSelection::Reference | DeviceSelection::Cpu => None,
        DeviceSelection::Metal | DeviceSelection::Cuda => Some(run_on_device_session(
            config, input, selection, host, stats, stats_file,
        )),
        DeviceSelection::Auto => {
            let mut registry = package::mir::system_device_registry();
            run_auto_on_device_session_with(
                config,
                input,
                &mut registry,
                host,
                stats,
                stats_file,
                &mut io::stdout(),
            )
        }
    }
}

/// [`try_device_session`] for `auto` over an injected registry and marker
/// sink.
#[cfg(feature = "device-runtime")]
#[allow(clippy::too_many_arguments)]
fn run_auto_on_device_session_with<H: radix_mir_runner::Host + ?Sized>(
    config: &radix::module::driver::Config,
    input: &Path,
    registry: &mut radix_device_core::registry::DeviceRegistry,
    host: &mut H,
    stats: bool,
    stats_file: Option<&std::path::PathBuf>,
    out: &mut dyn io::Write,
) -> Option<Result<(), Vec<Diagnostic>>> {
    let stats_requested = stats || stats_file.is_some();
    let metadata = if stats_requested {
        match run_stats::package_metadata(input) {
            Ok(metadata) => Some(metadata),
            // Metadata failure precedes any run: keep the route the package
            // would have taken (device reports it, CPU reports its own).
            Err(diagnostic) => {
                return device_session_selected(config, input, DeviceSelection::Auto)
                    .then(|| Err(vec![*diagnostic]));
            }
        }
    } else {
        None
    };
    let started = Instant::now();
    match package::mir::run_package_on_device_session_if_kernels(
        config,
        input,
        registry,
        host,
        stats_requested,
    ) {
        Ok(None) => None,
        Ok(Some(run)) => Some(finish_device_run(
            run, metadata, started, stats, stats_file, out,
        )),
        Err(diagnostics) => Some(Err(diagnostics)),
    }
}

/// The marker a dispatching run prints with its stats artifact: device phases
/// and runtime counters are measured, even when the driver has no GPU times.
#[cfg(feature = "device-runtime")]
const DEVICE_STATS_MARKER: &str = "device: dispatched (measured)";

/// Run `input` on the in-process device session over the system registry,
/// emitting host stats and a device receipt in [`DEVICE_STATS_MARKER`] format
/// when requested.
#[cfg(feature = "device-runtime")]
fn run_on_device_session(
    config: &radix::module::driver::Config,
    input: &Path,
    selection: DeviceSelection,
    host: &mut StdioHost,
    stats: bool,
    stats_file: Option<&std::path::PathBuf>,
) -> Result<(), Vec<Diagnostic>> {
    let mut registry = package::mir::system_device_registry();
    run_on_device_session_with(
        config,
        input,
        selection,
        &mut registry,
        host,
        stats,
        stats_file,
        &mut io::stdout(),
    )
}

/// [`run_on_device_session`] over an injected registry and marker sink.
#[cfg(feature = "device-runtime")]
#[allow(clippy::too_many_arguments)]
fn run_on_device_session_with<H: radix_mir_runner::Host + ?Sized>(
    config: &radix::module::driver::Config,
    input: &Path,
    selection: DeviceSelection,
    registry: &mut radix_device_core::registry::DeviceRegistry,
    host: &mut H,
    stats: bool,
    stats_file: Option<&std::path::PathBuf>,
    out: &mut dyn io::Write,
) -> Result<(), Vec<Diagnostic>> {
    let stats_requested = stats || stats_file.is_some();
    let metadata = if stats_requested {
        Some(run_stats::package_metadata(input).map_err(|diagnostic| vec![*diagnostic])?)
    } else {
        None
    };
    let started = Instant::now();
    let run = package::mir::run_package_on_device_session_with_stats(
        config,
        input,
        selection,
        registry,
        host,
        stats_requested,
    )?;
    finish_device_run(run, metadata, started, stats, stats_file, out)
}

/// Emit the host stats and device receipt in [`DEVICE_STATS_MARKER`] format
/// for a finished device run that was asked to collect stats.
#[cfg(feature = "device-runtime")]
fn finish_device_run(
    run: package::mir::DeviceSessionRun,
    metadata: Option<run_stats::PackageMetadata>,
    started: Instant,
    stats: bool,
    stats_file: Option<&std::path::PathBuf>,
    out: &mut dyn io::Write,
) -> Result<(), Vec<Diagnostic>> {
    let package::mir::DeviceSessionRun {
        stats: run_stats_value,
        device,
        ..
    } = run;
    if let (Some(metadata), Some(runner_stats)) = (metadata, run_stats_value) {
        writeln!(out, "{DEVICE_STATS_MARKER}").ok();
        let mut artifact =
            RunStatsArtifact::new(metadata, runner_stats, elapsed_wall_ms(started), 0);
        artifact.device = Some(run_stats::DeviceMetadata::from_receipt(&device));
        if let Err(error) = run_stats::emit(&artifact, stats, stats_file) {
            eprintln!("error: could not write run statistics: {error}");
            std::process::exit(1);
        }
    }
    Ok(())
}

/// Resolve the effective device selection for `faber run`: the shared
/// precedence (CLI > manifest > `auto`) lives in
/// [`package::resolve_device_selection`], then the `--threads` route contract
/// is enforced (need, CD-1): a worker count is valid only when the
/// resolved selection is the CPU device. Any other route fails closed before
/// launch rather than silently ignoring the count; there is no `[device]
/// threads` manifest key. A stats request is permitted with any selection: a
/// dispatching run reports host rows and a device receipt.
fn resolve_route_selection(
    args: &RunArgs,
    input_path: &Path,
) -> Result<DeviceSelection, Box<Diagnostic>> {
    let selection = package::resolve_device_selection(args.device, input_path)?;
    ensure_threads_cpu_route(args.threads, selection)?;
    Ok(selection)
}

/// The stderr note printed once per run when `--threads N` is accepted on
/// the CPU device but cannot configure anything yet: the CPU device runs on
/// the reference route until the worker device lands.
const THREADS_NO_EFFECT_NOTE: &str =
    "note: --threads has no effect yet: the cpu device runs on the reference route";

/// Decide the one visibility note for an accepted `--threads N` on the CPU
/// route. Pure so the rule is testable without launching a program: a count
/// reaches here only with the CPU selection ([`ensure_threads_cpu_route`]
/// fails closed everywhere else), where the reference route ignores it.
fn threads_no_effect_note(
    threads: Option<u32>,
    selection: DeviceSelection,
) -> Option<&'static str> {
    if threads.is_some() && selection == DeviceSelection::Cpu {
        return Some(THREADS_NO_EFFECT_NOTE);
    }
    None
}

/// Fail closed when `--threads N` is given with a resolved selection other
/// than the CPU device (need, CD-1). The count configures CPU
/// workers only; it is never silently ignored, and there is no `[device]
/// threads` manifest key.
fn ensure_threads_cpu_route(
    threads: Option<u32>,
    selection: DeviceSelection,
) -> Result<(), Box<Diagnostic>> {
    if threads.is_none() || selection == DeviceSelection::Cpu {
        return Ok(());
    }
    Err(Box::new(
        faber_cli::package_diagnostic_error("`--threads` is valid only with `--device cpu`")
            .with_arg("issue", "E_THREADS_REQUIRES_CPU")
            .with_arg("device", selection.spelling()),
    ))
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
                        || code.starts_with("E_THREADS_")
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
        faber_cli::io_buf::write_prompt(&mut stdout, "faber> ");
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
    // program, and the selection request is the CLI `--device` override
    // falling back to the image's recorded selection.
    let decision = match radix::program::mir::fmir_image_route_decision(
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
        .device
        .map_or(decision.declared_selection, DeviceChoice::selection);
    let mut host = StdioHost::with_argumenta(args.args);
    // The route function applies the one host-construction policy: a
    // device-bearing image runs through the composite host's device route
    // (S1-6 launch seam); anything else runs the CPU/FMIR runner.
    if let Err(diagnostics) = radix::program::mir::run_fmir_image_path_with_selection(
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

#[cfg(all(test, feature = "device-runtime", target_os = "macos"))]
#[path = "run_device_test.rs"]
mod run_device_tests;

#[cfg(all(test, feature = "device-runtime", target_os = "macos"))]
#[path = "run_device_hw_test.rs"]
mod run_device_hw_tests;
