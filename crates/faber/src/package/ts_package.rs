//! TypeScript package emission seam.
//!
//! Every unit of the package (entry plus siblings) is emitted through the
//! shared HIR→TypeScript codegen and post-processed, then the units are folded
//! into one stream by the pure `merge_package_modules`.

use std::collections::BTreeSet;
use std::path::Path;

use radix::module::CompileResult;
use radix::module::Output;
use radix::module::codegen::Target;
use radix::program::analyze::{AnalyzedProgram, AnalyzedProgramNode, ModuleId};

use super::product::{augment_namespace_imports, top_level_ts_decl_names, wrap_module_exports};

fn failure(diagnostics: Vec<radix::module::diagnostics::Diagnostic>) -> CompileResult {
    CompileResult {
        output: None,
        diagnostics,
    }
}

pub(crate) fn generate_package_ts_result(package: &AnalyzedProgram, input: &Path) -> CompileResult {
    let mut diagnostics = package.diagnostics.clone();

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

    let mut generated: Vec<(&AnalyzedProgramNode, String)> = Vec::new();
    for unit in package.nodes.values() {
        let raw = match radix::module::codegen::generate_from_analyzed(
            Target::HirTypeScript,
            &unit.analysis,
            &surface,
        ) {
            Ok(Output::TypeScript(output)) => output.code,
            Ok(_) => {
                diagnostics.push(
                    crate::package_diagnostic_error(
                        "TypeScript package codegen returned a non-TypeScript output",
                    )
                    .with_file(unit.path.display().to_string())
                    .with_arg("issue", "package_ts_codegen_failed"),
                );
                return failure(diagnostics);
            }
            Err(err) => {
                let mut diag = crate::package_diagnostic_error(err.message)
                    .with_file(unit.path.display().to_string())
                    .with_arg("issue", "package_ts_codegen_failed");
                for arg in err.args {
                    diag = diag.with_arg(arg.name, arg.value);
                }
                diagnostics.push(diag);
                return failure(diagnostics);
            }
        };
        let local_names = top_level_ts_decl_names(unit);
        let code = augment_namespace_imports(raw, &unit.namespace_exports, &local_names);
        let module_name = unit.module_segments.last().map_or("main", String::as_str);
        let code = wrap_module_exports(code, module_name);
        generated.push((unit, code));
    }

    let Some(entry_code) = generated
        .iter()
        .find(|(unit, _)| unit.path == entry.path)
        .map(|(_, code)| code.as_str())
    else {
        diagnostics.push(
            crate::package_diagnostic_error("entry unit produced no TypeScript output")
                .with_file(input.display().to_string())
                .with_arg("issue", "package_ts_codegen_failed"),
        );
        return failure(diagnostics);
    };

    let modules = ordered_module_units(package, &generated);
    let internal_specifiers = internal_import_specifiers(package);
    match radix::module::codegen::ts::merge_package_modules(
        entry_code,
        &modules,
        &internal_specifiers,
    ) {
        Ok(code) => CompileResult {
            output: Some(Output::TypeScript(radix::module::TypeScriptOutput { code })),
            diagnostics,
        },
        Err(err) => {
            diagnostics.push(
                crate::package_diagnostic_error(err.to_string())
                    .with_file(input.display().to_string())
                    .with_arg("issue", "package_ts_merge_collision")
                    .with_arg("target", "ts"),
            );
            failure(diagnostics)
        }
    }
}

/// Import specifiers in the package that name another unit of the same package
/// (`./` or `../` relative paths). The fold drops those import lines.
fn internal_import_specifiers(package: &AnalyzedProgram) -> BTreeSet<String> {
    let mut specifiers = BTreeSet::new();
    for unit in package.nodes.values() {
        for item in &unit.analysis.hir.items {
            let radix::module::hir::HirItemKind::Import(import) = &item.kind else {
                continue;
            };
            let path = unit.analysis.interner.resolve(import.path);
            if path.starts_with("./") || path.starts_with("../") {
                specifiers.insert(path.to_owned());
            }
        }
    }
    specifiers
}

/// Non-entry `(file name, code)` pairs, dependencies first.
///
/// Post-order DFS over the local import graph: dependencies are pushed before
/// the unit that imports them. Starts at the package entry, then sweeps any
/// never-visited node in map order as a stable fallback. The entry unit is
/// omitted because it is the fold head.
fn ordered_module_units(
    package: &AnalyzedProgram,
    generated: &[(&AnalyzedProgramNode, String)],
) -> Vec<(String, String)> {
    let mut visited: BTreeSet<ModuleId> = BTreeSet::new();
    let mut order: Vec<ModuleId> = Vec::new();
    if let Some(entry) = &package.entry {
        visit(package, entry, &mut visited, &mut order);
    }
    for id in package.nodes.keys() {
        visit(package, id, &mut visited, &mut order);
    }

    let mut modules = Vec::new();
    for id in order {
        if package.entry.as_ref() == Some(&id) {
            continue;
        }
        let Some(unit) = package.nodes.get(&id) else {
            continue;
        };
        let Some((_, code)) = generated
            .iter()
            .find(|(candidate, _)| candidate.path == unit.path)
        else {
            continue;
        };
        let file_name = if unit.module_segments.is_empty() {
            "main.ts".to_owned()
        } else {
            format!("{}.ts", unit.module_segments.join("_"))
        };
        modules.push((file_name, code.clone()));
    }
    modules
}

fn visit(
    package: &AnalyzedProgram,
    id: &ModuleId,
    visited: &mut BTreeSet<ModuleId>,
    order: &mut Vec<ModuleId>,
) {
    if !visited.insert(id.clone()) {
        return;
    }
    for successor in package.imports.local_successors(id) {
        visit(package, successor, visited, order);
    }
    order.push(id.clone());
}
