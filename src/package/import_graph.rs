use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::library::{LibraryResolveError, LibraryResolver, ResolvedLibraryModule};
use radix::diagnostics::Diagnostic;
use radix::lexer::{Interner, Span, TokenKind};
use radix::syntax::{AnnotationKind, ImportDecl, ImportKind, StmtKind};

use super::paths::normalize_path;
use super::{LibraryImportBinding, PackageFile, PackageSpec};
use radix::diagnostics::DiagnosticConvert;

#[derive(Default)]
pub(super) struct MountPlan {
    pub(super) root_cli: Option<radix::cli::CliProgram>,
    pub(super) module_cli: BTreeMap<PathBuf, radix::cli::CliProgram>,
}

struct MountSpec {
    prefix: Vec<String>,
    alias: String,
    span: Span,
}

pub(super) fn build_mount_plan(
    spec: &PackageSpec,
    files: &[PackageFile],
) -> Result<MountPlan, Vec<Diagnostic>> {
    let Some(entry_file) = files.iter().find(|file| file.path == spec.entry) else {
        return Ok(MountPlan::default());
    };

    let root_analysis = radix::cli::analyze(&entry_file.program, &entry_file.interner);
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
    let mut module_cli = BTreeMap::<PathBuf, radix::cli::CliProgram>::new();
    let mut command_origins = root_cli
        .commands
        .iter()
        .map(|command| (command.clone(), entry_file.path.clone()))
        .collect::<Vec<_>>();

    for mount in mounts {
        if imports.named_aliases.contains(&mount.alias) {
            diagnostics.push(
                crate::package_diagnostic_error(format!(
                    "@ imperia target '{}' must be a wildcard import alias, not a named import",
                    mount.alias
                ))
                .with_file(entry_file.path.display().to_string())
                .with_arg("issue", "mount_requires_wildcard_alias")
                .with_arg("alias", mount.alias.clone())
                .with_span(mount.span),
            );
            continue;
        }

        let Some(module_path) = imports.wildcard_aliases.get(&mount.alias) else {
            diagnostics.push(
                crate::package_diagnostic_error(format!(
                    "@ imperia target '{}' does not name a package-local wildcard import alias",
                    mount.alias
                ))
                .with_file(entry_file.path.display().to_string())
                .with_arg("issue", "mount_unknown_wildcard_alias")
                .with_arg("alias", mount.alias.clone())
                .with_span(mount.span),
            );
            continue;
        };
        let Some(module_file) = files_by_path.get(module_path) else {
            diagnostics.push(
                crate::package_diagnostic_error(format!(
                    "@ imperia target '{}' resolved to a module that was not loaded",
                    mount.alias
                ))
                .with_file(entry_file.path.display().to_string())
                .with_span(mount.span),
            );
            continue;
        };

        let module_analysis = radix::cli::analyze_mounted_module(
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
        mounted_cli.global_options = root_cli.global_options.clone();
        mounted_cli.global_operands = root_cli.global_operands.clone();
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
        root_cli.mode = radix::cli::CliMode::Subcommand;
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
        let import_path = file.interner.resolve(decl.path);
        let Some(target) = resolve_local_import(spec, &file.path, import_path) else {
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
    annotation: &radix::syntax::AnnotationStmt,
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
    commands: &[(radix::cli::CliCommand, PathBuf)],
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
    commands: &[radix::cli::CliCommand],
    root_cli: &radix::cli::CliProgram,
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

pub(super) fn detect_import_cycles(spec: &PackageSpec, files: &[PackageFile]) -> Vec<Diagnostic> {
    let by_path = files
        .iter()
        .map(|file| (file.path.clone(), file))
        .collect::<BTreeMap<_, _>>();
    let mut graph = BTreeMap::<PathBuf, Vec<(PathBuf, Span)>>::new();
    for file in files {
        let mut edges = Vec::new();
        for stmt in &file.program.statements {
            let StmtKind::Import(decl) = &stmt.kind else {
                continue;
            };
            let import_path = file.interner.resolve(decl.path);
            if let Some(target) = resolve_local_import(spec, &file.path, import_path) {
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
            &by_path,
            &mut visiting,
            &mut visited,
            &mut stack,
            &mut diagnostics,
        );
    }
    diagnostics
}

pub(super) enum ImportResolution {
    Local(PathBuf),
    Library(ResolvedLibraryModule),
    Unsupported,
    Error(Diagnostic),
}

pub(super) fn resolve_import(
    spec: &PackageSpec,
    library_resolver: &LibraryResolver,
    from_file: &Path,
    import_path: &str,
) -> ImportResolution {
    if radix::kernel::is_kernel_import_path(import_path) {
        return ImportResolution::Error(kernel_import_script_only_diagnostic(
            from_file,
            import_path,
        ));
    }

    match library_resolver.resolve(import_path) {
        Ok(Some(module)) => return ImportResolution::Library(module),
        Ok(None) => {}
        Err(err) => return ImportResolution::Error(library_resolve_diagnostic(from_file, err)),
    }

    if let Some(target) = resolve_local_import(spec, from_file, import_path) {
        return ImportResolution::Local(target);
    }

    ImportResolution::Unsupported
}

fn detect_import_cycles_from(
    path: &PathBuf,
    graph: &BTreeMap<PathBuf, Vec<(PathBuf, Span)>>,
    by_path: &BTreeMap<PathBuf, &PackageFile>,
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
        if by_path.contains_key(next) {
            detect_import_cycles_from(next, graph, by_path, visiting, visited, stack, diagnostics);
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
) -> Option<PathBuf> {
    if import_path.starts_with('.') {
        return resolve_module_candidates(&from_file.parent()?.join(import_path));
    }

    if import_path.starts_with('@') || import_path.contains("://") {
        return None;
    }

    resolve_module_candidates(&spec.source_root.join(import_path))
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

pub(super) fn library_import_binding(
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
        ImportKind::Wildcard { .. } => None,
    }
}

pub(super) fn library_resolve_diagnostic(file: &Path, err: LibraryResolveError) -> Diagnostic {
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
        LibraryResolveError::InvalidProviderSpecifier { specifier, reason } => crate::package_diagnostic_error(format!(
            "invalid library import specifier `{specifier}`: {reason}"
        ))
        .with_file(file.display().to_string()),
        LibraryResolveError::UnknownProvider {
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
        LibraryResolveError::InvalidInstalledManifest {
            specifier,
            provider,
            manifest_path,
            reason,
        } => crate::package_diagnostic_error(format!(
            "installed library provider `{provider}` for import `{specifier}` has an invalid manifest at {}: {reason}",
            manifest_path.display()
        ))
        .with_file(file.display().to_string())
        .with_arg("issue", "invalid_installed_library_manifest")
        .with_arg("specifier", specifier)
        .with_arg("provider", provider)
        .with_arg("manifest_path", manifest_path.display().to_string()),
        LibraryResolveError::MissingInstalledSourceRoot {
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
        LibraryResolveError::UnknownModule {
            specifier,
            package,
            expected_path,
            known_modules,
        } => crate::package_diagnostic_error(format!(
            "unknown library module `{specifier}` for provider `{package}`; expected {}; known modules: {}",
            expected_path.display(),
            known_modules.join(", ")
        ))
        .with_file(file.display().to_string())
        .with_arg("issue", "unknown_library_module")
        .with_arg("specifier", specifier)
        .with_arg("provider", package)
        .with_arg("expected_path", expected_path.display().to_string())
        .with_arg("known_modules", known_modules.join(",")),
        LibraryResolveError::MissingLibraryHome { specifier, hint } => crate::package_diagnostic_error(format!(
            "library import `{specifier}` needs a public library home; {hint}"
        ))
        .with_file(file.display().to_string())
        .with_arg("issue", "missing_library_home")
        .with_arg("specifier", specifier),
        LibraryResolveError::MissingLockedPackage {
            specifier,
            provider,
            version,
        } => {
            let version_hint = version
                .as_deref()
                .map(|v| format!(" (`{provider} = \"{v}\"`)"))
                .unwrap_or_default();
            crate::package_diagnostic_error(format!(
                "library import `{specifier}` is declared in faber.toml{version_hint} but missing from faber.lock; install the package with the package manager"
            ))
            .with_file(file.display().to_string())
            .with_arg("issue", "missing_locked_package")
            .with_arg("specifier", specifier)
            .with_arg("provider", provider)
        }
        LibraryResolveError::UndeclaredProvider {
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

pub(super) fn inner_library_import_unresolved_diagnostic(
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

pub(super) fn library_import_kind_diagnostic(
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
        radix::kernel::kernel_script_mode_only_message()
    ))
    .with_file(file.display().to_string())
    .with_arg("issue", "kernel_import_script_mode_only")
    .with_arg("path", import_path.to_owned())
}

pub(super) fn import_unsupported_diagnostic(
    file: &Path,
    decl: &ImportDecl,
    import_path: &str,
) -> Diagnostic {
    let kind = match &decl.kind {
        ImportKind::Named { .. } => "import",
        ImportKind::Wildcard { .. } => "wildcard import",
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
