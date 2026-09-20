//! `faber emit -t faber` policy wrapper.

use crate::cli::FaberCliTarget;
use radix::services::EmitCommand;

pub fn cmd_emit_faber(command: EmitCommand, format: bool, linter: bool) {
    if command.package {
        eprintln!("error: package Faber emit is not supported; use single-file input");
        std::process::exit(1);
    }

    cmd_emit_with_locale(command, format, linter);
}

fn report_locale_diagnostic(diagnostic: &radix::module::diagnostics::Diagnostic) -> ! {
    if diagnostic.issue() == Some("package_path_missing") {
        eprintln!("{}", radix::module::diagnostics::render_plain(diagnostic));
    } else {
        eprintln!("error: {}", diagnostic.message);
    }
    std::process::exit(1);
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
        std::process::exit(1);
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
            match faber_cli::package::locale_pack_for_emit_with_diagnostic(&command.input, Some(locale))
            {
                Ok(pack) => pack,
                Err(diagnostic) => report_locale_diagnostic(&diagnostic),
            }
        }
        None => code_pack.clone(),
    };

    let result = radix::services::compile_cli_input_with_locale_pack(
        &command.input,
        command.package,
        command.target,
        code_pack.as_ref(),
        command.output_mode,
        command.module_name.as_deref(),
    );

    faber_cli::postprocess::print_diagnostics(
        &result.diagnostics,
        command.diagnostic_mode,
        diagnostic_pack.as_ref(),
    );

    let Some(output) = result.output else {
        eprintln!("compilation failed");
        std::process::exit(1);
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
                    std::process::exit(1);
                }
                Err(err) => {
                    eprintln!("error: failed to serialize GPU reflection metadata: {err}");
                    std::process::exit(1);
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
        faber_cli::postprocess::write_output_artifact(&path, output, command.target, format, linter);
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

pub fn is_faber_emit(target: FaberCliTarget) -> bool {
    target.is_faber()
}
