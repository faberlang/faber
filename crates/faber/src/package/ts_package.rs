//! Entry-only TypeScript package emission seam.
//!
//! This unit wires the product seam for a single-unit package: the entry unit
//! is emitted through the shared HIR→TypeScript codegen. Multi-unit packages
//! still report the assembly-pending diagnostic; the fold that assembles
//! sibling modules arrives in the next unit.

use std::path::Path;

use radix::module::CompileResult;
use radix::module::Output;
use radix::module::codegen::Target;
use radix::program::analyze::AnalyzedProgram;

fn failure(diagnostics: Vec<radix::module::diagnostics::Diagnostic>) -> CompileResult {
    CompileResult {
        output: None,
        diagnostics,
    }
}

pub(crate) fn generate_package_ts_result(package: &AnalyzedProgram, input: &Path) -> CompileResult {
    let mut diagnostics = package.diagnostics.clone();

    if package.nodes.values().any(|unit| !unit.is_entry) {
        diagnostics.push(
            crate::package_diagnostic_error(
                "package compilation has a ts artifact plan but full product assembly is not implemented yet",
            )
            .with_file(input.display().to_string())
            .with_arg("issue", "package_target_assembly_pending")
            .with_arg("target", "ts"),
        );
        return failure(diagnostics);
    }

    let Some(entry) = package.entry_node() else {
        diagnostics.push(
            crate::package_diagnostic_error("package has no entry unit for TypeScript assembly")
                .with_file(input.display().to_string())
                .with_arg("issue", "package_ts_entry_missing"),
        );
        return failure(diagnostics);
    };

    // TypeScript is not a localized target; the surface is unused but required
    // by the shared dispatch seam after reader-locale emit threading.
    let latin = radix::module::locale::latin_locale_pack();
    let surface = radix::module::locale::KeywordSurface::new(&latin);
    let code = match radix::module::codegen::generate_from_analyzed(
        Target::HirTypeScript,
        &entry.analysis,
        &surface,
    ) {
        Ok(Output::TypeScript(output)) => output.code,
        Ok(_) => {
            diagnostics.push(
                crate::package_diagnostic_error(
                    "TypeScript package codegen returned a non-TypeScript output",
                )
                .with_file(entry.path.display().to_string())
                .with_arg("issue", "package_ts_codegen_failed"),
            );
            return failure(diagnostics);
        }
        Err(err) => {
            let mut diag = crate::package_diagnostic_error(err.message)
                .with_file(entry.path.display().to_string())
                .with_arg("issue", "package_ts_codegen_failed");
            for arg in err.args {
                diag = diag.with_arg(arg.name, arg.value);
            }
            diagnostics.push(diag);
            return failure(diagnostics);
        }
    };

    CompileResult {
        output: Some(Output::TypeScript(radix::module::TypeScriptOutput { code })),
        diagnostics,
    }
}
