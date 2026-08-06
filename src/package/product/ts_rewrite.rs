use super::build::BrowserController;
use super::ts_emit::library_src_fab_files;
use super::{product_diag, BTreeMap, BTreeSet, Diagnostic, Path};

/// TypeScript product naming derived from one library `.fab` source file.
pub(super) struct TsLibModuleNaming {
    /// Library import specifier, e.g. `triga:lighting/light`.
    pub(super) spec: String,
    /// Emitted file name (relative to the TS output root), e.g.
    /// `triga-lighting-light.ts`.
    pub(super) file_name: String,
    /// Relative ESM path used for import rewrites, e.g.
    /// `./triga-lighting-light.js`.
    pub(super) rel_path: String,
    /// Leaf module stem — the namespace-export name importers bind, e.g.
    /// `light` for `src/lighting/light.fab`.
    pub(super) leaf_stem: String,
}

/// Derive TypeScript product naming for a library `.fab` source file.
///
/// Top-level modules keep their historical flat names (`src/math.fab` →
/// `triga:math` / `triga-math.ts` / `./triga-math.js`). Nested leaves always
/// use the full relative path so a nested leaf can never collide with a
/// top-level module that shares its stem (`src/lighting/light.fab` →
/// `triga:lighting/light` / `triga-lighting-light.ts` / `./triga-lighting-light.js`).
/// The `-` delimiter is shared between the emitted `.ts` file names and the
/// `.js` map values so rewritten specifiers resolve against emitted files.
pub(super) fn ts_lib_module_naming(
    pkg_name: &str,
    src_dir: &Path,
    fab_path: &Path,
) -> Option<TsLibModuleNaming> {
    let rel = fab_path.strip_prefix(src_dir).ok()?;
    let segments: Vec<String> = rel
        .with_extension("")
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(segment) => Some(segment.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    if segments.is_empty() {
        return None;
    }
    let rel_str = segments.join("/");
    let dash_name = segments.join("-");
    Some(TsLibModuleNaming {
        spec: format!("{pkg_name}:{rel_str}"),
        file_name: format!("{pkg_name}-{dash_name}.ts"),
        rel_path: format!("./{pkg_name}-{dash_name}.js"),
        leaf_stem: segments.last().cloned().unwrap_or_default(),
    })
}

/// Build a map from library import specifiers to relative ESM paths for all
/// `kind=lib`, `target_language=ts` packages discovered via the lockfile.
///
/// Returns `{specifier} → {relative_path}` mappings, e.g.:
///   "triga:triga" → "./triga-triga.js"
///   "triga:geometry" → "./triga-geometry.js"
///   "triga:lighting/light" → "./triga-lighting-light.js"
///
/// Nested leaves are enumerated recursively and keyed by their full relative
/// module path; top-level modules keep their historical flat names.
///
/// Phase 4: used by [`rewrite_import_specifiers`] to rewrite bare library
/// specifiers in emitted TypeScript to relative ESM paths.
pub(crate) fn build_library_ts_module_map(
    package_root: &Path,
) -> Result<BTreeMap<String, String>, Box<Diagnostic>> {
    let lock = match super::super::lockfile::read_lock(package_root)? {
        Some(lock) => lock,
        None => return Ok(BTreeMap::new()),
    };
    let lock_path = package_root.join(super::super::lockfile::LOCK_FILE);
    let index = super::super::lockfile::lock_index(&lock_path, &lock).map_err(|mut diags| {
        diags.pop().unwrap_or_else(|| {
            product_diag("failed to index faber.lock for library import map")
                .with_file(package_root.display().to_string())
                .with_arg("issue", "product_library_import_map_failed")
        })
    })?;

    let mut map = BTreeMap::new();
    for pkg in index.values() {
        if pkg.kind != "lib" || pkg.target_language != "ts" {
            continue;
        }
        let pkg_root = pkg.package_root_path(package_root);
        let src_dir = pkg_root.join("src");
        if !src_dir.is_dir() {
            continue;
        }
        // FBR-P2-001: per-entry directory read errors are diagnostic failures,
        // never silently dropped modules.
        let src_files = library_src_fab_files(&src_dir)?;
        let mut emitted_file_names: BTreeMap<String, String> = BTreeMap::new();

        for fab_path in &src_files {
            let Some(naming) = ts_lib_module_naming(&pkg.name, &src_dir, fab_path) else {
                continue;
            };
            // Fail closed on emitted-file name collisions (FBR-P2-001): two
            // modules mapping to one output file would silently drop a module
            // and corrupt the import map.
            if let Some(existing_spec) = emitted_file_names.get(&naming.file_name) {
                return Err(Box::new(
                    product_diag(format!(
                        "library TS modules `{existing_spec}` and `{}` both map to `{}`",
                        naming.spec, naming.file_name
                    ))
                    .with_file(fab_path.display().to_string())
                    .with_arg("issue", "product_library_ts_module_name_collision"),
                ));
            }
            emitted_file_names.insert(naming.file_name.clone(), naming.spec.clone());
            map.insert(naming.spec, naming.rel_path);
        }
    }
    Ok(map)
}

/// One-pass rewrite of import/export specifier tokens in emitted TypeScript.
///
/// Merges the former two-pass `rewrite_library_imports` +
/// `rewrite_relative_import_extensions` into a single byte scan
/// (FBR-P2-008):
///
/// - Library specifiers (`from "name:stem"`) are replaced through exact map
///   membership (`library_imports` from [`build_library_ts_module_map`]) — one
///   pass, no per-specifier whole-file `String::replace` loop.
/// - Relative specifiers (`./` / `../`) get a `.js` extension appended when
///   they do not already carry a known extension.
/// - Comments, string literals, and template literals are skipped, so
///   specifier-looking text inside them is never rewritten.
/// - Only `from "<spec>"` / `from '<spec>'` tokens are rewritten, and only
///   when `from` is a standalone token (exact-token contract). Dynamic
///   `import("...")` calls are never touched; `radix-hir-ts` emits only
///   static `import { ... } from "..."` declarations, pinned by test.
pub(crate) fn rewrite_import_specifiers(
    code: String,
    library_imports: &BTreeMap<String, String>,
) -> String {
    let mut out = String::with_capacity(code.len() + 32);
    let bytes = code.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let rest = &bytes[i..];
        // Line comments are copied verbatim; their text is never a specifier.
        if rest.starts_with(b"//") {
            while i < bytes.len() && bytes[i] != b'\n' {
                out.push(bytes[i] as char);
                i += 1;
            }
            continue;
        }
        // Block comments are copied verbatim.
        if rest.starts_with(b"/*") {
            out.push_str("/*");
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                out.push(bytes[i] as char);
                i += 1;
            }
            if i + 1 < bytes.len() {
                out.push_str("*/");
                i += 2;
            }
            continue;
        }
        // Import/export specifier token: `from "` / `from '`. The `from` must
        // be a standalone token (not part of a longer identifier) and the
        // quote pair encloses the whole specifier string.
        if rest.starts_with(b"from ")
            && i + 6 <= bytes.len()
            && (bytes[i + 5] == b'"' || bytes[i + 5] == b'\'')
            && (i == 0 || !is_ts_ident_byte(bytes[i - 1]))
        {
            let quote = bytes[i + 5];
            out.push_str("from ");
            out.push(quote as char);
            i += 6;
            let start = i;
            while i < bytes.len() && bytes[i] != quote {
                i += 1;
            }
            let spec = &code[start..i];
            match library_imports.get(spec) {
                Some(rel_path) => out.push_str(rel_path),
                None => {
                    out.push_str(spec);
                    if spec.starts_with("./") || spec.starts_with("../") {
                        let needs_js = !spec.ends_with(".js")
                            && !spec.ends_with(".json")
                            && !spec.ends_with(".ts")
                            && !spec.ends_with(".mjs")
                            && !spec.ends_with(".cjs");
                        if needs_js {
                            out.push_str(".js");
                        }
                    }
                }
            }
            if i < bytes.len() {
                out.push(quote as char);
                i += 1;
            }
            continue;
        }
        // String literals are copied verbatim (backslash escapes respected).
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            let quote = bytes[i];
            out.push(quote as char);
            i += 1;
            while i < bytes.len() {
                let current = bytes[i];
                out.push(current as char);
                i += 1;
                if current == b'\\' {
                    if i < bytes.len() {
                        out.push(bytes[i] as char);
                        i += 1;
                    }
                } else if current == quote {
                    break;
                }
            }
            continue;
        }
        // Template literals are copied verbatim (backslash escapes respected).
        if bytes[i] == b'`' {
            out.push('`');
            i += 1;
            while i < bytes.len() {
                let current = bytes[i];
                out.push(current as char);
                i += 1;
                if current == b'\\' {
                    if i < bytes.len() {
                        out.push(bytes[i] as char);
                        i += 1;
                    }
                } else if current == b'`' {
                    break;
                }
            }
            continue;
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

/// Rewrite namespace-import bindings to resolve against the emitted target
/// module's namespace export (its leaf stem).
///
/// The radix TS emitter spells a module import's binding name verbatim from
/// the `importa … privata <name>` alias (`import { lighting } from
/// "triga:lighting/light"`), but emitted library modules export their leaf
/// stem as the namespace const (`export const light = { … }`). A binding that
/// does not match the target leaf stem fails `tsc` (TS2305); emit the alias
/// form `import { light as lighting }` instead, matching what codegen already
/// produces for `privata <stem> ut <name>`. Importers that bind the leaf stem
/// (the norm) are untouched.
///
/// Only single-binding module imports are namespace bindings; augmented type
/// names join later, after this pass.
pub(crate) fn normalize_library_namespace_bindings(
    code: String,
    library_imports: &BTreeMap<String, String>,
) -> String {
    let mut out = String::with_capacity(code.len() + 64);
    for line in code.lines() {
        let trimmed = line.trim_start();
        let rewritten = if let Some(rest) = trimmed.strip_prefix("import { ") {
            if let Some((binding, from_part)) = rest.split_once(" } from ") {
                let binding = binding.trim();
                if !binding.is_empty() && !binding.contains(',') && !binding.contains(" as ") {
                    let spec = import_clause_specifier(from_part);
                    let spec = spec.and_then(|spec| library_imports.get_key_value(spec));
                    if let Some((spec, _rel)) = spec {
                        // Leaf module stem: last segment after both the
                        // provider `:` and any nested `/` (`triga:math` →
                        // `math`; `triga:lighting/light` → `light`).
                        let leaf = spec.rsplit('/').next().and_then(|s| s.rsplit(':').next());
                        if let Some(leaf) = leaf {
                            if leaf != binding {
                                let indent = &line[..line.len() - trimmed.len()];
                                Some(format!(
                                    "{indent}import {{ {leaf} as {binding} }} from {from_part}"
                                ))
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };
        match rewritten {
            Some(rewritten) => {
                out.push_str(&rewritten);
                out.push('\n');
            }
            None => {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    out
}

/// Extract the specifier string from an import clause's `from "<spec>";`
/// remainder (which may carry a trailing `;` and any surrounding trivia).
fn import_clause_specifier(from_part: &str) -> Option<&str> {
    let from = from_part.trim();
    let bytes = from.as_bytes();
    if bytes
        .first()
        .is_none_or(|first| *first != b'"' && *first != b'\'')
    {
        return None;
    }
    let quote = char::from(bytes[0]);
    let after = &from[1..];
    let end = after.find(quote)?;
    Some(&after[..end])
}

/// Byte-level TypeScript identifier character check used for the exact
/// `from` token boundary in [`rewrite_import_specifiers`] (FBR-P2-008).
fn is_ts_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$'
}

pub(super) const DOM_TYPE_ALIASES: &[(&str, &str)] = &[
    ("Scope", "WebDomScope"),
    ("Element", "WebDomElement"),
    ("DomEvent", "WebDomEvent"),
    ("FrameState", "WebDomFrameState"),
    ("ResizeState", "WebDomResizeState"),
    ("KeyboardState", "WebDomKeyboardState"),
    ("PointerState", "WebDomPointerState"),
    ("FocusState", "WebDomFocusState"),
    ("PointerLockState", "WebDomPointerLockState"),
    ("Subscription", "WebDomSubscription"),
    ("SubmitOptions", "WebDomSubmitOptions"),
    ("FetchRequest", "WebDomFetchRequest"),
    ("FetchResponse", "WebDomFetchResponse"),
    ("EventHandler", "WebDomEventHandler"),
    ("InputHandler", "WebDomInputHandler"),
    ("SubmitHandler", "WebDomSubmitHandler"),
    ("FrameHandler", "WebDomFrameHandler"),
    ("ResizeHandler", "WebDomResizeHandler"),
    ("KeyboardHandler", "WebDomKeyboardHandler"),
    ("PointerHandler", "WebDomPointerHandler"),
    ("FocusHandler", "WebDomFocusHandler"),
    ("PointerLockHandler", "WebDomPointerLockHandler"),
];

pub(super) fn top_level_ts_decl_names(
    unit: &super::super::AnalyzedPackageUnit,
) -> BTreeSet<String> {
    unit.analysis
        .hir
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            radix::hir::HirItemKind::Function(function) => {
                Some(unit.analysis.interner.resolve(function.name).to_owned())
            }
            radix::hir::HirItemKind::Struct(strukt) => {
                Some(unit.analysis.interner.resolve(strukt.name).to_owned())
            }
            radix::hir::HirItemKind::Enum(enm) => {
                Some(unit.analysis.interner.resolve(enm.name).to_owned())
            }
            radix::hir::HirItemKind::Interface(interface) => {
                Some(unit.analysis.interner.resolve(interface.name).to_owned())
            }
            radix::hir::HirItemKind::TypeAlias(alias) => {
                Some(unit.analysis.interner.resolve(alias.name).to_owned())
            }
            radix::hir::HirItemKind::Constant(konst) => {
                Some(unit.analysis.interner.resolve(konst.name).to_owned())
            }
            radix::hir::HirItemKind::Import(_) => None,
        })
        .collect()
}

pub(super) fn augment_namespace_imports(
    code: String,
    namespace_exports: &BTreeMap<String, Vec<String>>,
    local_names: &BTreeSet<String>,
) -> String {
    let mut output = String::with_capacity(code.len() + 256);
    let mut reserved_names = local_names.clone();
    for imported in all_named_imports(&code) {
        reserved_names.insert(imported);
    }
    for line in code.lines() {
        let trimmed = line.trim_start();
        let indent = &line[..line.len() - trimmed.len()];
        let Some(rest) = trimmed.strip_prefix("import { ") else {
            output.push_str(line);
            output.push('\n');
            continue;
        };
        let Some((imports_part, from_part)) = rest.split_once(" } from ") else {
            output.push_str(line);
            output.push('\n');
            continue;
        };
        let mut imports = imports_part
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let existing = imports.iter().cloned().collect::<BTreeSet<_>>();
        let mut type_imports = Vec::new();
        let mut changed = false;

        for binding in existing.iter() {
            // A nested library import may be normalized to an aliased
            // namespace binding (`object as graph_object`). The exported
            // module name is the left side; the right side is only the local
            // name used by generated references.
            let (module_binding, _local_binding) = binding
                .split_once(" as ")
                .map_or((binding.as_str(), binding.as_str()), |(module, local)| {
                    (module.trim(), local.trim())
                });
            let Some(exports) = namespace_exports.get(module_binding) else {
                continue;
            };
            if module_binding == "dom" {
                for export in exports {
                    if is_dom_type_export(export)
                        && is_ts_identifier(export)
                        && bare_import_needed(&code, export)
                        && !reserved_names.contains(export)
                    {
                        type_imports.push(export.clone());
                        reserved_names.insert(export.clone());
                    }
                }
                continue;
            }
            for export in exports {
                if export == module_binding
                    || !is_ts_identifier(export)
                    || !bare_import_needed(&code, export)
                    || reserved_names.contains(export)
                {
                    continue;
                }
                imports.push(export.clone());
                reserved_names.insert(export.clone());
                changed = true;
            }
        }

        if changed {
            imports.sort();
            output.push_str(indent);
            output.push_str("import { ");
            output.push_str(&imports.join(", "));
            output.push_str(" } from ");
            output.push_str(from_part);
            output.push('\n');
        } else {
            output.push_str(line);
            output.push('\n');
        }

        type_imports.sort();
        type_imports.dedup();
        type_imports.retain(|name| !existing.contains(name));
        if !type_imports.is_empty() {
            output.push_str(indent);
            output.push_str("import type { ");
            output.push_str(&type_imports.join(", "));
            output.push_str(" } from ");
            output.push_str(from_part);
            output.push('\n');
        }
    }
    output
}

fn all_named_imports(code: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in code.lines() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix("import { ") else {
            continue;
        };
        let Some((imports_part, _)) = rest.split_once(" } from ") else {
            continue;
        };
        for item in imports_part
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
        {
            let imported = item
                .split_once(" as ")
                .map_or(item, |(_, alias)| alias)
                .trim();
            if is_ts_identifier(imported) {
                names.push(imported.to_owned());
            }
        }
    }
    names
}

fn bare_import_needed(code: &str, name: &str) -> bool {
    [
        format!(": {name}"),
        format!("!: {name}"),
        format!("<{name}>"),
        format!("| {name}"),
        format!("new {name}("),
        format!("new {name}()"),
        format!("new {name},"),
        format!("as {name}"),
    ]
    .iter()
    .any(|needle| code.contains(needle))
}

fn is_dom_type_export(name: &str) -> bool {
    DOM_TYPE_ALIASES
        .iter()
        .any(|(api_name, _)| *api_name == name)
}

fn is_ts_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_' || first == '$') {
        return false;
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '$')
}

pub(super) fn adapt_controller_typescript(
    mut code: String,
    controllers: &[BrowserController],
) -> String {
    for controller in controllers {
        code = code.replace(
            &format!("function {}(", controller.export),
            &format!("export function {}(", controller.export),
        );
    }
    // Imported nominal types are package-interface facts today, but Radix's TS
    // printer has no portable module-qualified type spelling yet. Faber's
    // product layer already validated the controller signature structurally;
    // keep `tsc` fail-closed for the emitted JavaScript while WEB4 supplies the
    // concrete DOM runtime surface.

    // Struct construction of unresolved types → empty object (value position).
    code = code.replace("new unresolved_def()", "{}");
    code = code.replace("new /* unresolved_def */()", "{}");
    code = rewrite_dom_type_constructors(code);
    // Codegen marker is invalid in type position; replace whole marker first.
    code = code.replace("/* unresolved_def */", "any");
    // Arrow-function closures with explicit `: void` return annotations reject
    // bodies that return a Promise (async handler).  Drop the annotation so
    // TypeScript infers the return type; assignment to a `void`-typed handler
    // parameter still accepts any return value.
    code = code.replace("): void =>", ") =>");
    code = code.replace("unresolved_def", "any");
    code = code.replace("new any()", "{}");
    code
}

fn rewrite_dom_type_constructors(mut code: String) -> String {
    for (api_name, runtime_name) in DOM_TYPE_ALIASES {
        for name in [*api_name, *runtime_name] {
            code = code.replace(&format!("new {name}()"), "{}");
            code = code.replace(&format!("new {name}("), "(");
        }
    }
    code
}

/// Make a generated TypeScript module valid for `tsc` by adding `export` to
/// every top-level function and class declaration, and appending a namespace
/// export object that enables `import { name } from "./file"` to resolve.
///
/// Radix's TS codegen emits module-scoped declarations without `export` and
/// generates `import { moduleName } from "./module"` in consumer files.  This
/// function bridges the gap until codegen adds native module support.  The
/// post-build `link-triga-ts.mjs` script also adds namespace exports via
/// `wrapNamespace` and skips re-adding when `export const <name> = {` exists.
pub(super) fn wrap_module_exports(mut code: String, module_name: &str) -> String {
    // Export every top-level declaration. The namespace export object only
    // carries value members; pure types (`type`, `interface`) cannot be
    // object properties.
    let mut export_names: Vec<String> = Vec::new();

    // Process line by line to find top-level declarations (indent level 0).
    let mut lines: Vec<String> = code.lines().map(|l| l.to_owned()).collect();
    for line in &mut lines {
        if line.starts_with(' ') || line.starts_with('\t') {
            continue;
        }
        let trimmed = line.trim();
        for prefix in [
            "function ",
            "class ",
            "enum ",
            "interface ",
            "type ",
            "const ",
        ] {
            let Some(rest) = trimmed.strip_prefix(prefix) else {
                continue;
            };
            let name = rest
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .next()
                .unwrap_or("")
                .trim();
            if name.is_empty() {
                continue;
            }
            if prefix != "type " && prefix != "interface " {
                export_names.push(name.to_owned());
            }
            *line = format!("export {}", trimmed);
            break;
        }
    }
    code = lines.join("\n");

    // Build the namespace export for this module (if not already present).
    let ns_marker = format!("export const {} = {{", module_name);
    if !export_names.is_empty() && !code.contains(&ns_marker) {
        code.push_str(&format!("\nexport const {} = {{\n", module_name));
        for (i, name) in export_names.iter().enumerate() {
            let comma = if i < export_names.len() - 1 { "," } else { "" };
            code.push_str(&format!("  {}{}\n", name, comma));
        }
        code.push_str("};\n");
    }

    // Ensure the file is treated as a module by TypeScript.
    if !code.contains("export ") {
        code.push_str("\nexport {};\n");
    }

    code
}

pub(super) fn ts_module_file_name(unit: &super::super::AnalyzedPackageUnit) -> String {
    if unit.module_segments.is_empty() {
        return "main.ts".to_owned();
    }
    format!("{}.ts", unit.module_segments.join("_"))
}
