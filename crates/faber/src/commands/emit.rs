//! `faber emit -t faber` policy wrapper.

use std::path::Path;

use crate::cli::FaberCliTarget;
use radix::services::EmitCommand;

// ST2-2.10: number of package analyses the retry seam entered on this thread.
//
// A test seam: the single-file -> package-analysis hand-off runs once and only
// when the structured import issue is present, and the unit proves the branch
// by reading this counter instead of inspecting message text. Thread-local so
// the parallel test threads never observe each other's entries.
#[cfg(test)]
thread_local! {
    static PACKAGE_ANALYSIS_ENTRIES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn package_analysis_entries() -> usize {
    PACKAGE_ANALYSIS_ENTRIES.with(std::cell::Cell::get)
}

#[cfg(test)]
pub(crate) fn reset_package_analysis_entries() {
    PACKAGE_ANALYSIS_ENTRIES.with(|entries| entries.set(0));
}

pub fn cmd_emit_faber(command: EmitCommand, format: bool, linter: bool) {
    if command.package {
        eprintln!("error: package Faber emit is not supported; use single-file input");
        faber_cli::exit_status::exit_usage();
    }

    cmd_emit_with_locale(command, format, linter);
}

fn report_locale_diagnostic(diagnostic: &radix::module::diagnostics::Diagnostic) -> ! {
    if diagnostic.issue() == Some("package_path_missing") {
        eprintln!("{}", radix::module::diagnostics::render_plain(diagnostic));
    } else {
        eprintln!("error: {}", diagnostic.message);
    }
    faber_cli::exit_status::exit_for_diagnostic(diagnostic);
}

/// Merge each library root's `[target.<spelling>]` bindings (read by Faber,
/// opaque to the compiler) into one map, collecting load diagnostics from the
/// roots that cannot be read.
pub(super) fn merge_library_target_bindings(
    roots: &std::collections::BTreeSet<std::path::PathBuf>,
    target_spelling: &str,
) -> (
    radix::module::driver::TargetBindings,
    Vec<radix::module::diagnostics::Diagnostic>,
) {
    let mut merged = radix::module::driver::TargetBindings::default();
    let mut load_errors = Vec::new();
    for root in roots {
        match faber_cli::package::load_target_bindings(root, target_spelling) {
            Ok(bindings) => {
                merged.functions.extend(bindings.functions);
                merged.types.extend(bindings.types);
                merged.annotations.extend(bindings.annotations);
            }
            Err(diagnostics) => load_errors.extend(diagnostics),
        }
    }
    (merged, load_errors)
}

/// The library package root for one resolved interface path: the nearest
/// ancestor directory (the file's own directory first) that holds the package
/// manifest (`package::MANIFEST_FILE`, not visible from the binary crate).
/// `None` when no ancestor does.
#[allow(dead_code)]
pub(super) fn library_root_of_interface(interface_path: &Path) -> Option<std::path::PathBuf> {
    interface_path
        .ancestors()
        .find(|ancestor| ancestor.join("faber.toml").is_file())
        .map(Path::to_path_buf)
}

/// Every library package root reachable through an analyzed program's
/// expanded library imports, deduped by directory. Imports whose interface
/// path has no manifest ancestor are skipped.
pub(super) fn library_package_roots(
    package: &radix::program::analyze::AnalyzedProgram,
) -> std::collections::BTreeSet<std::path::PathBuf> {
    let mut roots = std::collections::BTreeSet::new();
    for node in package.nodes.values() {
        for import in &node.expanded_library_imports {
            if let Some(root) = library_root_of_interface(&import.module.interface_path) {
                roots.insert(root);
            }
        }
    }
    roots
}

/// Install the library `[target.<spelling>]` bindings on the retry's Config.
///
/// The package-analysis retry re-emits the entry module after the single-file
/// route refused an import-reaching `comptime`. Only WGSL consumes target
/// bindings; every other target keeps the config untouched. A manifest that
/// cannot be read fails the emit with its load diagnostics.
pub(super) fn config_with_library_bindings(
    config: radix::driver::Config,
    command: &EmitCommand,
    package: &radix::program::analyze::AnalyzedProgram,
) -> Result<radix::driver::Config, Vec<radix::module::diagnostics::Diagnostic>> {
    if command.target != radix::module::codegen::Target::MirWgsl {
        return Ok(config);
    }
    let roots = library_package_roots(package);
    let (bindings, diagnostics) =
        merge_library_target_bindings(&roots, &command.target.to_string());
    if diagnostics.is_empty() {
        Ok(config.with_target_bindings(bindings))
    } else {
        Err(diagnostics)
    }
}

/// Single-file `wgsl-text` emit: each imported library's `[target.wgsl-text]`
/// bindings (read by Faber, opaque to the compiler) are merged and installed.
/// A manifest that cannot be read is reported as an error diagnostic.
fn compile_wgsl_with_library_bindings(
    command: &EmitCommand,
    code_pack: Option<&radix::module::locale::LocalePack>,
) -> radix::CompileResult {
    let target_spelling = command.target.to_string();
    let load_errors = std::cell::RefCell::new(Vec::new());
    let resolve = |roots: &std::collections::BTreeSet<std::path::PathBuf>| {
        let (merged, diagnostics) = merge_library_target_bindings(roots, &target_spelling);
        load_errors.borrow_mut().extend(diagnostics);
        merged
    };
    let mut result = radix::services::compile_cli_input_with_library_bindings(
        &command.input,
        command.package,
        command.target,
        code_pack,
        command.output_mode,
        command.module_name.as_deref(),
        &resolve,
    );
    let errors = load_errors.into_inner();
    if !errors.is_empty() {
        result.output = None;
        result.diagnostics.extend(errors);
    }
    result
}

/// Emit a single-file target with Faber-owned reader-locale resolution.
///
/// TypeScript remains on Radix's direct-file route, but it still needs the
/// package-aware locale resolver here when a library source file uses a
/// non-Latin reader surface. Keeping this seam in Faber avoids asking Radix
/// to rediscover Faber's install and package layout.
pub fn cmd_emit_with_locale(command: EmitCommand, format: bool, linter: bool) {
    if command.package {
        eprintln!("error: package emit must use the package compiler route");
        faber_cli::exit_status::exit_usage();
    }

    // Faber owns locale → pack resolution (install layout + package manifests
    // live here, not in radix). Code pack drives emit surface; diagnostic pack
    // drives message rendering (`--diagnostics-locale` when set).
    let code_pack = match faber_cli::package::locale_pack_for_emit_with_diagnostic(
        &command.input,
        command.locale.as_deref(),
    ) {
        Ok(pack) => pack,
        Err(diagnostic) => report_locale_diagnostic(&diagnostic),
    };
    let diagnostic_pack = match command.diagnostics_locale.as_deref() {
        Some(locale) => {
            match faber_cli::package::locale_pack_for_emit_with_diagnostic(
                &command.input,
                Some(locale),
            ) {
                Ok(pack) => pack,
                Err(diagnostic) => report_locale_diagnostic(&diagnostic),
            }
        }
        None => code_pack.clone(),
    };

    let result = if command.target == radix::module::codegen::Target::MirWgsl {
        compile_wgsl_with_library_bindings(&command, code_pack.as_ref())
    } else {
        radix::services::compile_cli_input_with_locale_pack(
            &command.input,
            command.package,
            command.target,
            code_pack.as_ref(),
            command.output_mode,
            command.module_name.as_deref(),
        )
    };
    let result = retry_single_file_via_package_analysis(&command, code_pack.as_ref(), result);

    faber_cli::postprocess::print_diagnostics(
        &result.diagnostics,
        command.diagnostic_mode,
        diagnostic_pack.as_ref(),
    );

    let Some(output) = result.output else {
        eprintln!("compilation failed");
        faber_cli::exit_status::exit_data_error();
    };

    if command.reflection {
        #[cfg(not(feature = "runner"))]
        super::exit_runner_feature_unavailable("emit --reflection");
        #[cfg(feature = "runner")]
        {
            let reflection_json = match radix::services::output_reflection_json(&output) {
                Ok(Some(json)) => json,
                Ok(None) => {
                    eprintln!("error: target does not expose GPU reflection metadata");
                    faber_cli::exit_status::exit_usage();
                }
                Err(err) => {
                    eprintln!("error: failed to serialize GPU reflection metadata: {err}");
                    faber_cli::exit_status::exit_software();
                }
            };
            if let Some(path) = command.output {
                faber_cli::postprocess::write_text_artifact(&path, &reflection_json);
            } else {
                println!("{reflection_json}");
            }
            return;
        }
    }

    if let Some(path) = command.output {
        faber_cli::postprocess::write_output_artifact(
            &path,
            output,
            command.target,
            format,
            linter,
        );
        return;
    }

    let code = faber_cli::postprocess::postprocess_code(
        radix::services::output_code(output),
        command.target,
        format,
        linter,
    );
    print!("{code}");
}

/// ST2-2.10: when the single-file route refused an import-reaching
/// `comptime` initializer (`praefixum_import_unavailable`), run the package
/// analysis once over the same input and emit the entry module from that
/// analysis.
///
/// The issue is the structured identity, never prose; the retry runs once and
/// its result, success or failure, replaces the single-file result. Any other
/// diagnostic and any stdin, directory or non-`.fab` input keeps today's path.
pub(super) fn retry_single_file_via_package_analysis(
    command: &EmitCommand,
    code_pack: Option<&radix::module::locale::LocalePack>,
    single_file: radix::CompileResult,
) -> radix::CompileResult {
    let Some(path) = retry_input_path(command) else {
        return single_file;
    };
    if single_file.output.is_some() {
        return single_file;
    }
    if !single_file
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.issue() == Some("praefixum_import_unavailable"))
    {
        return single_file;
    }
    emit_through_package_analysis(command, code_pack, path)
}

/// The `.fab` path a retry may act on: stdin, an empty input and directories
/// stay on the single-file route.
fn retry_input_path(command: &EmitCommand) -> Option<&Path> {
    let first = command.input.first()?;
    if first.is_empty() || first == "-" {
        return None;
    }
    let path = Path::new(first.as_str());
    if path.extension().and_then(std::ffi::OsStr::to_str) != Some("fab") {
        return None;
    }
    if path.is_dir() {
        return None;
    }
    Some(path)
}

/// Emit the input's entry module through the package analysis (ST2-2.10).
///
/// The package analysis evaluates the deferred `comptime` sites against the
/// linked local import closure; the entry node's analyzed module then travels
/// the same analysis-to-output tail as the single-file route, with the
/// command's target, reader pack, output mode and module name.
pub(super) fn emit_through_package_analysis(
    command: &EmitCommand,
    code_pack: Option<&radix::module::locale::LocalePack>,
    input: &Path,
) -> radix::CompileResult {
    #[cfg(test)]
    PACKAGE_ANALYSIS_ENTRIES.with(|entries| entries.set(entries.get() + 1));

    // Package discovery requires an absolute input: a raw `c.fab` lexically
    // resolves its package root to the empty path and refuses every local
    // import as `package_import_escape`.
    let input = faber_cli::package::absolutize_path(input);
    let config = package_analysis_config(command, code_pack);
    let mut package = match radix::program::analyze::analyze_package(
        &config,
        &input,
        &faber_cli::package::faber_analysis_seams(),
    ) {
        Ok(package) => package,
        Err(diagnostics) => {
            return radix::CompileResult {
                output: None,
                diagnostics,
            };
        }
    };
    let config = match config_with_library_bindings(config, command, &package) {
        Ok(config) => config,
        Err(diagnostics) => {
            return radix::CompileResult {
                output: None,
                diagnostics,
            };
        }
    };
    let source = match std::fs::read_to_string(&input) {
        Ok(source) => source,
        Err(err) => {
            return radix::CompileResult {
                output: None,
                diagnostics: vec![
                    radix::module::diagnostics::Diagnostic::io_error(&input, &err)
                        .with_code("IO001")
                        .with_arg("issue", "source_read_failed"),
                ],
            };
        }
    };
    let Some(entry) = package.entry.clone() else {
        return radix::CompileResult {
            output: None,
            diagnostics: package.diagnostics,
        };
    };
    let Some(node) = package.nodes.remove(&entry) else {
        return radix::CompileResult {
            output: None,
            diagnostics: package.diagnostics,
        };
    };
    let name = input.display().to_string();
    let session = radix::driver::Session::new(config);
    let mut result = radix::driver::compile_analyzed(&session, &name, &source, node.analysis);
    result.diagnostics.extend(package.diagnostics);
    result
}

/// The single-file route's `Config` shape, rebuilt for the retry: target,
/// output mode, module name and non-explicit reader pack.
pub(super) fn package_analysis_config(
    command: &EmitCommand,
    code_pack: Option<&radix::module::locale::LocalePack>,
) -> radix::driver::Config {
    let config = radix::driver::Config::default()
        .with_target(command.target)
        .with_output_mode(command.output_mode)
        .with_locale_pack_loader(faber_cli::package::embedded_locale_pack_loader())
        .with_dev_locale_root();
    let config = match &command.module_name {
        Some(name) => config.with_module_name(Some(name.clone())),
        None => config,
    };
    match code_pack {
        Some(pack) => config.with_locale_pack(pack.clone()),
        None => config,
    }
}

pub fn is_faber_emit(target: FaberCliTarget) -> bool {
    target.is_faber()
}
