use super::build::BrowserController;
use super::ts_rewrite::{
    adapt_controller_typescript, augment_namespace_imports, normalize_library_namespace_bindings,
    rewrite_import_specifiers, top_level_ts_decl_names, ts_lib_module_naming, ts_module_file_name,
    wrap_module_exports, DOM_TYPE_ALIASES,
};
use super::{fs, io_diag, product_diag, BTreeMap, BTreeSet, Diagnostic, Path, PathBuf};

pub(super) fn emit_typescript_modules(
    package: &super::super::AnalyzedPackage,
    ts_root: &Path,
    controllers: &[BrowserController],
    library_imports: &BTreeMap<String, String>,
) -> Result<(), Box<Diagnostic>> {
    let latin = radix::locale::latin_locale_pack();
    let surface = radix::locale::KeywordSurface::new(&latin);
    for unit in &package.units {
        let code = match radix::codegen::generate_from_analyzed(
            radix::codegen::Target::TypeScript,
            &unit.analysis,
            &surface,
        ) {
            Ok(radix::Output::TypeScript(output)) => output.code,
            Ok(_) => {
                return Err(Box::new(
                    product_diag("TypeScript product codegen returned a non-TypeScript output")
                        .with_file(unit.path.display().to_string())
                        .with_arg("issue", "product_typescript_codegen_failed"),
                ))
            }
            Err(err) => {
                let mut diag = product_diag(err.message)
                    .with_file(unit.path.display().to_string())
                    .with_arg("issue", "product_typescript_codegen_failed");
                for arg in err.args {
                    diag = diag.with_arg(arg.name, arg.value);
                }
                return Err(Box::new(diag));
            }
        };
        let local_names = top_level_ts_decl_names(unit);
        let code = adapt_controller_typescript(code, controllers);
        let code = augment_namespace_imports(code, &unit.namespace_exports, &local_names);
        let module_name = unit
            .module_segments
            .last()
            .map(|s| s.as_str())
            .unwrap_or("main");
        let code = wrap_module_exports(code, module_name);
        let code = rewrite_import_specifiers(code, library_imports);
        let path = ts_root.join(ts_module_file_name(unit));
        fs::write(&path, code).map_err(|err| io_diag(&path, err))?;
    }
    Ok(())
}

/// Apply emit-defect post-processing for library TypeScript files.
///
/// Phase 3: these are minimal patches for codegen defects that would otherwise
/// block TypeScript compilation. Each patch corresponds to a filed radix issue
/// so the root cause is tracked in the compiler, not papered over indefinitely.
fn apply_library_emit_fixes(mut code: String) -> String {
    // Fix 1: construction of unresolved types → empty object (value position).
    // Handle both bare and marker forms before any type-position rewrite.
    code = code.replace("new unresolved_def()", "{}");
    code = code.replace("new /* unresolved_def */()", "{}");

    // Fix 2: Codegen marker `/* unresolved_def */` is not a valid type position
    // (`/* any */` after a naive substring replace is still invalid). Use `any`.
    code = code.replace("/* unresolved_def */", "any");

    // Fix 3: bare `unresolved_def` identifiers still need rewriting.
    code = code.replace("unresolved_def", "any");

    // Fix 4: if a value-position rewrite was missed, `new any()` is still illegal.
    code = code.replace("new any()", "{}");

    // Radix codegen emits IIFE array-access expressions as the left-hand side
    // of assignment, which TypeScript rejects (TS2364).  Rewrite:
    //   ((__o, __i) => { ... })(arr, idx) = value;
    // → arr[idx] = value;
    //
    // The IIFE pattern (index trap form; keeps a stored `undefined` distinct
    // from a missing index via the `in` check):
    //   ((__o, __i) => { if (!(__i in __o)) throw new Error("index trap"); return __o[__i]; })(arr, idx)
    // We replace it with a plain arr[idx] when it appears as LHS of `=`.
    let iife_pattern = r#"((__o, __i) => { if (!(__i in __o)) throw new Error("index trap"); return __o[__i]; })("#;
    // Process line by line to handle IIFE-on-LHS.
    let mut result = String::with_capacity(code.len() + 4096);
    for line in code.lines() {
        let trimmed = line.trim();
        // Detect IIFE-as-LHS: line matches pattern `((__o, ...)(expr, idx) = ...`
        if trimmed.starts_with(iife_pattern) && trimmed.contains(") = ") {
            // Extract array expression and index from IIFE call.
            // Pattern: ((__o, __i) => { ... })(array_expr, index_expr) = value_expr
            let rest = &trimmed[iife_pattern.len()..];
            // Find the closing paren of the IIFE call.
            if let Some(close_idx) = rest.rfind(") = ") {
                let args_part = &rest[..close_idx];
                // Split at the comma to get array and index
                if let Some(comma_idx) = args_part.rfind(',') {
                    let array_expr = args_part[..comma_idx].trim();
                    let index_expr = args_part[comma_idx + 1..].trim();
                    let value_part = rest[close_idx + 4..].trim(); // after ") = "
                                                                   // Determine indentation from original line
                    let indent = &line[..line.len() - line.trim_start().len()];
                    result.push_str(&format!(
                        "{}{}[{}] = {};\n",
                        indent,
                        array_expr,
                        index_expr,
                        value_part.trim_end_matches(';').trim()
                    ));
                    continue;
                }
            }
        }
        result.push_str(line);
        result.push('\n');
    }
    result
}

/// Resolve the faber CLI binary for subprocess `emit`.
///
/// Unit tests run under a cargo test harness binary (`deps/faber-<hash>`), so
/// `current_exe()` is not a usable CLI. Prefer `CARGO_BIN_EXE_faber`, then a
/// sibling `faber` next to `deps/`, then `current_exe` when it *is* the CLI.
fn resolve_faber_cli_binary() -> PathBuf {
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_faber") {
        return PathBuf::from(path);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(name) = exe.file_name().and_then(|n| n.to_str()) {
            if name == "faber" || name == "faber.exe" {
                return exe;
            }
        }
        if let Some(parent) = exe.parent() {
            // .../debug/deps/faber-<hash> → .../debug/faber
            if parent.file_name().and_then(|n| n.to_str()) == Some("deps") {
                if let Some(debug_dir) = parent.parent() {
                    let candidate = debug_dir.join("faber");
                    if candidate.is_file() {
                        return candidate;
                    }
                }
            }
            let sibling = parent.join("faber");
            if sibling.is_file() {
                return sibling;
            }
        }
    }
    PathBuf::from("faber")
}

/// Emit TypeScript for library dependencies (kind=lib, target=ts) into the
/// TypeScript output directory alongside app modules.
///
/// Reads `faber.lock` to discover TS-targeting library packages, then
/// emits each `.fab` source file via `faber emit -t ts` (same approach
/// as `link-triga-ts.mjs`). Emitted files are named `{package}-{module}.ts`
/// and get namespace-wrapped exports the same way app modules do.
///
/// Phase 2: minimal emit — no import rewriting, no tsc passthrough.
/// Phase 3: post-processing fixes for known codegen defects (unresolved_def,
///          IIFE-as-LHS) applied via [`apply_library_emit_fixes`].
pub(super) fn emit_library_typescript_modules(
    _config: &radix::driver::Config,
    package_root: &Path,
    ts_root: &Path,
    library_imports: &BTreeMap<String, String>,
) -> Result<(), Box<Diagnostic>> {
    let lock = match super::super::lockfile::read_lock(package_root)? {
        Some(lock) => lock,
        None => return Ok(()),
    };
    let lock_path = package_root.join(super::super::lockfile::LOCK_FILE);
    let index = super::super::lockfile::lock_index(&lock_path, &lock).map_err(|mut diags| {
        diags
            .pop()
            .unwrap_or_else(|| product_diag("failed to index faber.lock for library TS emit"))
    })?;

    // Locate the faber binary for subprocess emit.
    // Prefer the real CLI binary (not the cargo test harness binary).
    let faber_bin = resolve_faber_cli_binary();

    // Pass 1: emit all library TS files into memory.
    // We need all files emitted before augmenting imports so cross-module
    // type references can be resolved.
    struct EmittedLibFile {
        ts_path: PathBuf,
        stem: String,
        code: String,
    }
    let mut emitted: Vec<EmittedLibFile> = Vec::new();

    for pkg in index.values() {
        if pkg.kind != "lib" || pkg.target_language != "ts" {
            continue;
        }
        let pkg_root = pkg.package_root_path(package_root);
        let src_dir = pkg_root.join("src");
        if !src_dir.is_dir() {
            continue;
        }
        // FBR-P2-001: directory read errors anywhere in the tree are
        // diagnostic failures, never silently dropped modules.
        let src_files = library_src_fab_files(&src_dir)?;

        // Prefer package-owned TS binding shims (e.g. faber-web/runtime/dom.ts)
        // over emitting `nota` stubs from .fab — stubs only console.log.
        let ts_bindings = load_ts_library_bindings(&pkg_root)?;

        for fab_path in &src_files {
            let Some(naming) = ts_lib_module_naming(&pkg.name, &src_dir, fab_path) else {
                continue;
            };
            let ts_path = ts_root.join(&naming.file_name);

            if let Some(bindings) = ts_bindings.as_ref() {
                if let Some(exports) = bindings.module_exports(&naming.leaf_stem) {
                    if !exports.is_empty() {
                        emit_ts_binding_facade(
                            &pkg_root,
                            &pkg.name,
                            &naming.leaf_stem,
                            &naming.file_name,
                            bindings,
                            &exports,
                            ts_root,
                        )?;
                        continue;
                    }
                }
            }

            let output = std::process::Command::new(&faber_bin)
                .args(["emit", "-t", "ts"])
                .arg(fab_path)
                .output();
            let output = match output {
                Ok(output) => output,
                Err(err) => {
                    eprintln!(
                        "faber: library TS emit subprocess failed for `{}`: {err}",
                        fab_path.display()
                    );
                    continue;
                }
            };
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                eprintln!(
                    "faber: library TS emit failed for `{}`: {stderr}",
                    fab_path.display()
                );
                continue;
            }
            let code = String::from_utf8_lossy(&output.stdout).to_string();
            emitted.push(EmittedLibFile {
                ts_path,
                stem: naming.leaf_stem,
                code,
            });
        }
    }

    // Pass 2: build namespace export map from emitted files.
    // Each library module's raw declarations become its namespace exports.
    // Other files that import that module namespace need the individual type
    // names added to the import statement.
    let mut namespace_exports: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for file in &emitted {
        let exports = collect_ts_bare_decl_names(&file.code);
        namespace_exports
            .entry(file.stem.clone())
            .or_default()
            .extend(exports);
    }

    // Pass 3: write each file with import augmentation applied.
    for file in &emitted {
        // Phase 3: apply emit-defect fixes before namespace wrapping.
        let code = apply_library_emit_fixes(file.code.clone());
        let code = wrap_module_exports(code, &file.stem);
        // Emitted modules export their leaf stem as the namespace const; a
        // `privata` binding that differs (e.g. `import { lighting } from
        // "triga:lighting/light"`) must alias the leaf stem or tsc fails
        // (TS2305).
        let code = normalize_library_namespace_bindings(code, library_imports);
        // Augment namespace imports with named type exports so TypeScript
        // can resolve cross-module type references (e.g. Vector3 from math).
        let local_names = collect_ts_local_decl_names(&code);
        let code = augment_namespace_imports(code, &namespace_exports, &local_names);
        // Phase 4: one-pass rewrite of import/export specifier tokens.
        let code = rewrite_import_specifiers(code, library_imports);
        fs::write(&file.ts_path, code).map_err(|err| io_diag(&file.ts_path, err))?;
    }

    Ok(())
}

/// Collect top-level declaration names from raw TS codegen output.
/// Only matches declarations at indent level 0 (no leading whitespace).
/// The subprocess emit does not add `export` keywords — those are added
/// later by `wrap_module_exports`.
fn collect_ts_bare_decl_names(code: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in code.lines() {
        // Only top-level declarations (no leading whitespace).
        if line.chars().next().is_some_and(|c| c.is_whitespace()) {
            continue;
        }
        for prefix in &[
            "class ",
            "function ",
            "enum ",
            "interface ",
            "type ",
            "const ",
        ] {
            if let Some(rest) = line.strip_prefix(prefix) {
                let name = rest
                    .split(|c: char| !c.is_alphanumeric() && c != '_')
                    .next()
                    .unwrap_or("");
                if !name.is_empty() {
                    names.push(name.to_owned());
                }
            }
        }
    }
    names
}

/// Collect top-level declaration names from emitted TS code.
fn collect_ts_local_decl_names(code: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for line in code.lines() {
        let trimmed = line.trim_start();
        for prefix in &[
            "export class ",
            "export function ",
            "export enum ",
            "export interface ",
            "export type ",
            "export const ",
        ] {
            if let Some(rest) = trimmed.strip_prefix(prefix) {
                let name = rest
                    .split(|c: char| !c.is_alphanumeric() && c != '_')
                    .next()
                    .unwrap_or("");
                if !name.is_empty() {
                    names.insert(name.to_owned());
                }
            }
        }
    }
    names
}

/// Parsed `[target.ts]` binding manifest for a TS library package.
struct TsLibraryBindings {
    /// Absolute path to the package-owned runtime shim (e.g. `runtime/dom.ts`).
    shim_path: PathBuf,
    /// Keyed by function route: `"web:dom.attr_set"` → `"webDomAttrSet"`.
    functions: BTreeMap<String, String>,
}

impl TsLibraryBindings {
    /// Faber API names and host symbols for one module stem (`dom` → attr_set…).
    fn module_exports(&self, stem: &str) -> Option<Vec<(String, String)>> {
        let mut exports = Vec::new();
        let needle = format!(":{stem}.");
        for (route, symbol) in &self.functions {
            // route = "{provider}:{module}.{fn}" e.g. web:dom.attr_set
            if let Some(idx) = route.find(&needle) {
                // Ensure module boundary: character before ':' segment is provider end.
                // Accept any provider prefix that ends with `:{stem}.`.
                let after = &route[idx + needle.len()..];
                if !after.is_empty() && !after.contains('.') && !after.contains(':') {
                    exports.push((after.to_owned(), symbol.clone()));
                }
            }
        }
        if exports.is_empty() {
            None
        } else {
            exports.sort_by(|a, b| a.0.cmp(&b.0));
            Some(exports)
        }
    }
}

#[derive(Debug, serde::Deserialize)]
struct TsBindingFile {
    #[serde(default)]
    functions: BTreeMap<String, TsBindingFn>,
    #[serde(default)]
    shim: Option<TsBindingShim>,
}

#[derive(Debug, serde::Deserialize)]
struct TsBindingFn {
    symbol: String,
}

#[derive(Debug, serde::Deserialize)]
struct TsBindingShim {
    path: String,
}

/// Load optional TS binding shim config from a library package's `faber.toml`.
fn load_ts_library_bindings(pkg_root: &Path) -> Result<Option<TsLibraryBindings>, Box<Diagnostic>> {
    let manifest_path = pkg_root.join(super::super::MANIFEST_FILE);
    if !manifest_path.is_file() {
        return Ok(None);
    }
    let manifest = match super::super::manifest::read_manifest(&manifest_path) {
        Ok(m) => m,
        Err(_) => return Ok(None),
    };
    let Some(target) = manifest.target.get("ts") else {
        return Ok(None);
    };
    let Some(bindings_rel) = target.bindings.as_deref() else {
        return Ok(None);
    };
    let binding_path =
        match super::super::resolve_package_member(pkg_root, bindings_rel, &manifest_path) {
            Ok(p) => p,
            Err(err) => return Err(Box::new(err)),
        };
    let source = fs::read_to_string(&binding_path).map_err(|err| io_diag(&binding_path, err))?;
    let file: TsBindingFile = toml::from_str(&source).map_err(|err| {
        Box::new(
            product_diag(format!(
                "invalid TS binding manifest `{}`: {err}",
                binding_path.display()
            ))
            .with_file(binding_path.display().to_string())
            .with_arg("issue", "product_ts_binding_manifest_invalid"),
        )
    })?;
    let Some(shim) = file.shim else {
        return Ok(None);
    };
    if shim.path.trim().is_empty() {
        return Ok(None);
    }
    let shim_path = match super::super::resolve_package_member(pkg_root, &shim.path, &binding_path)
    {
        Ok(p) => p,
        Err(err) => return Err(Box::new(err)),
    };
    if !shim_path.is_file() {
        return Err(Box::new(
            product_diag(format!("TS binding shim missing: {}", shim_path.display()))
                .with_file(binding_path.display().to_string())
                .with_arg("issue", "product_ts_binding_shim_missing"),
        ));
    }
    let functions: BTreeMap<String, String> = file
        .functions
        .into_iter()
        .map(|(k, v)| (k, v.symbol))
        .collect();
    Ok(Some(TsLibraryBindings {
        shim_path,
        functions,
    }))
}

/// Copy the package TS runtime shim into the product tree and write a facade
/// that re-exports host symbols under Faber API names (`attr_set`, …).
fn emit_ts_binding_facade(
    _pkg_root: &Path,
    pkg_name: &str,
    stem: &str,
    file_name: &str,
    bindings: &TsLibraryBindings,
    exports: &[(String, String)],
    ts_root: &Path,
) -> Result<(), Box<Diagnostic>> {
    let shim_stem = bindings
        .shim_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("runtime");
    let runtime_name = format!("{pkg_name}-shim-{shim_stem}.ts");
    let runtime_path = ts_root.join(&runtime_name);
    // Copy once (idempotent if several modules share the same shim).
    if !runtime_path.is_file() {
        fs::copy(&bindings.shim_path, &runtime_path).map_err(|err| io_diag(&runtime_path, err))?;
    }

    let runtime_import = format!("./{pkg_name}-shim-{shim_stem}.js");
    let mut code =
        String::from("// Generated by faber product packaging — TS binding shim facade.\n");
    code.push_str("import {\n");
    for (api_name, symbol) in exports {
        code.push_str(&format!("  {symbol} as {api_name},\n"));
    }
    code.push_str(&format!("}} from {runtime_import:?};\n\n"));
    if stem == "dom" {
        code.push_str("import type {\n");
        for (_, runtime_name) in DOM_TYPE_ALIASES {
            code.push_str(&format!("  {runtime_name},\n"));
        }
        code.push_str(&format!("}} from {runtime_import:?};\n\n"));
    }

    code.push_str("export {\n");
    for (api_name, _) in exports {
        code.push_str(&format!("  {api_name},\n"));
    }
    code.push_str("};\n\n");

    // Namespace object matches wrap_module_exports shape: `import { dom } from …`.
    code.push_str(&format!("export const {stem} = {{\n"));
    for (i, (api_name, _)) in exports.iter().enumerate() {
        let comma = if i + 1 < exports.len() { "," } else { "" };
        code.push_str(&format!("  {api_name}{comma}\n"));
    }
    code.push_str("};\n");
    if stem == "dom" {
        code.push('\n');
        for (api_name, runtime_name) in DOM_TYPE_ALIASES {
            code.push_str(&format!("export type {api_name} = {runtime_name};\n"));
        }
    }

    let facade_path = ts_root.join(file_name);
    fs::write(&facade_path, code).map_err(|err| io_diag(&facade_path, err))?;
    Ok(())
}

/// Recursively enumerate `.fab` files under a library `src/` directory.
///
/// FBR-P2-001: a directory read error anywhere in the tree is a diagnostic
/// failure, never a silently dropped module. Reuses the package discovery
/// walk in [`super::super::source_files::package_source_files`] (BFS, symlink-escape
/// guarded) with `include_proba = false`.
pub(super) fn library_src_fab_files(src_dir: &Path) -> Result<Vec<PathBuf>, Box<Diagnostic>> {
    super::super::source_files::package_source_files(src_dir, false).map_err(|mut diags| {
        Box::new(diags.pop().unwrap_or_else(|| {
            product_diag(format!(
                "failed to read library source root {}",
                src_dir.display()
            ))
            .with_file(src_dir.display().to_string())
        }))
    })
}
