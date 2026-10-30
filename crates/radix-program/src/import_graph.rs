//! Local/library import graph construction for package compilation.
//!
//! `RP2c`: moved from `crates/faber/src/package/import_graph.rs`. Owns local
//! import resolution (template/relative/source-root forms), library-vs-local
//! routing through the explicit [`LibraryResolver`], cycle detection, mounted
//! CLI plan construction, and the import diagnostics (cycle, missing module,
//! proba-import-forbidden, unsupported import kind, library resolve errors).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::discovery::PackageSpec;
use crate::library::{LibraryResolveError, LibraryResolver, ResolvedLibraryModule};
use crate::{LibraryImportBinding, PackageFile};
use radix_module::contains_path_traversal;
use radix_module::diagnostics::Diagnostic;
use radix_module::lexer::{Interner, Span, TokenKind};
use radix_module::syntax::{AnnotationKind, ImportDecl, ImportKind, StmtKind};

use crate::paths::normalize_path;
use radix_module::diagnostics::DiagnosticConvert;

#[derive(Default)]
pub struct MountPlan {
    pub root_cli: Option<radix_module::cli::CliProgram>,
    pub module_cli: BTreeMap<PathBuf, radix_module::cli::CliProgram>,
}

struct MountSpec {
    prefix: Vec<String>,
    alias: String,
    span: Span,
}

/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn build_mount_plan(
    spec: &PackageSpec,
    files: &[PackageFile],
) -> Result<MountPlan, Vec<Diagnostic>> {
    let Some(entry_file) = files.iter().find(|file| file.path == spec.entry) else {
        return Ok(MountPlan::default());
    };

    let root_analysis = radix_module::cli::analyze(&entry_file.program, &entry_file.interner);
    let mut diagnostics = root_analysis
        .errors
        .iter()
        .map(|err| {
            Diagnostic::from_semantic_error(
                &entry_file.path.display().to_string(),
                &entry_file.source,
                err,
            )
        })
        .collect::<Vec<_>>();
    let Some(mut root_cli) = root_analysis.program else {
        if diagnostics.iter().any(Diagnostic::is_error) {
            return Err(diagnostics);
        }
        return Ok(MountPlan::default());
    };

    let imports = import_aliases(spec, entry_file);
    let mounts = collect_root_mounts(entry_file, &mut diagnostics);
    let files_by_path = files
        .iter()
        .map(|file| (file.path.clone(), file))
        .collect::<BTreeMap<_, _>>();
    let mut module_cli = BTreeMap::<PathBuf, radix_module::cli::CliProgram>::new();
    let mut command_origins = root_cli
        .commands
        .iter()
        .map(|command| (command.clone(), entry_file.path.clone()))
        .collect::<Vec<_>>();

    for mount in mounts {
        let Some(module_file) = resolve_mount_module(
            &mount,
            &imports,
            &files_by_path,
            &entry_file.path,
            &mut diagnostics,
        ) else {
            continue;
        };

        let module_analysis = radix_module::cli::analyze_mounted_module(
            &module_file.program,
            &module_file.interner,
            &mount.prefix,
        );
        diagnostics.extend(module_analysis.errors.iter().map(|err| {
            Diagnostic::from_semantic_error(
                &module_file.path.display().to_string(),
                &module_file.source,
                err,
            )
        }));
        let Some(mut mounted_cli) = module_analysis.program else {
            continue;
        };
        mounted_cli
            .global_options
            .clone_from(&root_cli.global_options);
        mounted_cli
            .global_operands
            .clone_from(&root_cli.global_operands);
        diagnostics.extend(validate_mounted_global_collisions(
            &mounted_cli.commands,
            &root_cli,
            &module_file.path,
        ));

        for command in &mut mounted_cli.commands {
            let mut root_command = command.clone();
            root_command.module_path = Some(module_file.module_segments.clone());
            root_cli.commands.push(root_command.clone());
            command_origins.push((root_command, module_file.path.clone()));
        }
        module_cli.insert(module_file.path.clone(), mounted_cli);
    }

    diagnostics.extend(validate_mounted_command_collisions(&command_origins));
    if !root_cli.commands.is_empty() {
        root_cli.mode = radix_module::cli::CliMode::Subcommand;
    }
    if diagnostics.iter().any(Diagnostic::is_error) {
        Err(diagnostics)
    } else {
        Ok(MountPlan {
            root_cli: Some(root_cli),
            module_cli,
        })
    }
}

fn resolve_mount_module<'a>(
    mount: &MountSpec,
    imports: &ImportAliases,
    files_by_path: &'a BTreeMap<PathBuf, &'a PackageFile>,
    entry_path: &Path,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<&'a PackageFile> {
    if imports.named_aliases.contains(&mount.alias) {
        diagnostics.push(
            crate::package_diagnostic_error(format!(
                "@ imperia target '{}' must be a wildcard import alias, not a named import",
                mount.alias
            ))
            .with_file(entry_path.display().to_string())
            .with_arg("issue", "mount_requires_wildcard_alias")
            .with_arg("alias", mount.alias.clone())
            .with_span(mount.span),
        );
        return None;
    }

    let Some(module_path) = imports.wildcard_aliases.get(&mount.alias) else {
        diagnostics.push(
            crate::package_diagnostic_error(format!(
                "@ imperia target '{}' does not name a package-local wildcard import alias",
                mount.alias
            ))
            .with_file(entry_path.display().to_string())
            .with_arg("issue", "mount_unknown_wildcard_alias")
            .with_arg("alias", mount.alias.clone())
            .with_span(mount.span),
        );
        return None;
    };
    let Some(module_file) = files_by_path.get(module_path) else {
        diagnostics.push(
            crate::package_diagnostic_error(format!(
                "@ imperia target '{}' resolved to a module that was not loaded",
                mount.alias
            ))
            .with_file(entry_path.display().to_string())
            .with_span(mount.span),
        );
        return None;
    };
    Some(*module_file)
}

#[derive(Default)]
struct ImportAliases {
    wildcard_aliases: BTreeMap<String, PathBuf>,
    named_aliases: BTreeSet<String>,
}

fn import_aliases(spec: &PackageSpec, file: &PackageFile) -> ImportAliases {
    let mut aliases = ImportAliases::default();
    for stmt in &file.program.statements {
        let StmtKind::Import(decl) = &stmt.kind else {
            continue;
        };
        if decl.target.is_some() {
            continue;
        }
        let import_path = file.interner.resolve(decl.path);
        let Ok(Some(target)) = resolve_local_import(spec, &file.path, import_path) else {
            continue;
        };
        match &decl.kind {
            ImportKind::Wildcard { alias } => {
                aliases.wildcard_aliases.insert(
                    file.interner.resolve(alias.name).to_owned(),
                    normalize_path(&target),
                );
            }
            ImportKind::Named { name, alias, .. } => {
                let visible = alias.as_ref().unwrap_or(name);
                aliases
                    .named_aliases
                    .insert(file.interner.resolve(visible.name).to_owned());
            }
            // EOT-2M: selective members are caller-owned value locals, not
            // namespace aliases; they introduce no wildcard/named surface.
            ImportKind::Selective { .. } => {}
        }
    }
    aliases
}

fn collect_root_mounts(file: &PackageFile, diagnostics: &mut Vec<Diagnostic>) -> Vec<MountSpec> {
    let mut mounts = Vec::new();
    for stmt in &file.program.statements {
        let is_cli_entry = stmt
            .annotations
            .iter()
            .any(|annotation| matches!(annotation.kind, AnnotationKind::Cli(_)));
        for annotation in &stmt.annotations {
            let AnnotationKind::Statement(annotation_stmt) = &annotation.kind else {
                continue;
            };
            if file.interner.resolve(annotation_stmt.name.name) != "imperia" {
                continue;
            }
            if !is_cli_entry {
                diagnostics.push(
                    crate::package_diagnostic_error(
                        "@ imperia module mounts must annotate the root @ cli entry point",
                    )
                    .with_file(file.path.display().to_string())
                    .with_span(annotation.span),
                );
                continue;
            }
            match parse_mount_annotation(file, annotation_stmt, annotation.span) {
                Some(mount) => mounts.push(mount),
                None => diagnostics.push(
                    crate::package_diagnostic_error(
                        "@ imperia must use '@ imperia \"path\" ex <wildcard_alias>'",
                    )
                    .with_file(file.path.display().to_string())
                    .with_span(annotation.span),
                ),
            }
        }
    }
    mounts
}

fn parse_mount_annotation(
    file: &PackageFile,
    annotation: &radix_module::syntax::AnnotationStmt,
    span: Span,
) -> Option<MountSpec> {
    if annotation.args.len() != 3 {
        return None;
    }
    let TokenKind::String(path) = annotation.args[0].kind else {
        return None;
    };
    match annotation.args[1].kind {
        TokenKind::Ex => {}
        TokenKind::Ident(sym) if file.interner.resolve(sym) == "ex" => {}
        _ => return None,
    }
    let TokenKind::Ident(alias) = annotation.args[2].kind else {
        return None;
    };
    let raw_path = file.interner.resolve(path);
    let prefix = raw_path
        .split('/')
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    // Policy: mounted command prefixes are logical CLI paths, not filesystem
    // paths, so absolute paths and empty segments are rejected at annotation
    // parse time.
    if prefix.is_empty()
        || raw_path.starts_with('/')
        || raw_path.ends_with('/')
        || raw_path.contains("//")
    {
        return None;
    }
    Some(MountSpec {
        prefix,
        alias: file.interner.resolve(alias).to_owned(),
        span,
    })
}

fn validate_mounted_command_collisions(
    commands: &[(radix_module::cli::CliCommand, PathBuf)],
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let mut paths = BTreeMap::<String, Span>::new();
    let mut aliases = BTreeMap::<String, Span>::new();

    for (command, file) in commands {
        let path = command.path.join("/");
        if paths.insert(path.clone(), command.span).is_some() {
            diagnostics.push(
                crate::package_diagnostic_error(format!("duplicate command path '{path}'"))
                    .with_file(file.display().to_string())
                    .with_arg("issue", "duplicate_command_path")
                    .with_arg("path", path)
                    .with_span(command.span),
            );
        }
    }

    for (command, file) in commands {
        for alias in &command.aliases {
            if aliases.insert(alias.clone(), command.span).is_some() {
                diagnostics.push(
                    crate::package_diagnostic_error(format!("duplicate command alias '{alias}'"))
                        .with_file(file.display().to_string())
                        .with_arg("issue", "duplicate_command_alias")
                        .with_arg("alias", alias.clone())
                        .with_span(command.span),
                );
            }
            if paths.contains_key(alias) {
                diagnostics.push(
                    crate::package_diagnostic_error(format!(
                        "command alias '{alias}' collides with a command path"
                    ))
                    .with_file(file.display().to_string())
                    .with_span(command.span),
                );
            }
        }
    }

    diagnostics
}

fn validate_mounted_global_collisions(
    commands: &[radix_module::cli::CliCommand],
    root_cli: &radix_module::cli::CliProgram,
    file: &Path,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let mut globals = BTreeSet::<&str>::new();
    for option in &root_cli.global_options {
        globals.insert(option.binding.as_str());
    }
    for operand in &root_cli.global_operands {
        globals.insert(operand.binding.as_str());
    }

    for command in commands {
        let label = command.path.join("/");
        for option in &command.options {
            if globals.contains(option.binding.as_str()) {
                diagnostics.push(
                    crate::package_diagnostic_error(format!(
                        "command '{label}' option '{}' collides with a global CLI binding",
                        option.binding
                    ))
                    .with_file(file.display().to_string())
                    .with_arg("issue", "local_option_global_collision")
                    .with_arg("label", label.clone())
                    .with_arg("binding", option.binding.clone())
                    .with_span(option.span),
                );
            }
        }
        for operand in &command.operands {
            if globals.contains(operand.binding.as_str()) {
                diagnostics.push(
                    crate::package_diagnostic_error(format!(
                        "command '{label}' operand '{}' collides with a global CLI binding",
                        operand.binding
                    ))
                    .with_file(file.display().to_string())
                    .with_arg("issue", "local_operand_global_collision")
                    .with_arg("label", label.clone())
                    .with_arg("binding", operand.binding.clone())
                    .with_span(operand.span),
                );
            }
        }
    }

    diagnostics
}

#[must_use]
pub fn detect_import_cycles(spec: &PackageSpec, files: &[PackageFile]) -> Vec<Diagnostic> {
    let mut graph = BTreeMap::<PathBuf, Vec<(PathBuf, Span)>>::new();
    for file in files {
        let mut edges = Vec::new();
        for stmt in &file.program.statements {
            let StmtKind::Import(decl) = &stmt.kind else {
                continue;
            };
            if decl.target.is_some() {
                continue;
            }
            let import_path = file.interner.resolve(decl.path);
            if let Ok(Some(target)) = resolve_local_import(spec, &file.path, import_path) {
                edges.push((normalize_path(&target), decl.span));
            }
        }
        graph.insert(file.path.clone(), edges);
    }

    let mut diagnostics = Vec::new();
    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    let mut stack = Vec::<PathBuf>::new();
    for file in files {
        detect_import_cycles_from(
            &file.path,
            &graph,
            &mut visiting,
            &mut visited,
            &mut stack,
            &mut diagnostics,
        );
    }
    diagnostics
}

#[derive(Debug)]
pub enum ImportResolution {
    Local(PathBuf),
    Library(ResolvedLibraryModule),
    Unsupported,
    Error(Diagnostic),
}

#[must_use]
pub fn resolve_import(
    spec: &PackageSpec,
    library_resolver: &LibraryResolver,
    from_file: &Path,
    import_path: &str,
) -> ImportResolution {
    if radix_module::kernel::is_kernel_import_path(import_path) {
        return ImportResolution::Error(kernel_import_script_only_diagnostic(
            from_file,
            import_path,
        ));
    }

    // Explicit `.proba` in the specifier is never a legal import target.
    if crate::source_files::is_proba_source_path(Path::new(import_path)) {
        return ImportResolution::Error(proba_import_forbidden_diagnostic(from_file, import_path));
    }

    match library_resolver.resolve(import_path) {
        Ok(Some(module)) => {
            let interface = normalize_path(&module.interface_path);
            if crate::source_files::is_proba_source_path(&interface) {
                return ImportResolution::Error(proba_import_forbidden_diagnostic(
                    from_file,
                    import_path,
                ));
            }
            // A `provider:module` specifier that names a file in this package
            // is a self-import. Route it as Local so package analysis walks
            // the in-tree DAG once instead of re-entering
            // `analyze_cached_library_interface` for every sibling.
            if is_self_package_interface(spec, &interface) {
                return ImportResolution::Local(interface);
            }
            return ImportResolution::Library(module);
        }
        Ok(None) => {}
        Err(err) => return ImportResolution::Error(library_resolve_diagnostic(from_file, err)),
    }

    match resolve_local_import(spec, from_file, import_path) {
        Ok(Some(target)) => {
            if crate::source_files::is_proba_source_path(&target) {
                return ImportResolution::Error(proba_import_forbidden_diagnostic(
                    from_file,
                    import_path,
                ));
            }
            ImportResolution::Local(target)
        }
        Ok(None) => ImportResolution::Unsupported,
        Err(diagnostic) => ImportResolution::Error(diagnostic),
    }
}

fn is_self_package_interface(spec: &PackageSpec, interface: &Path) -> bool {
    let source_root = normalize_path(&spec.source_root);
    let interface = normalize_path(interface);
    if source_root.as_os_str().is_empty() || !interface.starts_with(&source_root) {
        return false;
    }
    // Prefix match is not package identity: a stdlib/library checkout nested
    // under the consumer directory has its own `faber.toml` and must stay a
    // library (Latin identity fallback), not a consumer-locale local unit.
    match radix_module::program::find_package_root(&interface) {
        Some(root) => normalize_path(&root) == normalize_path(&spec.package_root),
        None => true,
    }
}

fn proba_import_forbidden_diagnostic(from_file: &Path, import_path: &str) -> Diagnostic {
    crate::package_diagnostic_error(
        ".proba files are test sources and cannot be imported; move shared helpers to a .fab module",
    )
    .with_file(from_file.display().to_string())
    .with_arg("issue", "proba_import_forbidden")
    .with_arg("import", import_path.to_owned())
    .with_arg("remediation", "move_shared_helpers_to_fab_module")
}

fn detect_import_cycles_from(
    path: &PathBuf,
    graph: &BTreeMap<PathBuf, Vec<(PathBuf, Span)>>,
    visiting: &mut BTreeSet<PathBuf>,
    visited: &mut BTreeSet<PathBuf>,
    stack: &mut Vec<PathBuf>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if visited.contains(path) {
        return;
    }
    if !visiting.insert(path.clone()) {
        return;
    }
    stack.push(path.clone());

    for (next, span) in graph.get(path).into_iter().flatten() {
        if visiting.contains(next) {
            let cycle_start = stack.iter().position(|item| item == next).unwrap_or(0);
            let mut cycle = stack[cycle_start..]
                .iter()
                .map(|item| item.display().to_string())
                .collect::<Vec<_>>();
            cycle.push(next.display().to_string());
            diagnostics.push(
                crate::package_diagnostic_error(format!(
                    "import cycle detected: {}",
                    cycle.join(" -> ")
                ))
                .with_file(path.display().to_string())
                .with_arg("issue", "package_import_cycle")
                .with_span(*span),
            );
            continue;
        }
        if graph.contains_key(next) {
            detect_import_cycles_from(next, graph, visiting, visited, stack, diagnostics);
        }
    }

    stack.pop();
    visiting.remove(path);
    visited.insert(path.clone());
}

fn resolve_local_import(
    spec: &PackageSpec,
    from_file: &Path,
    import_path: &str,
) -> Result<Option<PathBuf>, Diagnostic> {
    // Template form: §name/rest → templates[name]/rest
    if import_path.starts_with('§') {
        return Ok(resolve_template_import(spec, import_path));
    }

    if import_path.starts_with('.') {
        let Some(parent) = from_file.parent() else {
            return Ok(None);
        };
        let Some(target) = resolve_module_candidates(&parent.join(import_path)) else {
            return Ok(None);
        };
        return contain_local_import(spec, from_file, import_path, target).map(Some);
    }

    if import_path.starts_with('@') || import_path.contains("://") {
        return Ok(None);
    }

    let Some(target) = resolve_module_candidates(&spec.source_root.join(import_path)) else {
        return Ok(None);
    };
    contain_local_import(spec, from_file, import_path, target).map(Some)
}

/// Contain a resolved relative/source-root import under the package's declared
/// surface.
///
/// Containment is decided on the canonicalized target, never on the spelling:
/// a legal in-package `../` import keeps resolving and a module reached through
/// a `§template` base keeps resolving its own siblings, while a `..` escape or
/// a symlink pointing outside every declared root is rejected before the
/// target is read.
fn contain_local_import(
    spec: &PackageSpec,
    from_file: &Path,
    import_path: &str,
    target: PathBuf,
) -> Result<PathBuf, Diagnostic> {
    let resolved = std::fs::canonicalize(&target).map_err(|error| {
        import_escape_diagnostic(
            from_file,
            import_path,
            &format!(
                "import target {} cannot be resolved: {error}",
                target.display()
            ),
        )
    })?;
    if declared_import_roots(spec)
        .iter()
        .any(|root| resolved.starts_with(root))
    {
        return Ok(target);
    }
    Err(import_escape_diagnostic(
        from_file,
        import_path,
        &format!("resolves outside the package root: {}", resolved.display()),
    ))
}

/// Canonical roots an import may resolve into: the package root plus every
/// manifest-declared `§template` base.
///
/// Template bases escape the package root by design (`gnu =
/// "../../common/gnu"`), so a module reached through one imports its siblings
/// relative to that base. The manifest is the package's own declared surface;
/// source-level specifiers stay contained to it.
fn declared_import_roots(spec: &PackageSpec) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(root) = std::fs::canonicalize(&spec.package_root) {
        roots.push(root);
    }
    for template in spec.templates.values() {
        if let Ok(base) = std::fs::canonicalize(template) {
            roots.push(base);
        }
    }
    roots
}

fn import_escape_diagnostic(from_file: &Path, import_path: &str, reason: &str) -> Diagnostic {
    crate::package_diagnostic_error(format!("import '{import_path}' {reason}"))
        .with_file(from_file.display().to_string())
        .with_arg("issue", "package_import_escape")
        .with_arg("import", import_path.to_owned())
}

/// Resolve `§name/rest` against `[paths.templates]`.
fn resolve_template_import(spec: &PackageSpec, import_path: &str) -> Option<PathBuf> {
    // `§` is two UTF-8 bytes.
    let after = import_path.get(2..)?;
    if after.is_empty() || after.starts_with('/') {
        return None;
    }
    let (name, rest) = match after.find('/') {
        Some(idx) => (&after[..idx], &after[idx + 1..]),
        None => (after, ""),
    };
    if name.is_empty() {
        return None;
    }
    // The template base escapes the package root by design; only the rest
    // segment is traversal-guarded (F-046 parity with the CLI resolver).
    if contains_path_traversal(rest) {
        return None;
    }
    let base = spec.templates.get(name)?;
    if rest.is_empty() {
        resolve_module_candidates(base)
    } else {
        resolve_module_candidates(&base.join(rest))
    }
}

fn resolve_module_candidates(base: &Path) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if base.extension().is_some() {
        candidates.push(base.to_path_buf());
    } else {
        candidates.push(base.with_extension("fab"));
        candidates.push(base.join("main.fab"));
        candidates.push(base.join("mod.fab"));
    }
    candidates
        .into_iter()
        .find(|candidate| candidate.exists())
        .map(|candidate| normalize_path(&candidate))
}

#[must_use]
pub fn library_import_binding(
    interner: &Interner,
    decl: &ImportDecl,
    module: ResolvedLibraryModule,
) -> Option<LibraryImportBinding> {
    let module_name = module.module_name()?;
    match &decl.kind {
        ImportKind::Named { name, alias, .. } => {
            if alias.is_some() && interner.resolve(name.name) != module_name {
                return None;
            }
            let binding = alias.as_ref().unwrap_or(name);
            Some(LibraryImportBinding {
                binding: interner.resolve(binding.name).to_owned(),
                visibility: decl.visibility,
                import_span: decl.span,
                module,
            })
        }
        ImportKind::Wildcard { alias } => Some(LibraryImportBinding {
            // Wildcard imports bind the module's full export surface under the
            // alias (driver/emit parity: `import_binding` + HIR lowering both
            // name the alias, not the module).
            binding: interner.resolve(alias.name).to_owned(),
            visibility: decl.visibility,
            import_span: decl.span,
            module,
        }),
        // EOT-2M: a selective import binds no namespace; key the module by its
        // import path so the driver installs the file interface where the
        // resolve pass looks members up (see `import_binding` in analyze.rs).
        ImportKind::Selective { .. } => Some(LibraryImportBinding {
            binding: interner.resolve(decl.path).to_owned(),
            visibility: decl.visibility,
            import_span: decl.span,
            module,
        }),
    }
}

#[must_use]
pub fn library_resolve_diagnostic(file: &Path, err: LibraryResolveError) -> Diagnostic {
    match err {
        LibraryResolveError::OldBuiltinNormaSpecifier {
            specifier: _,
            replacement,
        } => crate::package_diagnostic_error(format!(
            "built-in Norma imports use provider syntax; write \"{replacement}\""
        ))
        .with_file(file.display().to_string())
        .with_arg("issue", "old_builtin_norma_specifier")
        .with_arg("replacement", replacement),
        LibraryResolveError::ProviderSpecifierInvalid { specifier, reason } => crate::package_diagnostic_error(format!(
            "invalid library import specifier `{specifier}`: {reason}"
        ))
        .with_file(file.display().to_string())
        .with_arg("issue", "invalid_library_specifier")
        .with_arg("specifier", specifier)
        .with_arg("reason", reason),
        LibraryResolveError::ProviderUnknown {
            specifier,
            provider,
            library_home,
        } => crate::package_diagnostic_error(format!(
            "unknown library provider `{provider}` in import `{specifier}`; expected provider repo at {}",
            library_home.join(&provider).display()
        ))
        .with_file(file.display().to_string())
        .with_arg("issue", "unknown_library_provider")
        .with_arg("specifier", specifier)
        .with_arg("provider", provider)
        .with_arg("library_home", library_home.display().to_string()),
        LibraryResolveError::InstalledManifestInvalid {
            specifier,
            provider,
            manifest_path,
            reason,
            cause_issue,
        } => installed_manifest_invalid_diagnostic(
            file,
            &specifier,
            &provider,
            &manifest_path,
            &reason,
            cause_issue,
        ),
        LibraryResolveError::InstalledSourceRootMissing {
            specifier,
            provider,
            source_root,
        } => crate::package_diagnostic_error(format!(
            "installed library provider `{provider}` for import `{specifier}` is missing source root {}",
            source_root.display()
        ))
        .with_file(file.display().to_string())
        .with_arg("issue", "missing_installed_library_source_root")
        .with_arg("specifier", specifier)
        .with_arg("provider", provider)
        .with_arg("source_root", source_root.display().to_string()),
        LibraryResolveError::ModuleUnknown {
            specifier,
            package,
            expected_path,
            known_modules,
        } => unknown_module_diagnostic(
            file,
            &specifier,
            &package,
            &expected_path,
            &known_modules,
        ),
        // Uncoded on purpose: the standing renderer contract prints prose
        // for coded non-Codegen diagnostics only through locale-pack
        // templates, and no pack carries a PKG001 template — coding this arm
        // renders an empty `error[PKG001:missing_library_home]: <file>`
        // body (same contract as `file_interface::interface_error`). The
        // issue/specifier args keep the failure machine-identifiable, and
        // the hint stays in the message: a missing home blocks the build,
        // so the actionable sentence must not hide behind a flag.
        LibraryResolveError::LibraryHomeMissing { specifier, hint } => {
            Diagnostic::error(format!(
                "library import `{specifier}` needs a public library home; {hint}"
            ))
            .with_phase(radix_module::diagnostics::DiagnosticPhase::Analysis)
            .with_file(file.display().to_string())
            .with_arg("issue", "missing_library_home")
            .with_arg("specifier", specifier)
        }
        LibraryResolveError::LockedPackageMissing {
            specifier,
            provider,
            version,
        } => locked_package_missing_diagnostic(file, &specifier, &provider, version.as_deref()),
        LibraryResolveError::ProviderUndeclared {
            specifier,
            provider,
        } => crate::package_diagnostic_error(format!(
            "library import `{specifier}` uses undeclared provider `{provider}`; add it to faber.toml [dependencies] and install it"
        ))
        .with_file(file.display().to_string())
        .with_arg("issue", "undeclared_library_provider")
        .with_arg("specifier", specifier)
        .with_arg("provider", provider),
    }
}

fn unknown_module_diagnostic(
    file: &Path,
    specifier: &str,
    package: &str,
    expected_path: &Path,
    known_modules: &[String],
) -> Diagnostic {
    crate::package_diagnostic_error(format!(
        "unknown library module `{specifier}` for provider `{package}`; expected {}; known modules: {}",
        expected_path.display(),
        known_modules.join(", ")
    ))
    .with_file(file.display().to_string())
    .with_arg("issue", "unknown_library_module")
    .with_arg("specifier", specifier.to_owned())
    .with_arg("provider", package.to_owned())
    .with_arg("expected_path", expected_path.display().to_string())
    .with_arg("known_modules", known_modules.join(","))
}

fn locked_package_missing_diagnostic(
    file: &Path,
    specifier: &str,
    provider: &str,
    version: Option<&str>,
) -> Diagnostic {
    let version_hint = version
        .map(|v| format!(" (`{provider} = \"{v}\"`)"))
        .unwrap_or_default();
    crate::package_diagnostic_error(format!(
        "library import `{specifier}` is declared in faber.toml{version_hint} but missing from faber.lock; install the package with the package manager"
    ))
    .with_file(file.display().to_string())
    .with_arg("issue", "missing_locked_package")
    .with_arg("specifier", specifier.to_owned())
    .with_arg("provider", provider.to_owned())
}

fn installed_manifest_invalid_diagnostic(
    file: &Path,
    specifier: &str,
    provider: &str,
    manifest_path: &Path,
    reason: &str,
    cause_issue: Option<Box<str>>,
) -> Diagnostic {
    let diagnostic = crate::package_diagnostic_error(format!(
        "installed library provider `{provider}` for import `{specifier}` has an invalid manifest at {}: {reason}",
        manifest_path.display()
    ))
    .with_file(file.display().to_string())
    .with_arg("issue", "invalid_installed_library_manifest")
    .with_arg("specifier", specifier.to_owned())
    .with_arg("provider", provider.to_owned())
    .with_arg("manifest_path", manifest_path.display().to_string());
    match cause_issue {
        Some(cause) => diagnostic.with_arg("cause_issue", cause.into_string()),
        None => diagnostic,
    }
}

#[must_use]
pub fn inner_library_import_unresolved_diagnostic(
    file: &Path,
    decl: &ImportDecl,
    import_path: &str,
) -> Diagnostic {
    crate::package_diagnostic_error(format!(
        "library interface import `{import_path}` must resolve to a built-in library module (provider syntax `norma:…`)"
    ))
    .with_file(file.display().to_string())
    .with_span(decl.span)
}

#[must_use]
pub fn library_import_kind_diagnostic(
    file: &Path,
    decl: &ImportDecl,
    import_path: &str,
) -> Diagnostic {
    crate::package_diagnostic_error(format!(
        "library import `{import_path}` must import its module name as a module alias"
    ))
    .with_file(file.display().to_string())
    .with_span(decl.span)
}

fn kernel_import_script_only_diagnostic(file: &Path, import_path: &str) -> Diagnostic {
    crate::package_diagnostic_error(format!(
        "{} (`{import_path}`)",
        radix_module::kernel::kernel_script_mode_only_message()
    ))
    .with_file(file.display().to_string())
    .with_arg("issue", "kernel_import_script_mode_only")
    .with_arg("path", import_path.to_owned())
}

#[must_use]
pub fn import_unsupported_diagnostic(
    file: &Path,
    decl: &ImportDecl,
    import_path: &str,
) -> Diagnostic {
    let kind = match &decl.kind {
        ImportKind::Named { .. } => "import",
        ImportKind::Wildcard { .. } => "wildcard import",
        ImportKind::Selective { .. } => "selective import",
    };
    crate::package_diagnostic_error(format!(
        "package compilation only supports local intra-package imports; unsupported {kind} path `{import_path}`"
    ))
    .with_file(file.display().to_string())
    .with_span(decl.span)
    .with_arg("issue", "package_import_unsupported_path")
    .with_arg("kind", kind)
    .with_arg("path", import_path)
}

#[cfg(test)]
#[path = "import_graph_test.rs"]
mod tests;

#[cfg(test)]
#[path = "library_home_missing_render_test.rs"]
mod library_home_missing_render_test;
