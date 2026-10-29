//! Per-unit TypeScript file sets for the Node project build.
//!
//! Every unit of a package becomes its own ES module (the entry in script
//! mode, siblings in library mode), and every provider module the package
//! reaches (`norma:*`, locked libraries) becomes `<provider>-<module>.ts`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use radix::module::Output;
use radix::module::codegen::Target;
use radix::program::analyze::{AnalyzedProgram, ModuleId};

/// The TypeScript backend's target identity.
pub(crate) const TS_TARGET: Target = Target::HirTypeScript;

/// One generated file of the Node project: a path relative to `src/` and its
/// TypeScript text.
pub(crate) struct TsProjectFile {
    pub path: PathBuf,
    pub code: String,
}

/// The per-unit TypeScript file set of a package.
pub(crate) struct TsProjectFiles {
    /// The entry unit's path relative to `src/` (script mode).
    pub entry: PathBuf,
    /// Every unit, entry included, in dependency order.
    pub files: Vec<TsProjectFile>,
}

/// Emit every unit of the package as its own ES module: the entry in script
/// mode, each sibling in library mode with named exports. Relative imports
/// carry `.js` and resolve between the generated files under `NodeNext`.
///
/// # Errors
/// Returns the diagnostics when the package has no entry or a unit fails TypeScript
/// codegen.
pub(crate) fn generate_ts_project_files(
    package: &AnalyzedProgram,
) -> Result<TsProjectFiles, Vec<radix::module::diagnostics::Diagnostic>> {
    let Some(entry) = package.entry_node() else {
        return Err(vec![
            crate::package_diagnostic_error("package has no entry unit for TypeScript assembly")
                .with_arg("issue", "package_ts_entry_missing"),
        ]);
    };
    let latin = radix::module::locale::latin_locale_pack();
    let surface = radix::module::locale::KeywordSurface::new(&latin);
    let root = common_unit_root(package);

    let mut order: Vec<ModuleId> = Vec::new();
    let mut visited: BTreeSet<ModuleId> = BTreeSet::new();
    if let Some(id) = &package.entry {
        visit(package, id, &mut visited, &mut order);
    }
    for id in package.nodes.keys() {
        visit(package, id, &mut visited, &mut order);
    }

    let mut files = Vec::new();
    for id in order {
        let Some(unit) = package.nodes.get(&id) else {
            continue;
        };
        let is_entry = unit.path == entry.path;
        let stem = unit
            .path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("main")
            .to_owned();
        let module_name = unit.module_segments.last().cloned().unwrap_or(stem);
        let mode = if is_entry {
            radix::module::codegen::OutputMode::Application
        } else {
            radix::module::codegen::OutputMode::Library
        };
        let code = match radix::module::codegen::generate_from_analyzed_with_options(
            TS_TARGET,
            &unit.analysis,
            &surface,
            mode,
            Some(&module_name),
        ) {
            Ok(Output::TypeScript(output)) => output.code,
            Ok(_) => {
                return Err(vec![
                    crate::package_diagnostic_error(
                        "TypeScript package codegen returned a non-TypeScript output",
                    )
                    .with_file(unit.path.display().to_string())
                    .with_arg("issue", "package_ts_codegen_failed"),
                ]);
            }
            Err(err) => {
                let mut diag = crate::package_diagnostic_error(err.message)
                    .with_file(unit.path.display().to_string())
                    .with_arg("issue", "package_ts_codegen_failed");
                for arg in err.args {
                    diag = diag.with_arg(arg.name, arg.value);
                }
                return Err(vec![diag]);
            }
        };
        let relative = unit
            .path
            .strip_prefix(&root)
            .unwrap_or(&unit.path)
            .with_extension("ts");
        files.push(TsProjectFile {
            path: relative,
            code,
        });
    }
    let entry_path = entry
        .path
        .strip_prefix(&root)
        .unwrap_or(&entry.path)
        .with_extension("ts");
    Ok(TsProjectFiles {
        entry: entry_path,
        files,
    })
}

/// The deepest directory that holds every unit of the package, so relative
/// imports between units keep their meaning in the generated tree.
fn common_unit_root(package: &AnalyzedProgram) -> PathBuf {
    let mut root: Option<PathBuf> = None;
    for unit in package.nodes.values() {
        let dir = unit
            .path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        root = Some(match root {
            None => dir,
            Some(current) => current
                .components()
                .zip(dir.components())
                .take_while(|(a, b)| a == b)
                .map(|(a, _)| a)
                .collect(),
        });
    }
    root.unwrap_or_default()
}

/// Emit every provider module the package reaches (`norma:*` and locked
/// libraries) as its own library-mode ES module `<provider>-<module>.ts` at
/// the project's `src/` root, matching the specifiers the importing units
/// emit (`./norma-console.js`). `norma:math`, `norma:time` and `norma:json`
/// lower to native JavaScript at their call sites and have no module.
///
/// # Errors
/// Returns the diagnostics of the first module that fails to analyze or emit.
pub(crate) fn generate_ts_library_files(
    package: &AnalyzedProgram,
    config: &radix::module::driver::Config,
) -> Result<Vec<TsProjectFile>, Vec<radix::module::diagnostics::Diagnostic>> {
    let latin = radix::module::locale::latin_locale_pack();
    let surface = radix::module::locale::KeywordSurface::new(&latin);
    let mut cache = super::library_cache_with_config(config);
    let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
    let mut files = Vec::new();
    for unit in package.nodes.values() {
        for import in &unit.expanded_library_imports {
            let package_name = import.module.package.as_str();
            let segments = &import.module.module_path;
            if package_name == "norma"
                && segments.len() == 1
                && matches!(segments[0].as_str(), "math" | "time" | "json")
            {
                continue;
            }
            if !seen.insert((package_name.to_owned(), segments.join("/"))) {
                continue;
            }
            let stem = format!("{package_name}-{}", segments.join("-"));
            let module_name = segments.last().cloned().unwrap_or_else(|| stem.clone());
            let code = radix::program::library::with_library_cached_analysis_mut(
                import,
                &package.library_resolver,
                &mut cache,
                |analysis, _| match radix::module::codegen::generate_from_analyzed_with_options(
                    TS_TARGET,
                    &*analysis,
                    &surface,
                    radix::module::codegen::OutputMode::Library,
                    Some(&module_name),
                )? {
                    Output::TypeScript(output) => Ok(output.code),
                    _ => Err(radix::module::codegen::CodegenError {
                        message: "TypeScript library codegen returned a non-TypeScript output"
                            .to_owned(),
                        args: Vec::new(),
                    }),
                },
            )
            .map_err(|diag| vec![diag])?;
            files.push(TsProjectFile {
                path: PathBuf::from(format!("{stem}.ts")),
                code,
            });
        }
    }
    Ok(files)
}

pub(super) fn visit(
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
