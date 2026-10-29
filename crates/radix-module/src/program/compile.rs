//! CLI compilation helpers, import-contract resolution, and package-mode policy.
//!
//! This file is the radix CLI compile boundary: read a single-file input,
//! resolve its import contract and package identities, and compile through
//! [`crate::Compiler`] / [`crate::driver::Session`]. Package inputs are rejected
//! with a message pointing at the `faber` CLI.

use super::export_surface::{import_binding, program_declaration_surface, program_export_surface};
use crate::file_interface::{
    FileExport, FileExportKind, FileInterface, InterfaceAliasExport, InterfaceAnnotationContract,
    InterfaceAnnotationContractField, InterfaceBodyChannel, InterfaceCallable, InterfaceCallableParam,
    InterfaceConstValue, InterfaceDeviceContract, InterfaceDeviceRole, InterfaceDeviceVisibility, InterfaceEnumVariant,
    InterfaceGenericParam, InterfaceGenericParamDefault, InterfaceIndexSnapshot, InterfaceLibraryIdentity,
    InterfaceMethodExport, InterfaceNominalExport, InterfaceNominalKind, InterfaceNominalType, InterfaceParam,
    InterfaceStructExport, InterfaceStructField, InterfaceTensorEdgePolicyRequirement, InterfaceTypeParamConstraint,
    InterfaceTypeSnapshot,
};
use crate::hir::HirItemKind;
use crate::semantic::numeric_width::BitsWidth;
use crate::semantic::passes::visibility_annotation::{VisibilityTier, member_visibility};
use crate::semantic::{ContractFieldType, InstansPrecision, NumericWidth, Primitive, SemanticParamMode, Type};
use crate::syntax::{
    Annotation, AnnotationFieldValue, AnnotationKind, AstParamMode, ClassDecl, ClassMemberKind, ExprKind, FiguraExpr,
    Literal, Program, Stmt, StmtKind, TypeExpr, TypeExprKind,
};
use handroll::FxHashMap;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Session import-target memo: `(reader-pack id, target path)` → export
/// surface, full declaration tiers (including module-private), interface, and
/// the resolved target path.
#[derive(Clone)]
pub(crate) struct CachedImportTarget {
    exports: BTreeMap<String, VisibilityTier>,
    declarations: BTreeMap<String, VisibilityTier>,
    interface: FileInterface,
    /// Path the target resolved to when this entry was filled. Retained so
    /// consumers can recompute the package seam relative to the *current*
    /// consumer without a fresh filesystem resolution, which would fail once
    /// the target has been deleted and change the contract across calls.
    target_path: PathBuf,
    /// The target's declared `regio` name (D7.7): the default binding for a
    /// path-inferred import of this target.
    region_name: Option<String>,
    /// Analyzed unit carried for HIR emission (ARCH-U4 / S01-3): the target
    /// is analyzed once per driver run, right where this entry parses it,
    /// under the contract codegen needs — backend emission maps over carried
    /// units instead of re-entering the front half from disk. `None` only in
    /// the transient window between the parse-memo insert (the recursion
    /// brake) and the analysis back-patch, and for targets a cyclic import
    /// re-enters during that window.
    provider_unit: Option<crate::driver::CarriedProviderUnit>,
}

/// Memoized outcome for one import target (KPC-WIRE, advisory D2).
///
/// `Parsed` serves the cached surface. `Unparsable` records a target that
/// resolved and read but produced no parseable program: the failure is
/// memoized with the surface so a repeated import of the same physical file
/// reports the same named import-site failure once per driver run instead of
/// silently matching no namespace (advisory D5 — the session memo both
/// scopes and deduplicates the failure).
#[derive(Clone)]
pub(crate) enum CachedImportTargetEntry {
    Parsed(Box<CachedImportTarget>),
    Unparsable(PathBuf),
}

/// Lookup result for one import target.
///
/// `Unresolved` keeps the pre-existing silent skip for absent, disallowed, or
/// unreadable non-provider targets — surfaceless-by-design namespaces (kernel
/// manifests, bare-driver empty contracts) live here and must not trip the
/// fail-closed gate.
enum ImportTargetLookup {
    Parsed(Box<CachedImportTarget>),
    Unparsable(PathBuf),
    /// A provider-qualified specifier that named no library module.
    UnresolvedProvider(Box<crate::import_resolve::ImportResolveError>),
    Unresolved,
}

/// Map type of the driver-run-scoped import-target memo cell carried by
/// [`SessionMemos`](crate::driver::SessionMemos) (ARCH-U3 / S01-9).
pub(crate) type ImportTargetCache = FxHashMap<(Option<String>, PathBuf), CachedImportTargetEntry>;

/// True when the file interface exports at least one device-bearing
/// function — the only imports whose interface the provider route carries
/// (DFV2-3). Recorded per consumer on
/// [`PackageImportIdentities::device_interfaces`](crate::driver::PackageImportIdentities::device_interfaces).
fn interface_bears_device(interface: &FileInterface) -> bool {
    interface
        .exports
        .values()
        .any(|export| matches!(&export.kind, FileExportKind::Function(callable) if callable.device.is_some()))
}

type ReaderPackRef<'a> = Option<&'a crate::locale::LocalePack>;

#[allow(clippy::result_large_err)]
fn read_input_source(input: &[String]) -> Result<(String, String), crate::Diagnostic> {
    if input.is_empty() || input[0] == "-" {
        let mut source = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut source).map_err(|err| {
            crate::Diagnostic::error(format!("failed to read stdin: {err}"))
                .with_phase(crate::diagnostics::DiagnosticPhase::Io)
                .with_code("IO001")
                .with_arg("issue", "stdin_read_failed")
        })?;
        return Ok(("<stdin>".to_owned(), source));
    }
    let path = PathBuf::from(&input[0]);
    let source = std::fs::read_to_string(&path).map_err(|err| {
        crate::Diagnostic::io_error(&path, &err)
            .with_code("IO001")
            .with_arg("issue", "source_read_failed")
    })?;
    Ok((input[0].clone(), source))
}

fn package_reject(message: &str, issue: &str) -> crate::CompileResult {
    crate::CompileResult {
        output: None,
        diagnostics: vec![
            crate::Diagnostic::error(message.to_owned())
                .with_phase(crate::diagnostics::DiagnosticPhase::Tool)
                .with_code("TOOL002")
                .with_arg("issue", issue),
        ],
    }
}

/// Return whether an input path syntactically names package mode.
#[must_use]
pub fn should_treat_as_package(path: &Path) -> bool {
    path.is_dir() || path.file_name().and_then(|name| name.to_str()) == Some("faber.toml")
}

/// Combine explicit package mode with path-based package detection.
#[must_use]
pub fn resolve_package_mode(path: &Path, force_package: bool) -> bool {
    force_package || should_treat_as_package(path)
}

/// Compile command input after applying stdin and package-mode policy.
#[must_use]
pub fn compile_cli_input(input: &[String], package: bool, target: crate::codegen::Target) -> crate::CompileResult {
    compile_cli_input_with_locale_pack(input, package, target, None, crate::codegen::OutputMode::Application, None)
}

/// Compile command input with optional reader-pack source tokenization.
#[must_use]
pub fn compile_cli_input_with_locale_pack(
    input: &[String],
    package: bool,
    target: crate::codegen::Target,
    code_pack: ReaderPackRef<'_>,
    output_mode: crate::codegen::OutputMode,
    module_name: Option<&str>,
) -> crate::CompileResult {
    compile_cli_input_with_locale_pack_inner(input, package, target, code_pack, output_mode, module_name, false)
}

/// Compile command input with an explicit reader pack that must win over any
/// source frontmatter locale (radix CLI `--locale` semantics).
#[must_use]
pub fn compile_cli_input_with_locale_pack_explicit(
    input: &[String],
    package: bool,
    target: crate::codegen::Target,
    code_pack: ReaderPackRef<'_>,
    output_mode: crate::codegen::OutputMode,
    module_name: Option<&str>,
) -> crate::CompileResult {
    compile_cli_input_with_locale_pack_inner(input, package, target, code_pack, output_mode, module_name, true)
}

/// Resolves the opaque route-to-spelling maps for the library package roots a
/// single-file input imports. The caller (the `faber` layer) owns reading
/// binding manifests; the compiler only transports the resulting maps.
pub type LibraryBindingsResolver<'a> = &'a dyn Fn(&BTreeSet<PathBuf>) -> crate::driver::TargetBindings;

/// Compile command input like [`compile_cli_input_with_locale_pack`], then hand
/// the maps `library_bindings` returns for the input's imported library roots
/// to the driver as `Config::target_bindings`. Package and stdin inputs take
/// the existing route unchanged (stdin has no import closure).
#[must_use]
pub fn compile_cli_input_with_library_bindings(
    input: &[String],
    package: bool,
    target: crate::codegen::Target,
    code_pack: ReaderPackRef<'_>,
    output_mode: crate::codegen::OutputMode,
    module_name: Option<&str>,
    library_bindings: LibraryBindingsResolver<'_>,
) -> crate::CompileResult {
    let Some(first) = input.first().filter(|first| first.as_str() != "-") else {
        return compile_cli_input_with_locale_pack(input, package, target, code_pack, output_mode, module_name);
    };
    let path = PathBuf::from(first);
    if resolve_package_mode(&path, package) {
        return compile_cli_input_with_locale_pack(input, package, target, code_pack, output_mode, module_name);
    }
    let source = match read_input_source(input) {
        Ok((_name, source)) => source,
        Err(diagnostic) => return crate::CompileResult { output: None, diagnostics: vec![diagnostic] },
    };
    let session = crate::driver::Session::new(config_for_target(target, code_pack, output_mode, module_name, false));
    match prepare_program_module_with_bindings(&path, &source, &session, Some(library_bindings)) {
        Ok(prepared) => crate::driver::compile(
            &prepared.session,
            &path.display().to_string(),
            &source,
            crate::driver::ModuleCompileRequest {
                contract: prepared.contract,
                package_identities: prepared.package_identities,
                provider_units: prepared.provider_units,
            },
        ),
        Err(diagnostic) => crate::CompileResult { output: None, diagnostics: vec![*diagnostic] },
    }
}

fn compile_cli_input_with_locale_pack_inner(
    input: &[String],
    package: bool,
    target: crate::codegen::Target,
    code_pack: ReaderPackRef<'_>,
    output_mode: crate::codegen::OutputMode,
    module_name: Option<&str>,
    explicit: bool,
) -> crate::CompileResult {
    if input.is_empty() || input[0] == "-" {
        if package {
            return package_reject("package compilation requires a path input", "package_requires_path_input");
        }

        let (name, source) = match read_input_source(input) {
            Ok(read) => read,
            Err(diagnostic) => return crate::CompileResult { output: None, diagnostics: vec![diagnostic] },
        };
        return compile_cli_source_with_locale_pack_inner(
            &name,
            &source,
            target,
            code_pack,
            output_mode,
            module_name,
            explicit,
        );
    }

    let path = PathBuf::from(&input[0]);
    compile_cli_path_with_locale_pack_inner(
        &path,
        resolve_package_mode(&path, package),
        target,
        code_pack,
        output_mode,
        module_name,
        explicit,
    )
}

/// Compile a single file path through the public compiler API.
#[must_use]
pub fn compile_cli_path(path: &Path, package: bool, target: crate::codegen::Target) -> crate::CompileResult {
    compile_cli_path_with_locale_pack(path, package, target, None, crate::codegen::OutputMode::Application, None)
}

/// Compile a single file path with optional reader-pack source tokenization.
#[must_use]
pub fn compile_cli_path_with_locale_pack(
    path: &Path,
    package: bool,
    target: crate::codegen::Target,
    code_pack: ReaderPackRef<'_>,
    output_mode: crate::codegen::OutputMode,
    module_name: Option<&str>,
) -> crate::CompileResult {
    compile_cli_path_with_locale_pack_inner(path, package, target, code_pack, output_mode, module_name, false)
}

fn compile_cli_path_with_locale_pack_inner(
    path: &Path,
    package: bool,
    target: crate::codegen::Target,
    code_pack: ReaderPackRef<'_>,
    output_mode: crate::codegen::OutputMode,
    module_name: Option<&str>,
    explicit: bool,
) -> crate::CompileResult {
    if package || should_treat_as_package(path) {
        return package_reject(
            "package compilation is owned by the `faber` tool; rerun with `faber build` or `faber emit`",
            "package_compilation_owned_by_faber",
        );
    }

    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(err) => {
            return crate::CompileResult {
                output: None,
                diagnostics: vec![
                    crate::Diagnostic::io_error(path, &err)
                        .with_code("IO001")
                        .with_arg("issue", "source_read_failed"),
                ],
            };
        }
    };
    let name = path.display().to_string();
    let session = crate::driver::Session::new(config_for_target(target, code_pack, output_mode, module_name, explicit));
    let mut result = compile_prepared_program_path(&session, path, &source);

    if target == crate::codegen::Target::HirFaber && result.success() {
        result.output = Some(crate::Output::Faber(crate::FaberOutput {
            code: peeled_source_body(&name, &source).unwrap_or(source),
        }));
    }

    result
}

/// Compile in-memory command source through the public compiler API.
#[must_use]
pub fn compile_cli_source(name: &str, source: &str, target: crate::codegen::Target) -> crate::CompileResult {
    compile_cli_source_with_locale_pack(name, source, target, None, crate::codegen::OutputMode::Application, None)
}

/// Compile in-memory command source with optional reader-pack source tokenization.
#[must_use]
pub fn compile_cli_source_with_locale_pack(
    name: &str,
    source: &str,
    target: crate::codegen::Target,
    code_pack: ReaderPackRef<'_>,
    output_mode: crate::codegen::OutputMode,
    module_name: Option<&str>,
) -> crate::CompileResult {
    compile_cli_source_with_locale_pack_inner(name, source, target, code_pack, output_mode, module_name, false)
}

fn compile_cli_source_with_locale_pack_inner(
    name: &str,
    source: &str,
    target: crate::codegen::Target,
    code_pack: ReaderPackRef<'_>,
    output_mode: crate::codegen::OutputMode,
    module_name: Option<&str>,
    explicit: bool,
) -> crate::CompileResult {
    compiler_for_target(target, code_pack, output_mode, module_name, explicit).compile_str(name, source)
}

fn compiler_for_target(
    target: crate::codegen::Target,
    code_pack: ReaderPackRef<'_>,
    output_mode: crate::codegen::OutputMode,
    module_name: Option<&str>,
    explicit: bool,
) -> crate::Compiler {
    crate::Compiler::new(config_for_target(target, code_pack, output_mode, module_name, explicit))
}

fn config_for_target(
    target: crate::codegen::Target,
    code_pack: ReaderPackRef<'_>,
    output_mode: crate::codegen::OutputMode,
    module_name: Option<&str>,
    explicit: bool,
) -> crate::driver::Config {
    let config = crate::driver::Config::default()
        .with_target(target)
        .with_output_mode(output_mode)
        .with_dev_locale_root();
    let config = match module_name {
        Some(name) => config.with_module_name(Some(name.to_owned())),
        None => config,
    };
    match code_pack {
        Some(pack) if explicit => config.with_resolved_locale_pack(pack.clone()),
        Some(pack) => config.with_locale_pack(pack.clone()),
        None => config,
    }
}

#[derive(Debug, Default, PartialEq)]
pub(crate) struct ImportContract {
    /// Importable names per binding, with their visibility tier. `@ interna`
    /// names stay in this surface (tier preserved — the seam check decides
    /// cross-package importability); unmarked and `@ privata` declarations
    /// are absent.
    pub(crate) namespace_exports: BTreeMap<String, BTreeMap<String, VisibilityTier>>,
    /// Per-binding same-package flag from `find_package_root`.
    pub(crate) namespace_seams: BTreeMap<String, bool>,
    /// All target top-level names including module-private, keyed by binding.
    pub(crate) namespace_declarations: BTreeMap<String, BTreeMap<String, VisibilityTier>>,
    pub(crate) file_interfaces: BTreeMap<String, FileInterface>,
    /// Package roots of resolved `provider:module` imports. Used to load
    /// library-owned `locale/<consumer-locale>/pack.toml` fragments.
    pub(crate) library_package_roots: BTreeSet<PathBuf>,
    /// Analyzed provider units for this module's direct import edges, keyed
    /// by import-path spelling (ARCH-U4 / S01-3): carriage for backend
    /// emission, not a resolver fact — `into_module_contract` leaves it here
    /// and the program-layer entries move it onto the analyzed output.
    pub(crate) provider_units: crate::driver::ProviderUnitView,
    /// Fail-closed import-site facts (KPC-WIRE, advisory D2): imported
    /// libraries that resolved and read but did not parse. The binding's
    /// namespace surface stays uninstalled — it stops silently matching — and
    /// the driver surfaces each fact as a named import-site `Diagnostic`.
    pub(crate) import_site_failures: Vec<ImportSiteFailure>,
    /// Provider-qualified imports that could not resolve to a library module;
    /// the driver turns the first fact into the package diagnostic identity.
    pub(crate) import_resolve_failures: Vec<ImportResolveFailure>,
    /// Import path spelling → target `regio` name for default-named imports
    /// whose target declares one (D7.7). Every binding-keyed map above is
    /// keyed by the declared name; the driver rewrites its own parse to match.
    pub(crate) default_import_names: BTreeMap<String, String>,
}

/// One parse-failed import target, recorded at the consumer's import site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ImportSiteFailure {
    /// Import binding the consumer declared (`import from … math`).
    pub(crate) binding: String,
    /// Import path spelling as written by the consumer.
    pub(crate) import_path: String,
    /// Resolved target path that failed to parse.
    pub(crate) target_path: PathBuf,
}

/// One provider-qualified import that failed during library-module resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ImportResolveFailure {
    pub(crate) binding: String,
    pub(crate) import_path: String,
    pub(crate) provider: String,
    pub(crate) message: String,
    pub(crate) cause_issue: &'static str,
}

/// Named import-site diagnostic for a parse-failed imported library (KPC-WIRE
/// `done_when` (i), advisory D2/D3): rides the driver-side [`Diagnostic`]
/// channel only — never a `FileInterfaceError` or an interface-type change.
fn import_site_parse_failure_diagnostic(consumer: &Path, failure: &ImportSiteFailure) -> crate::Diagnostic {
    crate::Diagnostic::error(format!(
        "imported library `{}` from `{}` did not parse: {}",
        failure.binding,
        failure.import_path,
        failure.target_path.display()
    ))
    .with_phase(crate::diagnostics::DiagnosticPhase::Parse)
    .with_code("IMP001")
    .with_arg("issue", "imported_library_parse_failure")
    .with_arg("binding", failure.binding.as_str())
    .with_arg("import_path", failure.import_path.as_str())
    .with_file(consumer.display().to_string())
}

/// Named package diagnostic for a provider-qualified import that resolved no module.
fn import_resolve_failure_diagnostic(consumer: &Path, failure: &ImportResolveFailure) -> crate::Diagnostic {
    crate::Diagnostic::error(format!(
        "unknown library module `{}` for provider `{}`: {}",
        failure.import_path, failure.provider, failure.message
    ))
    .with_phase(crate::diagnostics::DiagnosticPhase::Analysis)
    .with_code("PKG001")
    .with_file(consumer.display().to_string())
    .with_arg("issue", "unknown_library_module")
    .with_arg("specifier", failure.import_path.clone())
    .with_arg("provider", failure.provider.clone())
    .with_arg("cause_issue", failure.cause_issue)
}

impl ImportContract {
    fn into_module_contract(self) -> crate::driver::ModuleImportContract {
        crate::driver::ModuleImportContract {
            namespace_exports: self.namespace_exports,
            file_interfaces: self.file_interfaces,
            namespace_seams: self.namespace_seams,
            namespace_declarations: self.namespace_declarations,
            default_import_names: self.default_import_names,
        }
    }
}

/// Program-layer compile of one on-disk source: discover contracts, then
/// fail-closed module analysis.
#[must_use]
pub fn compile_prepared_program_path(
    session: &crate::driver::Session,
    path: &Path,
    source: &str,
) -> crate::CompileResult {
    match prepare_program_module(path, source, session) {
        Ok(prepared) => crate::driver::compile(
            &prepared.session,
            &path.display().to_string(),
            source,
            crate::driver::ModuleCompileRequest {
                contract: prepared.contract,
                package_identities: prepared.package_identities,
                provider_units: prepared.provider_units,
            },
        ),
        Err(diagnostic) => crate::CompileResult { output: None, diagnostics: vec![*diagnostic] },
    }
}

pub struct PreparedProgramModule {
    pub session: crate::driver::Session,
    pub contract: crate::driver::ModuleImportContract,
    pub package_identities: Option<crate::driver::PackageImportIdentities>,
    /// Analyzed provider units for this module's import edges (ARCH-U4 /
    /// S01-3): carriage for the analyzed output — the program-layer analysis
    /// entries install it on the returned unit, and `compile_prepared_*`
    /// threads it through [`crate::driver::ModuleCompileRequest`].
    pub provider_units: crate::driver::ProviderUnitView,
}

/// Build caller-supplied analysis inputs from a real file path.
///
/// This is the program/discovery helper. Module analysis must not call it.
/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn prepare_program_module(
    path: &Path,
    source: &str,
    session: &crate::driver::Session,
) -> Result<PreparedProgramModule, Box<crate::Diagnostic>> {
    prepare_program_module_with_bindings(path, source, session, None)
}

/// [`prepare_program_module`] that also installs `Config::target_bindings` from
/// the library package roots the module's imports resolved to.
fn prepare_program_module_with_bindings(
    path: &Path,
    source: &str,
    session: &crate::driver::Session,
    library_bindings: Option<LibraryBindingsResolver<'_>>,
) -> Result<PreparedProgramModule, Box<crate::Diagnostic>> {
    let templates = session.config.import_path_templates.as_ref();
    let mut pack = if session.config.locale_pack_explicit {
        session.config.locale_pack.clone()
    } else {
        frontmatter_locale_pack_from_source(&session.config, &path.display().to_string(), source)
            .or_else(|| session.config.locale_pack.clone())
    };
    let mut parsed = parse_program_for_namespace_exports(&path.display().to_string(), source, pack.as_ref());
    let import = parsed
        .as_ref()
        .map_or_else(ImportContract::default, |(program, interner)| {
            import_contract_for_parsed_file(session, path, program, interner, pack.as_ref(), templates)
        });
    if let Some(pack) = pack.as_mut() {
        crate::locale::merge_library_locale_packs(pack, &import.library_package_roots).map_err(|err| {
            Box::new(
                crate::Diagnostic::error(format!(
                    "failed to load library locale pack: {}",
                    crate::locale::format_locale_pack_load_error(&err)
                ))
                .with_phase(crate::diagnostics::DiagnosticPhase::Lex)
                .with_code("LOCALE001")
                .with_arg("issue", "library_locale_pack_load_failed")
                .with_file(path.display().to_string()),
            )
        })?;
    }
    // Fail-closed import-site propagation (KPC-WIRE done_when (i), advisory
    // D2): a parse-failed imported library stops the compile with its named
    // diagnostic instead of degrading to an interfaceless namespace. The
    // first failure names the site; the contract carries every fact.
    if let Some(failure) = import.import_site_failures.first() {
        return Err(Box::new(import_site_parse_failure_diagnostic(path, failure)));
    }
    if let Some(failure) = import.import_resolve_failures.first() {
        return Err(Box::new(import_resolve_failure_diagnostic(path, failure)));
    }
    let mut config = session.config.clone();
    if let Some(pack) = pack.as_ref() {
        config = config.with_resolved_locale_pack(pack.clone());
    }
    if let Some(resolve) = library_bindings {
        config = config.with_target_bindings(resolve(&import.library_package_roots));
    }
    // Package identities key by binding: rewrite default-named imports to
    // their targets' declared names first (D7.7), as the driver will.
    if let Some((program, interner)) = parsed.as_mut() {
        super::default_import::apply_default_import_names(program, interner, &import.default_import_names);
    }
    let provider_units = import.provider_units.clone();
    Ok(PreparedProgramModule {
        // Per-module re-wrap of the caller's session (ARCH-U3): the prepared
        // module's session carries the resolved pack config but shares memo
        // identity with the driver run, so target parses performed during
        // contract discovery serve the analysis that follows.
        session: session.re_wrap(config),
        contract: import.into_module_contract(),
        package_identities: parsed.as_ref().and_then(|(program, interner)| {
            package_identity_facts_for_parsed_file(session, path, program, interner, pack.as_ref(), templates)
                .filter(|facts| !facts.imports.is_empty())
        }),
        provider_units,
    })
}

/// Program/discovery helper keyed by diagnostic name: walk only when `name`
/// is an existing file. In-memory names keep empty contracts.
/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn prepare_program_module_for_name(
    name: &str,
    source: &str,
    session: &crate::driver::Session,
) -> Result<PreparedProgramModule, Box<crate::Diagnostic>> {
    let path = Path::new(name);
    if path.is_file() {
        prepare_program_module(path, source, session)
    } else {
        Ok(PreparedProgramModule {
            // Re-wrap, not a fresh run: in-memory names keep the driver's
            // memo identity even without a contract walk (ARCH-U3).
            session: session.re_wrap(session.config.clone()),
            contract: crate::driver::ModuleImportContract::default(),
            package_identities: None,
            provider_units: crate::driver::ProviderUnitView::default(),
        })
    }
}

/// Analyze one on-disk source after program-layer contract discovery.
///
/// Module analysis stays fail-closed. Path-shaped e2e / interpret / verify
/// callers enter here instead of empty-contract [`crate::driver::analyze_source`].
/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn analyze_prepared_program_path(
    session: &crate::driver::Session,
    path: &Path,
    source: &str,
) -> Result<crate::driver::AnalyzedModule, Vec<crate::Diagnostic>> {
    let prepared = prepare_program_module(path, source, session).map_err(|diagnostic| vec![*diagnostic])?;
    let PreparedProgramModule { session: analysis_session, contract, package_identities: _, provider_units } = prepared;
    let mut unit = crate::driver::analyze_source(
        &analysis_session,
        &path.display().to_string(),
        source,
        crate::driver::AnalyzeRequest { cli_program: None, contract },
    )?;
    // Carry the analyzed provider units on the analyzed output (ARCH-U4 /
    // S01-3): backend emission maps over the view instead of re-entering the
    // front half from disk.
    unit.provider_units = provider_units;
    Ok(unit)
}

/// Analyze a diagnostic name after program-layer discovery when `name` is a file.
///
/// In-memory names keep empty contracts.
/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn analyze_prepared_program_name(
    session: &crate::driver::Session,
    name: &str,
    source: &str,
) -> Result<crate::driver::AnalyzedModule, Vec<crate::Diagnostic>> {
    let prepared = prepare_program_module_for_name(name, source, session).map_err(|diagnostic| vec![*diagnostic])?;
    let PreparedProgramModule { session: analysis_session, contract, package_identities: _, provider_units } = prepared;
    let mut unit = crate::driver::analyze_source(
        &analysis_session,
        name,
        source,
        crate::driver::AnalyzeRequest { cli_program: None, contract },
    )?;
    // Carry the analyzed provider units on the analyzed output (ARCH-U4 /
    // S01-3), mirroring `analyze_prepared_program_path`.
    unit.provider_units = provider_units;
    Ok(unit)
}

/// Resolve and parse one import target through the session cache.
///
/// Returns the cached exports and `FileInterface` when available. Relative
/// imports are probed before resolution (key = joined spelling, mirroring
/// `resolve_fab_file`'s extension normalization), so a previously parsed
/// target is served even if the target file has since been deleted — no I/O
/// and no parse on the hit path. On a miss the target is resolved, looked up
/// under the raw and canonical path keys, and read+parsed once, with both
/// keys inserted on success.
///
/// A target that resolved and read but did not parse returns
/// [`ImportTargetLookup::Unparsable`] (KPC-WIRE, advisory D2): the parse
/// failure is a named import-site fact for the consumer, never a silent
/// `None`. Provider-qualified resolution failures retain their structured
/// error for the consumer's package diagnostic; unresolved relative and
/// `§` targets stay [`ImportTargetLookup::Unresolved`].
fn cached_import_target(
    session: &crate::driver::Session,
    from_file: &Path,
    import_path: &str,
    pack_key: Option<&String>,
    code_pack: ReaderPackRef<'_>,
    templates: Option<&BTreeMap<String, String>>,
) -> ImportTargetLookup {
    fn serve(entry: Option<CachedImportTargetEntry>) -> Option<ImportTargetLookup> {
        entry.map(|entry| match entry {
            CachedImportTargetEntry::Parsed(target) => ImportTargetLookup::Parsed(target),
            CachedImportTargetEntry::Unparsable(target_path) => ImportTargetLookup::Unparsable(target_path),
        })
    }

    if import_path.starts_with("./") {
        // Fast path: derive the would-be resolved target without touching the
        // filesystem, so a session-cached target is served after deletion.
        let Some(parent) = from_file.parent() else {
            return ImportTargetLookup::Unresolved;
        };
        let candidate = parent.join(import_path);
        let probe_key = (pack_key.cloned(), candidate.with_extension("fab"));
        if let Some(lookup) = serve(
            session
                .memos
                .import_targets
                .borrow()
                .get(&probe_key)
                .cloned(),
        ) {
            return lookup;
        }
    }
    let target_path = match resolve_namespace_import(session, from_file, import_path, templates) {
        Ok(target_path) => target_path,
        Err(error) => {
            return if provider_segments(import_path).is_some() {
                ImportTargetLookup::UnresolvedProvider(Box::new(error))
            } else {
                ImportTargetLookup::Unresolved
            };
        }
    };
    let raw_key = (pack_key.cloned(), target_path.clone());
    if let Some(lookup) = serve(session.memos.import_targets.borrow().get(&raw_key).cloned()) {
        return lookup;
    }
    let canonical_key = std::fs::canonicalize(&target_path)
        .ok()
        .map(|canonical| (pack_key.cloned(), canonical));
    if let Some(key) = &canonical_key
        && let Some(lookup) = serve(session.memos.import_targets.borrow().get(key).cloned())
    {
        return lookup;
    }
    let Ok(target_source) = std::fs::read_to_string(&target_path) else {
        return ImportTargetLookup::Unresolved;
    };
    let target_pack = import_target_locale_pack(session, &target_path, &target_source, code_pack);
    let Some((target_program, target_interner)) =
        parse_program_for_namespace_exports(&target_path.display().to_string(), &target_source, target_pack.as_ref())
    else {
        // Fail-closed parse-failure propagation (KPC-WIRE, advisory D2): the
        // library has content but no parseable program. Memoize the failure
        // under both path keys so a repeated import reports once.
        {
            let mut cache = session.memos.import_targets.borrow_mut();
            let entry = CachedImportTargetEntry::Unparsable(target_path.clone());
            cache.insert(raw_key, entry.clone());
            if let Some(key) = canonical_key {
                cache.insert(key, entry);
            }
        }
        return ImportTargetLookup::Unparsable(target_path);
    };
    let exports = program_export_surface(&target_program, &target_interner);
    let declarations = program_declaration_surface(&target_program, &target_interner);
    let module_identity = import_target_identity(import_path, None);
    let interface = file_interface_for_program(&target_program, &target_interner, module_identity.as_ref());
    let region_name = super::default_import::program_region_name(&target_program, &target_interner);
    let cached = CachedImportTarget {
        exports,
        declarations,
        interface,
        target_path: target_path.clone(),
        region_name,
        provider_unit: None,
    };
    {
        let mut cache = session.memos.import_targets.borrow_mut();
        let entry = CachedImportTargetEntry::Parsed(Box::new(cached.clone()));
        cache.insert(raw_key.clone(), entry.clone());
        if let Some(key) = canonical_key.clone() {
            cache.insert(key, entry);
        }
    }
    // Analyze the target under the contract codegen needs and carry the unit
    // on this memo entry (ARCH-U4 / S01-3): emission maps over carried units
    // instead of re-entering the front half from disk at every codegen call.
    // The parse entry above is inserted first because the analysis re-enters
    // contract discovery for the target's own imports — a provider import
    // cycle must hit the memo instead of recursing.
    let provider_unit = provider_unit_for_target(session, &target_path, &target_source);
    // ST2-1: the AST-built interface carries constant types only; the folded
    // values come from the analyzed provider unit, so a consumer's `=`
    // initializer folds `module.NAME` from this run's source, never a stale one.
    let mut interface = cached.interface.clone();
    if let Some(unit) = provider_unit.lock_unit() {
        apply_tensor_edge_policy_requirements(&mut interface, &unit);
    }
    let const_values = provider_const_values(&provider_unit);
    apply_const_values(&mut interface, &const_values);
    // ST3-2: the AST-built class fields carry no initializer; the analyzed
    // provider unit supplies the genus static-field defaults a consumer's
    // `module.Tab.Z` read inlines.
    apply_static_field_defaults(&mut interface, &provider_unit);
    {
        let mut cache = session.memos.import_targets.borrow_mut();
        for key in std::iter::once(raw_key).chain(canonical_key) {
            if let Some(CachedImportTargetEntry::Parsed(entry)) = cache.get_mut(&key) {
                entry.provider_unit = Some(provider_unit.clone());
                entry.interface = interface.clone();
            }
        }
    }
    ImportTargetLookup::Parsed(Box::new(CachedImportTarget {
        provider_unit: Some(provider_unit),
        interface,
        ..cached
    }))
}

/// Folded values of the provider unit's module constants, by export name.
/// A failed provider carries none, so its constants stay value-less.
fn provider_const_values(provider: &crate::driver::CarriedProviderUnit) -> Vec<(String, InterfaceConstValue)> {
    let Some(unit) = provider.lock_unit() else {
        return Vec::new();
    };
    unit.hir
        .items
        .iter()
        .filter_map(|item| {
            let HirItemKind::Constant(konst) = &item.kind else {
                return None;
            };
            let value = crate::semantic::passes::statica::export_value(
                &konst.value,
                konst.ty.or(konst.value.ty),
                &unit.types,
                &unit.interner,
            )?;
            Some((unit.interner.resolve(konst.name).to_owned(), value))
        })
        .collect()
}

fn apply_const_values(interface: &mut FileInterface, values: &[(String, InterfaceConstValue)]) {
    for (name, value) in values {
        if let Some(FileExport { kind: FileExportKind::Const(_, slot), .. }) = interface.exports.get_mut(name) {
            *slot = Some(value.clone());
        }
    }
}

/// Patch the analyzed provider unit's genus static-field initializers into the
/// sibling interface's class fields. A failed provider carries none, so its
/// fields stay `default: None` and the consumer's `module.Tab.Z` read keeps
/// failing closed.
fn apply_static_field_defaults(interface: &mut FileInterface, provider: &crate::driver::CarriedProviderUnit) {
    let Some(unit) = provider.lock_unit() else {
        return;
    };
    for item in &unit.hir.items {
        let HirItemKind::Struct(strukt) = &item.kind else {
            continue;
        };
        let name = unit.interner.resolve(strukt.name).to_owned();
        let Some(FileExport { kind: FileExportKind::Struct(export), .. }) = interface.exports.get_mut(&name) else {
            continue;
        };
        for field in &strukt.fields {
            if !field.is_static() {
                continue;
            }
            let Some(init) = &field.init else {
                continue;
            };
            let field_name = unit.interner.resolve(field.name).to_owned();
            if let Some(interface_field) = export.fields.iter_mut().find(|f| f.name == field_name) {
                interface_field.default = crate::file_interface::InterfaceFieldDefault::snapshot(
                    init,
                    &unit.types,
                    &unit.interner,
                    &unit.resolver,
                    interface.identity.as_ref(),
                )
                .ok();
            }
        }
    }
}

/// Attach semantic `limes<P>()` requirements to the syntax-built package
/// interface. The initial package surface is needed before target analysis;
/// the carried provider unit is the typed authority that fills this metadata
/// once available.
fn apply_tensor_edge_policy_requirements(interface: &mut FileInterface, provider: &crate::driver::ProviderUnit) {
    let mut rejected_exports = Vec::new();
    for item in &provider.hir.items {
        let HirItemKind::Function(function) = &item.kind else {
            continue;
        };
        let name = provider.interner.resolve(function.name).to_owned();
        let Some(requirements) = provider
            .tensor_edge_policies
            .function_requirements
            .get(&item.def_id)
        else {
            continue;
        };
        let Some(FileExport { kind: FileExportKind::Function(callable), .. }) = interface.exports.get_mut(&name) else {
            continue;
        };

        let portable = requirements
            .iter()
            .map(|requirement| {
                let mut policy_type = requirement.policy_type;
                while let Type::Alias(_, inner) = provider.types.get(policy_type) {
                    policy_type = *inner;
                }
                let Type::Param(policy_param) = provider.types.get(policy_type) else {
                    return None;
                };
                let policy_param = provider.interner.resolve(*policy_param).to_owned();
                if !callable
                    .type_params
                    .iter()
                    .any(|param| param.kind == crate::syntax::GenericParamKind::Typus && param.name == policy_param)
                {
                    return None;
                }
                let element_type = crate::file_interface::snapshot_interface_type_with_resolver(
                    requirement.element_type,
                    &provider.types,
                    &provider.interner,
                    &provider.resolver,
                )
                .ok()?;
                Some(InterfaceTensorEdgePolicyRequirement { policy_param, element_type, rank: requirement.rank })
            })
            .collect::<Option<Vec<_>>>();
        if let Some(portable) = portable {
            callable.tensor_edge_policy_requirements = portable;
        } else {
            // A generic policy requirement without a portable type witness
            // must not cross the package boundary as an apparently ordinary
            // callable. The consumer then fails closed at import resolution.
            rejected_exports.push(name);
        }
    }
    for name in rejected_exports {
        interface.exports.remove(&name);
    }
}

/// Analyze one parsed import target under the contract codegen needs
/// (ARCH-U4 / S01-3): bodyless functions, the dev locale root, and the target
/// package's `[reader] locale` pack — the exact configuration the Rust
/// provider-module emitter analyzed under when it re-entered the front half
/// from disk, aligned rather than forked, so carried emission facts cannot
/// differ from the analysis-time facts they replace. Failures are carried as
/// the marker, never propagated: the consumer's compile path does not analyze
/// providers today and must not start failing on their diagnostics; emission
/// fails closed on the marker instead.
///
/// Public so the package-analysis path (radix-program) mints carriage through
/// this same entry instead of forking the provider analysis config.
#[must_use]
pub fn provider_unit_for_target(
    session: &crate::driver::Session,
    target_path: &Path,
    target_source: &str,
) -> crate::driver::CarriedProviderUnit {
    use crate::driver::CarriedProviderUnit;

    let mut config = crate::driver::Config::default()
        .with_bodyless_functions()
        .with_dev_locale_root();
    // The package `[reader] locale` (e.g. tela's `locale = "en"` for
    // `tela:valida`) is threaded as a non-explicit session pack so an English
    // provider stays English under an otherwise Latin-default session instead
    // of failing PARSE001.
    if let Some(pack) = package_reader_locale_pack(session, target_path) {
        config = config.with_locale_pack(pack);
    }
    // Per-provider re-wrap (ARCH-U3): the provider's analysis session carries
    // its own contract config but shares the driver run's memo identity, so
    // the provider's own contract walk reuses the run's parse-once cells.
    let provider_session = session.re_wrap(config);
    let name = target_path.display().to_string();
    let prepared = match prepare_program_module(target_path, target_source, &provider_session) {
        Ok(prepared) => prepared,
        Err(diagnostic) => {
            return CarriedProviderUnit::Failed {
                message: format!("contract discovery failed: {}", diagnostic.message),
            };
        }
    };
    let crate::program::PreparedProgramModule {
        session: analysis_session,
        contract,
        provider_units,
        package_identities: _,
    } = prepared;
    let mut unit = match crate::driver::analyze_source(
        &analysis_session,
        &name,
        target_source,
        crate::driver::AnalyzeRequest { cli_program: None, contract },
    ) {
        Ok(unit) => unit,
        Err(diagnostics) => {
            return CarriedProviderUnit::Failed {
                message: format!(
                    "failed analysis: {}",
                    diagnostics
                        .iter()
                        .map(|diagnostic| diagnostic.message.as_str())
                        .collect::<Vec<_>>()
                        .join("; ")
                ),
            };
        }
    };
    // The provider's own nested imports ride its carried unit the same way
    // the consumer's ride its own — the emitter recurses through this view.
    unit.provider_units = provider_units;
    // Reduce the analyzed unit to the facts emission maps over. The arenas
    // move as-is; the shared handle keeps `AnalyzedModule` `Send` (the
    // `faber test` budget thread sends analyzed programs across a channel).
    let crate::driver::AnalyzedModule {
        hir,
        types,
        interner,
        resolver,
        tensor_edge_policies,
        shape_obligations,
        diagnostics,
        provider_units: nested,
        ..
    } = unit;
    CarriedProviderUnit::Unit(std::sync::Arc::new(std::sync::Mutex::new(crate::driver::ProviderUnit {
        hir,
        types,
        interner,
        resolver,
        tensor_edge_policies,
        shape_obligations,
        diagnostics,
        nested,
    })))
}

/// Build namespace export names and typed file interfaces for a source file's imports.
///
/// Resolves relative paths, `norma:*`, and other `provider:module` imports via
/// the configured library home.
pub(crate) fn import_contract_for_file(
    path: &Path,
    source: &str,
    code_pack: ReaderPackRef<'_>,
    templates: Option<&BTreeMap<String, String>>,
) -> ImportContract {
    // Session-less entry (check path, tests): one call is its own driver run,
    // so it mints a fresh session and its memo scope ends with the call. The
    // compile path threads the driver's own session through
    // `import_contract_for_parsed_file` instead.
    let session = crate::driver::Session::new(crate::driver::Config::default());
    import_contract_for_session(&session, path, source, code_pack, templates)
}

fn import_contract_for_session(
    session: &crate::driver::Session,
    path: &Path,
    source: &str,
    code_pack: ReaderPackRef<'_>,
    templates: Option<&BTreeMap<String, String>>,
) -> ImportContract {
    // The analysis driver resolves a source's `+++ locale = "…" +++` frontmatter
    // into a reader pack before lexing, so the import contract must recognize
    // the same keyword surface or English-surface `import from …` / `const`
    // spellings are missed and module exports never install (corpus converted
    // to the default English locale). Explicit caller packs still win.
    let resolved_pack = code_pack
        .map(ToOwned::to_owned)
        .or_else(|| frontmatter_locale_pack_from_source(&session.config, &path.display().to_string(), source));
    let Some((program, interner)) =
        parse_program_for_namespace_exports(&path.display().to_string(), source, resolved_pack.as_ref())
    else {
        // The consumer itself did not parse. The authoritative diagnostics
        // come from the real compile pass over this source (which lexes and
        // parses it again and reports there); there is no import AST here to
        // name an import site, and an extra unnamed fact would only duplicate
        // that report.
        return ImportContract::default();
    };
    import_contract_for_parsed_file(session, path, &program, &interner, resolved_pack.as_ref(), templates)
}

/// Build an import contract from a program that has already been parsed.
///
/// Keeping the parsed program and interner together lets callers that need
/// several program-layer facts share the same pre-parse. The session is the
/// driver-run carrier: every target lookup in the walk shares its memo cells.
pub(crate) fn import_contract_for_parsed_file(
    session: &crate::driver::Session,
    path: &Path,
    program: &Program,
    interner: &crate::lexer::Interner,
    resolved_pack: ReaderPackRef<'_>,
    templates: Option<&BTreeMap<String, String>>,
) -> ImportContract {
    let mut contract = ImportContract::default();
    let pack_key = resolved_pack.map(|pack| pack.metadata.id.clone());
    let mut transitive_visited: BTreeSet<PathBuf> = BTreeSet::new();
    let mut transitive_interfaces: Vec<(String, FileInterface)> = Vec::new();
    for stmt in &program.statements {
        let StmtKind::Import(decl) = &stmt.kind else {
            continue;
        };
        if decl.target.is_some() {
            continue;
        }
        // A selective import introduces no namespace; key its interface by
        // the import path, as the package analysis does, so the resolve pass
        // finds its members (`importa ex "norma:order" fixum Orderable`).
        let binding = match &decl.kind {
            crate::syntax::ImportKind::Selective { .. } => Some(interner.resolve(decl.path).to_owned()),
            kind => import_binding(interner, kind),
        };
        let Some(mut binding) = binding else {
            continue;
        };
        let import_path = interner.resolve(decl.path);
        // Do not open targets `validate_import_paths` would later reject.
        if !import_contract_path_allowed(import_path, templates) {
            continue;
        }
        let target = cached_import_target(session, path, import_path, pack_key.as_ref(), resolved_pack, templates);
        // D7.7: a default-named import binds the target's declared `regio`
        // name; the driver rewrites its parse through `default_import_names`.
        if let ImportTargetLookup::Parsed(target) = &target
            && super::default_import::import_takes_default_name(decl)
            && let Some(region) = &target.region_name
        {
            binding.clone_from(region);
            contract
                .default_import_names
                .insert(import_path.to_owned(), region.clone());
        }
        let target = match target {
            ImportTargetLookup::Parsed(target) => target,
            ImportTargetLookup::Unparsable(target_path) => {
                // Fail-closed propagation (KPC-WIRE done_when (i)): record the
                // named import-site fact and leave the binding's surface
                // uninstalled, so an interfaceless namespace stops silently
                // matching. The gate fires only on parse failure with library
                // content — never on surfaceless-by-design namespaces.
                contract.import_site_failures.push(ImportSiteFailure {
                    binding: binding.clone(),
                    import_path: import_path.to_owned(),
                    target_path,
                });
                continue;
            }
            ImportTargetLookup::UnresolvedProvider(error) => {
                let provider =
                    provider_segments(import_path).map_or_else(String::new, |(provider, _)| provider.to_owned());
                contract.import_resolve_failures.push(ImportResolveFailure {
                    binding: binding.clone(),
                    import_path: import_path.to_owned(),
                    provider,
                    message: error.message.clone(),
                    cause_issue: error.issue,
                });
                continue;
            }
            ImportTargetLookup::Unresolved => continue,
        };
        contract
            .namespace_exports
            .insert(binding.clone(), target.exports);
        contract
            .namespace_declarations
            .insert(binding.clone(), target.declarations);
        contract
            .file_interfaces
            .insert(binding.clone(), target.interface.clone());
        // The seam is recomputed relative to the *current* consumer on the
        // retained target path, not through a fresh resolution: the second
        // call must serve the same contract even after the target is deleted.
        let target_path = &target.target_path;
        contract
            .namespace_seams
            .insert(binding.clone(), crate::import_resolve::same_package(session, path, target_path));
        // Carry the analyzed unit for this edge (ARCH-U4 / S01-3): the walk
        // is the one place the import-path spelling and the parsed target are
        // in hand together, and every file-interface binding it installs gets
        // its carried unit here — HIR emission maps over the view instead of
        // re-resolving providers from disk at emit time.
        if let Some(unit) = target.provider_unit.clone() {
            contract.provider_units.insert(import_path, unit);
        }
        if import_path.contains(':')
            && let Some(root) = crate::import_resolve::find_package_root_with_session(session, target_path)
        {
            contract.library_package_roots.insert(root);
        }
        // Seed the transitive same-package walk from every resolvable direct
        // import target (MRL-4, extended to package-form nested edges): a
        // module's public surface can name sibling nominals through its own
        // relative or package-form imports (`list<p.Punctum>` in a `Catena`
        // field, `capsule.Capsule` in an `admit_capsule` signature), and a
        // consumer that imports only the outer module cannot remap those
        // fields without the sibling's nominal registrations. Relative
        // targets seed with a derived identity only when their own edge
        // carries one; provider-form targets seed with the identity derived
        // from their provider spelling.
        if import_path.starts_with("./") || import_path.contains(':') {
            collect_transitive_same_package_interfaces(
                session,
                &target.target_path,
                target.interface.identity.as_ref(),
                pack_key.as_ref(),
                templates,
                &mut transitive_visited,
                &mut transitive_interfaces,
            );
        }
    }
    // Transitive sibling interfaces install under their producer-side
    // bindings; a consumer's own binding for the same name always wins, and
    // the extra namespaces stay inert (the consumer's HIR has no import
    // binding for them, mirroring the transitive library-leaf install).
    for (binding, interface) in transitive_interfaces {
        contract.file_interfaces.entry(binding).or_insert(interface);
    }
    contract
}

/// Collect file interfaces for the same-package imports of an imported
/// module, transitively (MRL-4).
///
/// Relative (`./sibling`), provider-form (`norma:fault`), and package-form
/// (bare source-root) edges are followed: the provider and package-form
/// edges belong to the imported module's own AST, so only the shared
/// traversal/absolute guards apply to them.
/// `visited` guards cycles (a module diamond or a genuine import cycle both
/// stop at the first visit). Interfaces are keyed by the producer-side
/// binding; the caller decides precedence against the consumer's own
/// bindings.
///
/// `from_identity` is the importing module's own canonical identity. Each
/// collected interface is re-keyed to the identity the importer's snapshot
/// context derives for the edge (`import_target_identity(edge,
/// from_identity)`) — the install and the snapshot must agree on the
/// canonical key or the consumer's identity lookup fails closed (WARN014
/// export skip).
fn collect_transitive_same_package_interfaces(
    session: &crate::driver::Session,
    from_file: &Path,
    from_identity: Option<&InterfaceLibraryIdentity>,
    pack_key: Option<&String>,
    templates: Option<&BTreeMap<String, String>>,
    visited: &mut BTreeSet<PathBuf>,
    out: &mut Vec<(String, FileInterface)>,
) {
    let Ok(source) = std::fs::read_to_string(from_file) else {
        return;
    };
    let pack = import_target_locale_pack(session, from_file, &source, None);
    let pack_ref = pack.as_ref();
    let Some((program, interner)) =
        parse_program_for_namespace_exports(&from_file.display().to_string(), &source, pack_ref)
    else {
        return;
    };
    for stmt in &program.statements {
        let StmtKind::Import(decl) = &stmt.kind else {
            continue;
        };
        if decl.target.is_some() {
            continue;
        }
        let import_path = interner.resolve(decl.path);
        let is_relative = import_path.starts_with("./");
        let is_provider_form = import_path
            .split_once(':')
            .is_some_and(|(provider, module)| !provider.is_empty() && !module.is_empty() && !module.contains(':'));
        let is_package_form = !is_relative && !is_provider_form && !import_path.starts_with('§');
        if !is_relative && !is_package_form && !is_provider_form {
            continue;
        }
        // Relative edges must match the consumer's own SEM006-validated
        // scheme set; provider and package-form edges belong to the imported
        // module's own AST, which the consumer's SEM006 pass never revalidates —
        // only the shared traversal/absolute guards apply there.
        let path_allowed = if is_relative {
            import_contract_path_allowed(import_path, templates)
        } else {
            import_path_shape_allowed(import_path)
        };
        if !path_allowed {
            continue;
        }
        let Some(binding) = import_binding(&interner, &decl.kind) else {
            continue;
        };
        // An unparsable transitive target contributes no interface here; the
        // module that directly imports it reports the named import-site
        // failure through its own contract walk.
        let target = if is_package_form {
            // Package-form edges do not resolve through
            // `resolve_namespace_import`; resolve against the source root and
            // re-derive the parse/snapshot through the equivalent relative
            // spelling (same candidate, same memo keys). The re-derivation
            // anchors on a synthetic file whose parent is the source root —
            // only `.parent()` of the anchor is consulted. The edge identity
            // is re-keyed below from the importer's own identity either way.
            crate::import_resolve::resolve_source_root_import(session, from_file, import_path).and_then(
                |(base, _resolved)| {
                    let anchor = base.join("anchor.fab");
                    match cached_import_target(
                        session,
                        &anchor,
                        &format!("./{import_path}"),
                        pack_key,
                        pack_ref,
                        templates,
                    ) {
                        ImportTargetLookup::Parsed(target) => Some(target),
                        ImportTargetLookup::Unparsable(_)
                        | ImportTargetLookup::Unresolved
                        | ImportTargetLookup::UnresolvedProvider(_) => None,
                    }
                },
            )
        } else {
            match cached_import_target(session, from_file, import_path, pack_key, pack_ref, templates) {
                ImportTargetLookup::Parsed(target) => Some(target),
                ImportTargetLookup::Unparsable(_)
                | ImportTargetLookup::Unresolved
                | ImportTargetLookup::UnresolvedProvider(_) => None,
            }
        };
        let Some(target) = target else {
            continue;
        };
        if !visited.insert(target.target_path.clone()) {
            continue;
        }
        // D7.7: the producer's own default-named import binds its target's
        // declared `regio` name, exactly as the producer's analysis does.
        let binding = match &target.region_name {
            Some(region) if super::default_import::import_takes_default_name(decl) => region.clone(),
            _ => binding,
        };
        let mut interface = target.interface.clone();
        if let Some(identity) = import_target_identity(import_path, from_identity) {
            interface.identity = Some(identity);
        }
        let child_identity = interface.identity.clone();
        out.push((binding, interface));
        collect_transitive_same_package_interfaces(
            session,
            &target.target_path,
            child_identity.as_ref(),
            pack_key,
            templates,
            visited,
            out,
        );
    }
}

/// Resolve the frontmatter `locale` reader pack when the caller supplied none.
///
/// Resolution uses `config`'s language-pack root and product loader, so the
/// session that will parse the source is also the one that resolves its
/// declared locale.
///
/// Returns `None` when the source declares no locale or the declared pack
/// cannot be loaded; the analysis driver surfaces the authoritative
/// diagnostic for a malformed locale during the real compile.
pub(crate) fn frontmatter_locale_pack_from_source(
    config: &crate::driver::Config,
    name: &str,
    source: &str,
) -> Option<crate::locale::LocalePack> {
    let peeled = crate::driver::peel_raw_source(name, source).ok()?;
    frontmatter_locale_pack_from_frontmatter(config, peeled.frontmatter.as_ref())
}

/// Resolve the frontmatter `locale` reader pack from already-peeled metadata.
///
/// Shared by the tool surfaces that hold a [`crate::driver::SourceFile`] (whose
/// `content` is already frontmatter-stripped) and by source-level callers that
/// re-peel first. Returns `None` when no locale is declared or the declared
/// pack cannot be loaded from `config`'s pack sources.
pub(crate) fn frontmatter_locale_pack_from_frontmatter(
    config: &crate::driver::Config,
    frontmatter: Option<&crate::driver::FileFrontmatter>,
) -> Option<crate::locale::LocalePack> {
    let locale_id = frontmatter?.locale_result()?.ok()?;
    crate::driver::resolve_frontmatter_locale_pack(config, locale_id).ok()
}

/// Resolve an explicit `--locale <name>` to its reader pack via stdlib.
///
/// Mirrors the frontmatter-locale resolver, but an explicit CLI locale that
/// cannot be resolved exits with the resolver's diagnostic instead of falling
/// back silently. Returns `None` when no locale was given.
/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
#[allow(clippy::result_large_err)]
pub fn explicit_locale_pack(locale: Option<&str>) -> Result<Option<crate::locale::LocalePack>, crate::Diagnostic> {
    let Some(locale) = locale.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let config = crate::driver::Config::default();
    match crate::driver::resolve_frontmatter_locale_pack(&config, locale) {
        Ok(pack) => Ok(Some(pack)),
        Err(diag) => Err(*diag),
    }
}

/// Resolve the reader pack a cross-package import target must be parsed under.
///
/// A provider module is lexed in its own reader surface, not the importing
/// module's. Resolution chain:
///
/// 1. The target's own `+++ locale = "…" +++` frontmatter.
/// 2. The target package's `faber.toml` locale (no frontmatter).
/// 3. The importing module's pack (loose same-locale files keep the session
///    `--locale` behavior).
/// 4. `None` — the built-in Latin surface.
pub(crate) fn import_target_locale_pack(
    session: &crate::driver::Session,
    target_path: &Path,
    target_source: &str,
    reader_pack: ReaderPackRef<'_>,
) -> Option<crate::locale::LocalePack> {
    frontmatter_locale_pack_from_source(&session.config, &target_path.display().to_string(), target_source)
        .or_else(|| package_reader_locale_pack(session, target_path))
        .or_else(|| reader_pack.cloned())
}

/// Resolve the `[reader] locale` reader pack declared by the package
/// `faber.toml` containing `path`, when present.
pub(crate) fn package_reader_locale_pack(
    session: &crate::driver::Session,
    path: &Path,
) -> Option<crate::locale::LocalePack> {
    let package_root = crate::import_resolve::find_package_root_with_session(session, path)?;
    let manifest_path = package_root.join("faber.toml");
    let manifest = std::fs::read_to_string(manifest_path).ok()?;
    let locale_id = reader_locale_from_manifest(&manifest)?;
    let config = crate::driver::Config::default();
    crate::driver::resolve_frontmatter_locale_pack(&config, &locale_id).ok()
}

/// Parse the `locale = "…"` value from a `faber.toml` manifest source.
///
/// Accepts the canonical `[locale]` table and the legacy `[reader]` alias. Returns
/// `None` when the manifest has neither table or no `locale` key.
#[must_use]
fn reader_locale_from_manifest(toml_source: &str) -> Option<String> {
    let manifest = toml::from_str::<toml::Value>(toml_source).ok()?;
    ["locale", "reader"]
        .iter()
        .find_map(|table| manifest.get(table)?.get("locale")?.as_str())
        .filter(|locale| !locale.is_empty())
        .map(str::to_owned)
}

/// Package identity facts for a source file path, regardless of local imports.
///
/// Package harnesses compiling a sibling LIBRARY unit (a file with no local
/// imports of its own, e.g. `auxilium.fab`) need the product + module segments
/// to record the canonical identity on the unit's own exported functions so
/// the emitted module defines the external symbol its consumers call.
///
/// The file is re-parsed under the caller-resolved reader pack, with the
/// source's frontmatter still able to select a pack when the caller did not
/// make an exact choice. This mirrors the pack used by the package graph, so
/// local imports register as same-package identities instead of falling
/// through to a hidden product default.
#[must_use]
pub fn package_identity_facts_for_path(
    path: &Path,
    config: &crate::driver::Config,
) -> Option<crate::driver::PackageImportIdentities> {
    // Session-less entry (exempla harness, tests): one call is its own driver
    // run, so it mints a fresh session for the memo scope. The compile path
    // threads the driver's own session through
    // `package_identity_facts_for_parsed_file` instead.
    let session = crate::driver::Session::new(config.clone());
    let source = std::fs::read_to_string(path).ok()?;
    let code_pack = package_effective_reader_pack(path, &source, config);
    package_identity_facts_for_file(&session, path, &source, code_pack.as_ref(), None)
}

fn package_effective_reader_pack(
    path: &Path,
    source: &str,
    config: &crate::driver::Config,
) -> Option<crate::locale::LocalePack> {
    let peeled = crate::driver::peel_raw_source(&path.display().to_string(), source).ok()?;
    let session = crate::driver::Session::new(config.clone());
    let mut diagnostics = Vec::new();
    crate::driver::resolve_peeled_lex_pack(&session, &peeled, &mut diagnostics)
        .ok()
        .flatten()
}

/// Compute package-local import identity facts for one compiled file.
///
/// Local sibling imports are the `./rel` form only: `norma:*` and other
/// provider-module imports are library identities, not user-code cross-module
/// calls.
fn package_identity_facts_for_file(
    session: &crate::driver::Session,
    path: &Path,
    source: &str,
    code_pack: ReaderPackRef<'_>,
    templates: Option<&BTreeMap<String, String>>,
) -> Option<crate::driver::PackageImportIdentities> {
    let path = absolute_cli_path(path);
    let (program, interner) = parse_program_for_namespace_exports(&path.display().to_string(), source, code_pack)?;
    package_identity_facts_for_parsed_file(session, &path, &program, &interner, code_pack, templates)
}

fn package_identity_facts_for_parsed_file(
    session: &crate::driver::Session,
    path: &Path,
    program: &Program,
    interner: &crate::lexer::Interner,
    code_pack: ReaderPackRef<'_>,
    templates: Option<&BTreeMap<String, String>>,
) -> Option<crate::driver::PackageImportIdentities> {
    let path = absolute_cli_path(path);
    let package_root = match crate::import_resolve::find_package_root_with_session(session, &path) {
        Some(root) => root,
        None => path.parent()?.to_path_buf(),
    };
    let product = package_root
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())?;
    let module_segments = module_segments_relative_to(&package_root, &path);
    let mut imports = BTreeMap::new();
    let mut device_interfaces = BTreeMap::new();
    for stmt in &program.statements {
        let StmtKind::Import(decl) = &stmt.kind else {
            continue;
        };
        if decl.target.is_some() {
            continue;
        }
        let Some(binding) = import_binding(interner, &decl.kind) else {
            continue;
        };
        let import_path = interner.resolve(decl.path);
        if !import_path.starts_with("./") {
            continue;
        }
        if !import_contract_path_allowed(import_path, templates) {
            continue;
        }
        let Ok(target) = crate::import_resolve::resolve_namespace_import(session, &path, import_path, templates) else {
            continue;
        };
        // DFV2-3: record the target's device-bearing interface for this
        // consumer so the MIR provider route can carry role + body channel
        // into the consumer link. The record rides the unit's own
        // `PackageImportIdentities` — never a process-wide registry — so the
        // route sees the same interface the resolver installed and can only
        // ever serve this unit. Served through the import-target memo, so an
        // unparsable target records no route — its contract walk already
        // failed closed with the named import-site diagnostic.
        let pack_key = code_pack.map(|pack| pack.metadata.id.clone());
        if let ImportTargetLookup::Parsed(linked) =
            cached_import_target(session, &path, import_path, pack_key.as_ref(), code_pack, templates)
            && interface_bears_device(&linked.interface)
        {
            device_interfaces.insert(binding.clone(), linked.interface.clone());
        }
        imports.insert(binding.clone(), module_segments_relative_to(&package_root, &target));
    }
    Some(crate::driver::PackageImportIdentities { product, module_segments, imports, device_interfaces })
}

/// Normalize a CLI path against the process CWD so package-root discovery from
/// a relative input (`importa.fab`, parent = "") uses the real directory.
fn absolute_cli_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().map_or_else(|_| path.to_path_buf(), |cwd| cwd.join(path))
    }
}

/// Canonical module segments for a file under a source root — mirror of the
/// package machinery's `module_segments_for_file` (frontmatter `group`, else
/// source-root-relative path with `.fab`/`main.fab`/`mod.fab` normalization).
fn module_segments_relative_to(root: &Path, file: &Path) -> Vec<String> {
    let relative = file.strip_prefix(root).unwrap_or(file);
    let mut parts: Vec<String> = relative
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    if let Some(last) = parts.last_mut() {
        if last == "main.fab" || last == "mod.fab" {
            parts.pop();
        } else if let Some(stripped) = last.strip_suffix(".fab") {
            *last = stripped.to_string();
        } else if let Some(stripped) = last.strip_suffix(".proba") {
            *last = format!("{stripped}_proba");
        }
    }
    parts
        .into_iter()
        .map(|segment| sanitize_module_segment(&segment))
        .collect()
}

/// Map a filesystem path segment to a valid module identifier — mirror of the
/// package machinery's `sanitize_rust_module_ident` (ASCII alphanumeric +
/// `_` kept, everything else `_`, leading digit prefixed, empty → `m`).
fn sanitize_module_segment(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len().max(1));
    for c in segment.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    let trimmed = out.trim_matches('_');
    let mut ident = if trimmed.is_empty() {
        "m".to_owned()
    } else {
        trimmed.to_owned()
    };
    if ident.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        ident = format!("m_{ident}");
    }
    ident
}

/// Snapshot context for one import target's export interface.
///
/// `module_identity` is the target module's own canonical identity (derived
/// from its `provider:module` import path), and `bindings` maps each import
/// binding the module declares to its target module's identity. Nominal
/// references in exported signatures resolve through these identities at the
/// consumer: local names bind to the module's own identity, qualified names
/// (`visus.Visus`) bind to the qualifier target's identity — no short-name
/// ambiguity when multiple namespaces export the same name, and no WARN014
/// skip when the referenced sibling is itself imported by the consumer.
#[derive(Default, Clone)]
struct SnapshotContext {
    module_identity: Option<InterfaceLibraryIdentity>,
    bindings: BTreeMap<String, InterfaceLibraryIdentity>,
}

impl SnapshotContext {
    fn for_program(
        program: &Program,
        interner: &crate::lexer::Interner,
        module_identity: Option<&InterfaceLibraryIdentity>,
    ) -> Self {
        let mut ctx = SnapshotContext { module_identity: module_identity.cloned(), bindings: BTreeMap::new() };
        for stmt in &program.statements {
            let StmtKind::Import(decl) = &stmt.kind else {
                continue;
            };
            if decl.target.is_some() {
                continue;
            }
            let Some(binding) = import_binding(interner, &decl.kind) else {
                continue;
            };
            let import_path = interner.resolve(decl.path);
            if let Some(identity) = import_target_identity(import_path, ctx.module_identity.as_ref()) {
                ctx.bindings.insert(binding, identity);
            }
        }
        ctx
    }
}

/// Derive a module's canonical identity from its `provider:module` import
/// path. Relative (`./sibling`) and `§template` imports have no derivable
/// provider identity; those snapshots stay on the legacy name-keyed path.
fn import_target_identity(
    import_path: &str,
    module_identity: Option<&InterfaceLibraryIdentity>,
) -> Option<InterfaceLibraryIdentity> {
    if let Some((provider, module)) = import_path.split_once(':')
        && !provider.is_empty()
        && !module.is_empty()
        && !module.contains(':')
    {
        let module_path = module.split('/').map(str::to_owned).collect();
        return Some(InterfaceLibraryIdentity { provider: provider.to_owned(), package: None, module_path });
    }
    if let Some(relative) = import_path.strip_prefix("./") {
        // Relative sibling import: shares the exporting module's provider and
        // package, extending its module path by the relative segments.
        let identity = module_identity?;
        let mut module_path = identity.module_path.clone();
        module_path.extend(
            relative
                .split('/')
                .filter(|segment| !segment.is_empty() && *segment != ".")
                .map(str::to_owned),
        );
        return Some(InterfaceLibraryIdentity {
            provider: identity.provider.clone(),
            package: identity.package.clone(),
            module_path,
        });
    }
    // Package-form (source-root) import: bare spelling — no provider colon,
    // no `./` prefix, no `§` template. The target lives in the same package
    // as the importing module, so the identity shares the module's provider
    // and package, with the module path spelled from the source root. This
    // is the same canonical key the provider-form spelling of the same
    // target derives; leaving it None here makes package-form snapshots
    // fail closed at the consumer's canonical lookup (WARN014 export skip).
    if !import_path.starts_with('§') {
        let identity = module_identity?;
        let module_path: Vec<String> = import_path
            .split('/')
            .filter(|segment| !segment.is_empty() && *segment != ".")
            .map(str::to_owned)
            .collect();
        if module_path.is_empty() {
            return None;
        }
        return Some(InterfaceLibraryIdentity {
            provider: identity.provider.clone(),
            package: identity.package.clone(),
            module_path,
        });
    }
    None
}

fn file_interface_for_program(
    program: &Program,
    interner: &crate::lexer::Interner,
    module_identity: Option<&InterfaceLibraryIdentity>,
) -> FileInterface {
    let mut interface = FileInterface::new();
    if let Some(identity) = module_identity {
        interface = interface.with_identity(identity.clone());
    }
    let nominal_kinds = nominal_kinds_for_program(program, interner);
    let ctx = SnapshotContext::for_program(program, interner, module_identity);
    for stmt in &program.statements {
        // Keep the legacy unmarked-exported surface for interface snapshots
        // (mirror of the package path's `program_export_surface`): a public
        // callable's signature may reference an unmarked nominal of the same
        // module, and dropping that nominal from the interface poisons the
        // callable's import (WARN014 skip → false SEM010s in consumers).
        // Explicit `@ privata` declarations stay excluded.
        if crate::semantic::passes::visibility_annotation::has_private_visibility_annotation(stmt, interner) {
            continue;
        }
        // Skip constructs we cannot snapshot yet; do not drop the whole interface.
        let Some(export) = file_export_from_stmt(stmt, interner, &nominal_kinds, &ctx) else {
            continue;
        };
        if matches!(stmt.kind, StmtKind::Func(_)) {
            let gates = unavailable_gates_from_annotations(&stmt.annotations, interner);
            if !gates.is_empty() {
                interface.unavailable.insert(export.name.clone(), gates);
            }
        }
        interface.insert(export);
    }
    interface
}

/// The `@ nondum [target] ["reason"]` gates written on one declaration, in
/// sugar or braced spelling, as portable interface records.
fn unavailable_gates_from_annotations(
    annotations: &[Annotation],
    interner: &crate::lexer::Interner,
) -> Vec<crate::file_interface::InterfaceUnavailable> {
    annotations
        .iter()
        .filter_map(|annotation| {
            let view = match &annotation.kind {
                AnnotationKind::Statement(stmt) => crate::syntax::nondum_from_stmt(stmt, interner),
                AnnotationKind::Braced(braced) => crate::syntax::braced_nondum_view(braced, interner),
                _ => None,
            }?;
            Some(crate::file_interface::InterfaceUnavailable {
                target: view
                    .target
                    .map(|symbol| interner.resolve(symbol).to_owned()),
                reason: view.ratio.map(|symbol| interner.resolve(symbol).to_owned()),
            })
        })
        .collect()
}

fn peeled_source_body(name: &str, source: &str) -> Option<String> {
    crate::driver::peel_raw_source(name, source)
        .ok()
        .map(|peeled| peeled.body.to_owned())
}

fn nominal_kinds_for_program(
    program: &Program,
    interner: &crate::lexer::Interner,
) -> BTreeMap<String, InterfaceNominalKind> {
    let mut kinds = BTreeMap::new();
    for stmt in &program.statements {
        match &stmt.kind {
            StmtKind::Class(decl) => {
                kinds.insert(interner.resolve(decl.name.name).to_owned(), InterfaceNominalKind::Struct);
            }
            StmtKind::Enum(decl) => {
                kinds.insert(interner.resolve(decl.name.name).to_owned(), InterfaceNominalKind::Enum);
            }
            StmtKind::Union(decl) => {
                kinds.insert(interner.resolve(decl.name.name).to_owned(), InterfaceNominalKind::Enum);
            }
            StmtKind::Interface(decl) => {
                kinds.insert(interner.resolve(decl.name.name).to_owned(), InterfaceNominalKind::Interface);
            }
            _ => {}
        }
    }
    kinds
}

fn file_export_from_stmt(
    stmt: &Stmt,
    interner: &crate::lexer::Interner,
    nominal_kinds: &BTreeMap<String, InterfaceNominalKind>,
    ctx: &SnapshotContext,
) -> Option<FileExport> {
    match &stmt.kind {
        StmtKind::Func(decl) => func_export_from_stmt(stmt, decl, interner, nominal_kinds, ctx),
        StmtKind::Class(decl) => class_export_from_stmt(stmt, decl, interner, nominal_kinds, ctx),
        StmtKind::Enum(decl) => enum_export_from_stmt(decl, interner, ctx),
        StmtKind::Union(decl) => union_export_from_stmt(decl, interner, nominal_kinds, ctx),
        StmtKind::Interface(decl) => interface_export_from_stmt(stmt, decl, interner, nominal_kinds, ctx),
        StmtKind::TypeAlias(decl) => type_alias_export_from_stmt(decl, interner, nominal_kinds, ctx),
        StmtKind::Var(var) => var_export_from_stmt(var, interner, nominal_kinds, ctx),
        _ => None,
    }
}

fn func_export_from_stmt(
    stmt: &Stmt,
    decl: &crate::syntax::FuncDecl,
    interner: &crate::lexer::Interner,
    nominal_kinds: &BTreeMap<String, InterfaceNominalKind>,
    ctx: &SnapshotContext,
) -> Option<FileExport> {
    let name = interner.resolve(decl.name.name).to_owned();
    let type_param_names: BTreeSet<&str> = decl
        .type_params
        .iter()
        .map(|tp| interner.resolve(tp.name.name))
        .collect();
    let callable_type_params =
        interface_callable_params(&decl.type_params, interner, &type_param_names, nominal_kinds, ctx)?;
    let mut params = Vec::new();
    for param in &decl.params {
        params.push(InterfaceParam {
            ty: interface_type_from_type_expr(&param.ty, interner, &type_param_names, nominal_kinds, ctx)?,
            mode: semantic_param_mode(param.mode),
            optional: param.sponte || param.default.is_some(),
            sponte: param.sponte,
            axis: param.mode == AstParamMode::Axis,
        });
    }
    let ret = match &decl.ret {
        Some(ret) => interface_type_from_type_expr(ret, interner, &type_param_names, nominal_kinds, ctx)?,
        None => InterfaceTypeSnapshot::Primitive(Primitive::Vacuum),
    };
    let err = match &decl.err {
        Some(err_ty) => Some(Box::new(interface_type_from_type_expr(
            err_ty,
            interner,
            &type_param_names,
            nominal_kinds,
            ctx,
        )?)),
        None => None,
    };
    let is_async =
        decl.posture.is_async() || annotations_include_callable_family(&stmt.annotations, interner, "futura");
    let is_generator =
        decl.posture.is_stream() || annotations_include_callable_family(&stmt.annotations, interner, "cursor");
    let device = device_contract_from_stmt(stmt, decl, interner);
    Some(FileExport {
        name,
        kind: FileExportKind::Function(InterfaceCallable {
            type_params: callable_type_params,
            params,
            ret: Box::new(ret),
            err,
            is_async,
            is_generator,
            tensor_edge_policy_requirements: Vec::new(),
            device,
        }),
    })
}

fn class_export_from_stmt(
    stmt: &Stmt,
    decl: &ClassDecl,
    interner: &crate::lexer::Interner,
    nominal_kinds: &BTreeMap<String, InterfaceNominalKind>,
    ctx: &SnapshotContext,
) -> Option<FileExport> {
    let name = interner.resolve(decl.name.name).to_owned();
    let no_params = BTreeSet::new();
    let fields = decl
        .members
        .iter()
        .filter_map(|member| {
            let ClassMemberKind::Field(field) = &member.kind else {
                return None;
            };
            Some(InterfaceStructField {
                name: interner.resolve(field.name.name).to_owned(),
                ty: interface_type_from_type_expr(&field.ty, interner, &no_params, nominal_kinds, ctx)?,
                optional: field.sponte,
                required: !field.sponte && field.init.is_none(),
                default: None,
                modifier: field.modifier,
                visibility: member_visibility(&member.annotations, interner),
            })
        })
        .collect::<Vec<_>>();
    let methods = decl
        .members
        .iter()
        .filter_map(|member| {
            let ClassMemberKind::Method(method) = &member.kind else {
                return None;
            };
            let type_param_names: BTreeSet<&str> = method
                .type_params
                .iter()
                .map(|tp| interner.resolve(tp.name.name))
                .collect();
            let callable_type_params =
                interface_callable_params(&method.type_params, interner, &type_param_names, nominal_kinds, ctx)?;
            let mut params = Vec::new();
            for param in &method.params {
                params.push(InterfaceParam {
                    ty: interface_type_from_type_expr(&param.ty, interner, &type_param_names, nominal_kinds, ctx)?,
                    mode: semantic_param_mode(param.mode),
                    optional: param.sponte || param.default.is_some(),
                    sponte: param.sponte,
                    axis: param.mode == AstParamMode::Axis,
                });
            }
            let ret = match &method.ret {
                Some(ret) => interface_type_from_type_expr(ret, interner, &type_param_names, nominal_kinds, ctx)?,
                None => InterfaceTypeSnapshot::Primitive(Primitive::Vacuum),
            };
            let err = match &method.err {
                Some(err_ty) => Some(Box::new(interface_type_from_type_expr(
                    err_ty,
                    interner,
                    &type_param_names,
                    nominal_kinds,
                    ctx,
                )?)),
                None => None,
            };
            let is_async = method.posture.is_async()
                || annotations_include_callable_family(&member.annotations, interner, "futura");
            let is_generator = method.posture.is_stream()
                || annotations_include_callable_family(&member.annotations, interner, "cursor");
            Some(InterfaceMethodExport {
                name: interner.resolve(method.name.name).to_owned(),
                callable: InterfaceCallable {
                    type_params: callable_type_params,
                    params,
                    ret: Box::new(ret),
                    err,
                    is_async,
                    is_generator,
                    tensor_edge_policy_requirements: Vec::new(),
                    device: None,
                },
                visibility: match member_visibility(&member.annotations, interner) {
                    crate::syntax::MemberVisibility::Publica => member_visibility(&method.annotations, interner),
                    visibility => visibility,
                },
                unavailable: unavailable_gates_from_annotations(&member.annotations, interner),
            })
        })
        .collect::<Vec<_>>();
    let annotation_contract = annotation_contract_from_stmt(stmt, decl, interner);
    let class_type_params: BTreeSet<&str> = decl
        .type_params
        .iter()
        .map(|tp| interner.resolve(tp.name.name))
        .collect();
    // A contract with no portable form is dropped: the importer then refuses
    // the genus for that slot (fail closed) instead of guessing.
    let implements = decl
        .implements
        .iter()
        .filter_map(|contract| contract_ref_snapshot(contract, interner, &class_type_params, nominal_kinds, ctx))
        .collect();
    Some(FileExport {
        name: name.clone(),
        kind: FileExportKind::Struct(InterfaceStructExport {
            is_json: annotations_include_callable_family(&stmt.annotations, interner, "json"),
            implements,
            name,
            params: interface_generic_params(&decl.type_params, interner),
            fields,
            methods,
            annotation_contract,
        }),
    })
}

/// Portable form of one `implements` contract: a nominal, applied to its
/// arguments when the contract is generic.
fn contract_ref_snapshot(
    contract: &radix_syntax::ContractRefExpr,
    interner: &crate::lexer::Interner,
    type_params: &BTreeSet<&str>,
    nominal_kinds: &BTreeMap<String, InterfaceNominalKind>,
    ctx: &SnapshotContext,
) -> Option<InterfaceTypeSnapshot> {
    let base = nominal_snapshot(
        interner.resolve(contract.name.name),
        nominal_kinds,
        ctx.module_identity.as_ref(),
    );
    if contract.args.is_empty() {
        return Some(base);
    }
    let args = contract
        .args
        .iter()
        .map(|arg| {
            interface_type_from_type_expr(arg, interner, type_params, nominal_kinds, ctx)
                .map(crate::file_interface::InterfaceAppliedArg::Type)
        })
        .collect::<Option<Vec<_>>>()?;
    Some(InterfaceTypeSnapshot::Applied(Box::new(base), args))
}

fn enum_export_from_stmt(
    decl: &crate::syntax::EnumDecl,
    interner: &crate::lexer::Interner,
    ctx: &SnapshotContext,
) -> Option<FileExport> {
    let name = interner.resolve(decl.name.name).to_owned();
    // `ordo` members are unit variants: project their names so
    // consumers can reference `module.MEMBER` values and match them.
    // Only identity-bearing snapshots project; the identity-less
    // relative-import surface keeps the legacy empty-variant
    // fail-closed behavior.
    let variants = if ctx.module_identity.is_some() {
        decl.members
            .iter()
            .map(|member| InterfaceEnumVariant {
                name: interner.resolve(member.name.name).to_owned(),
                fields: Vec::new(),
            })
            .collect()
    } else {
        Vec::new()
    };
    Some(FileExport {
        name: name.clone(),
        kind: FileExportKind::Enum(InterfaceNominalExport {
            name,
            // AST `discretio` declarations carry no generic parameters.
            params: Vec::new(),
            methods: Vec::new(),
            variants,
            shared_fields: Vec::new(),
            radix_contract: None,
        }),
    })
}

fn union_export_from_stmt(
    decl: &crate::syntax::UnionDecl,
    interner: &crate::lexer::Interner,
    nominal_kinds: &BTreeMap<String, InterfaceNominalKind>,
    ctx: &SnapshotContext,
) -> Option<FileExport> {
    let name = interner.resolve(decl.name.name).to_owned();
    // Mirror the package path's enum snapshot: `discretio` variants
    // ride the file interface (name + payload fields) so imported
    // union patterns (`module.VARIANT` in `discerne` arms) resolve
    // instead of failing SEM001. Identity-bearing snapshots only —
    // identity-less relative-import targets keep the legacy
    // empty-variant surface the error-masking
    // diagnostics pin (SEM001.unknown_variant / SEM008 on imported
    // union refs).
    let (variants, shared_fields) = if ctx.module_identity.is_some() {
        let type_param_names: BTreeSet<&str> = decl
            .type_params
            .iter()
            .map(|tp| interner.resolve(tp.name.name))
            .collect();
        let variants = decl
            .variants
            .iter()
            .map(|variant| {
                let fields = variant
                    .fields
                    .iter()
                    .filter_map(|field| {
                        Some(InterfaceStructField {
                            name: interner.resolve(field.name.name).to_owned(),
                            ty: interface_type_from_type_expr(
                                &field.ty,
                                interner,
                                &type_param_names,
                                nominal_kinds,
                                ctx,
                            )?,
                            optional: false,
                            required: true,
                            default: None,
                            modifier: None,
                            visibility: crate::syntax::MemberVisibility::Publica,
                        })
                    })
                    .collect::<Vec<_>>();
                InterfaceEnumVariant { name: interner.resolve(variant.name.name).to_owned(), fields }
            })
            .collect::<Vec<_>>();
        let shared_fields = decl
            .shared_fields
            .iter()
            .filter_map(|shared| {
                Some(InterfaceStructField {
                    name: interner.resolve(shared.field.name.name).to_owned(),
                    ty: interface_type_from_type_expr(
                        &shared.field.ty,
                        interner,
                        &type_param_names,
                        nominal_kinds,
                        ctx,
                    )?,
                    optional: false,
                    required: true,
                    default: None,
                    modifier: None,
                    visibility: crate::syntax::MemberVisibility::Publica,
                })
            })
            .collect::<Vec<_>>();
        (variants, shared_fields)
    } else {
        (Vec::new(), Vec::new())
    };
    Some(FileExport {
        name: name.clone(),
        kind: FileExportKind::Enum(InterfaceNominalExport {
            name,
            params: interface_generic_params(&decl.type_params, interner),
            methods: Vec::new(),
            variants,
            shared_fields,
            radix_contract: None,
        }),
    })
}

fn interface_export_from_stmt(
    stmt: &Stmt,
    decl: &crate::syntax::InterfaceDecl,
    interner: &crate::lexer::Interner,
    nominal_kinds: &BTreeMap<String, InterfaceNominalKind>,
    ctx: &SnapshotContext,
) -> Option<FileExport> {
    let name = interner.resolve(decl.name.name).to_owned();
    // Method signatures ride the interface so importers can implement
    // the contract and call it through a bound (D1.1/D1.2); the
    // contract's own parameters stay symbolic.
    let type_param_names: BTreeSet<&str> = decl
        .type_params
        .iter()
        .map(|tp| interner.resolve(tp.name.name))
        .collect();
    let methods = decl
        .methods
        .iter()
        .filter_map(|method| interface_method_from_ast(method, interner, &type_param_names, nominal_kinds, ctx))
        .collect();
    let radix_contract = stmt.annotations.iter().find_map(|annotation| {
        crate::semantic::radix_contract::role_from_ast(annotation, interner)
            .and_then(Result::ok)
            .map(|role| role.as_str().to_owned())
    });
    Some(FileExport {
        name: name.clone(),
        kind: FileExportKind::Interface(InterfaceNominalExport {
            name,
            params: interface_generic_params(&decl.type_params, interner),
            methods,
            variants: Vec::new(),
            shared_fields: Vec::new(),
            radix_contract,
        }),
    })
}

fn type_alias_export_from_stmt(
    decl: &crate::syntax::TypeAliasDecl,
    interner: &crate::lexer::Interner,
    nominal_kinds: &BTreeMap<String, InterfaceNominalKind>,
    ctx: &SnapshotContext,
) -> Option<FileExport> {
    let name = interner.resolve(decl.name.name).to_owned();
    let params = interface_generic_params(&decl.type_params, interner);
    let no_params = BTreeSet::new();
    Some(FileExport {
        name: name.clone(),
        kind: FileExportKind::TypeAlias(InterfaceAliasExport {
            name,
            params,
            body: interface_type_from_type_expr(&decl.ty, interner, &no_params, nominal_kinds, ctx)?,
        }),
    })
}

fn var_export_from_stmt(
    var: &crate::syntax::VarDecl,
    interner: &crate::lexer::Interner,
    nominal_kinds: &BTreeMap<String, InterfaceNominalKind>,
    ctx: &SnapshotContext,
) -> Option<FileExport> {
    // Typed `fixum` consts ride the interface as value exports
    // (`module.CONST`), mirroring the package path's const library
    // items. Untyped or mutable declarations are left out.
    if var.mutability != crate::syntax::AstMutability::Immutable {
        return None;
    }
    let crate::syntax::BindingPattern::Ident(ident) = &var.binding else {
        return None;
    };
    let name = interner.resolve(ident.name).to_owned();
    let no_params = BTreeSet::new();
    Some(FileExport {
        name,
        kind: FileExportKind::Const(
            interface_type_from_type_expr(var.ty.as_ref()?, interner, &no_params, nominal_kinds, ctx)?,
            None,
        ),
    })
}

fn semantic_param_mode(mode: AstParamMode) -> SemanticParamMode {
    match mode {
        AstParamMode::Owned | AstParamMode::Axis => SemanticParamMode::Owned,
        AstParamMode::De => SemanticParamMode::Ref,
        AstParamMode::In => SemanticParamMode::MutRef,
        AstParamMode::Ex => SemanticParamMode::Move,
    }
}

fn interface_generic_params(
    params: &[crate::syntax::TypeParam],
    interner: &crate::lexer::Interner,
) -> Vec<InterfaceGenericParam> {
    params
        .iter()
        .map(|param| InterfaceGenericParam { name: interner.resolve(param.name.name).to_owned(), kind: param.kind })
        .collect()
}

/// Callable-side descriptor row for the AST export snapshot.
///
/// Stage 1 never carried constraints, so they stay `Any`; defaults snapshot
/// through the same type-expression walker as parameter types.
fn interface_callable_params(
    params: &[crate::syntax::TypeParam],
    interner: &crate::lexer::Interner,
    type_param_names: &BTreeSet<&str>,
    nominal_kinds: &BTreeMap<String, InterfaceNominalKind>,
    ctx: &SnapshotContext,
) -> Option<Vec<InterfaceCallableParam>> {
    params
        .iter()
        .map(|param| {
            Some(InterfaceCallableParam {
                name: interner.resolve(param.name.name).to_owned(),
                kind: param.kind,
                constraint: InterfaceTypeParamConstraint::Any,
                default: match &param.default {
                    Some(crate::syntax::GenericParamDefault::Typus(expr)) => {
                        Some(InterfaceGenericParamDefault::Typus(Box::new(interface_type_from_type_expr(
                            expr,
                            interner,
                            type_param_names,
                            nominal_kinds,
                            ctx,
                        )?)))
                    }
                    Some(crate::syntax::GenericParamDefault::Magnitudo { value, .. }) => {
                        Some(InterfaceGenericParamDefault::Magnitudo(*value))
                    }
                    None => None,
                },
            })
        })
        .collect()
}

fn interface_type_from_generic_arg(
    arg: &radix_syntax::GenericArgExpr,
    interner: &crate::lexer::Interner,
    type_params: &BTreeSet<&str>,
    nominal_kinds: &BTreeMap<String, InterfaceNominalKind>,
    ctx: &SnapshotContext,
) -> Option<InterfaceTypeSnapshot> {
    match arg {
        radix_syntax::GenericArgExpr::Type(ty) => {
            interface_type_from_type_expr(ty, interner, type_params, nominal_kinds, ctx)
        }
        radix_syntax::GenericArgExpr::Name(name) => {
            let ty = TypeExpr {
                nullable: false,
                mode: None,
                kind: TypeExprKind::Named(name.clone(), Vec::new()),
                span: name.span,
                form: radix_syntax::TypeFormLevel::LONG,
                iuncta_label: None,
            };
            interface_type_from_type_expr(&ty, interner, type_params, nominal_kinds, ctx)
        }
        radix_syntax::GenericArgExpr::Size(_) | radix_syntax::GenericArgExpr::Range(_) => None,
    }
}

fn generic_arg_bare_name(arg: &radix_syntax::GenericArgExpr) -> Option<radix_syntax::Ident> {
    match arg {
        radix_syntax::GenericArgExpr::Name(name) => Some(name.clone()),
        radix_syntax::GenericArgExpr::Type(ty) => match &ty.kind {
            TypeExprKind::Named(name, params) if params.is_empty() => Some(name.clone()),
            _ => None,
        },
        radix_syntax::GenericArgExpr::Size(_) | radix_syntax::GenericArgExpr::Range(_) => None,
    }
}

fn interface_type_from_type_expr(
    ty: &TypeExpr,
    interner: &crate::lexer::Interner,
    type_params: &BTreeSet<&str>,
    nominal_kinds: &BTreeMap<String, InterfaceNominalKind>,
    ctx: &SnapshotContext,
) -> Option<InterfaceTypeSnapshot> {
    match &ty.kind {
        TypeExprKind::Named(name, params) => {
            let base = interner.resolve(name.name);
            if params.is_empty() {
                // Callable type parameters must be Param, not guessed nominals.
                if type_params.contains(base) {
                    return Some(InterfaceTypeSnapshot::Param(base.to_owned()));
                }
                return match base {
                    "ascii" => Some(InterfaceTypeSnapshot::Primitive(Primitive::Ascii)),
                    "textus" => Some(InterfaceTypeSnapshot::Primitive(Primitive::Textus)),
                    "numerus" => Some(InterfaceTypeSnapshot::Primitive(Primitive::Numerus)),
                    "fractus" => Some(InterfaceTypeSnapshot::Primitive(Primitive::Fractus)),
                    "bivalens" => Some(InterfaceTypeSnapshot::Primitive(Primitive::Bivalens)),
                    "nihil" => Some(InterfaceTypeSnapshot::Primitive(Primitive::Nihil)),
                    "vacuum" => Some(InterfaceTypeSnapshot::Primitive(Primitive::Vacuum)),
                    // Formal language type (faber::Json); not a guessed genus shell.
                    "json" => Some(InterfaceTypeSnapshot::Primitive(Primitive::Json)),
                    "valor" => Some(InterfaceTypeSnapshot::Primitive(Primitive::Valor)),
                    "instans" => Some(InterfaceTypeSnapshot::Primitive(Primitive::Instans)),
                    // Raw-bits markers `bN` (ruling d477c76d): locale-neutral
                    // like `u8`, so `lista<b8>` (the byte list)
                    // snapshots its element as Bits, never a guessed genus.
                    "b8" => Some(InterfaceTypeSnapshot::Bits(BitsWidth::B8)),
                    "b16" => Some(InterfaceTypeSnapshot::Bits(BitsWidth::B16)),
                    "b32" => Some(InterfaceTypeSnapshot::Bits(BitsWidth::B32)),
                    "b64" => Some(InterfaceTypeSnapshot::Bits(BitsWidth::B64)),
                    // Width sugar used in numeric modules.
                    "i8" => Some(InterfaceTypeSnapshot::SizedNumeric(Primitive::Numerus, NumericWidth::I8)),
                    "i16" => Some(InterfaceTypeSnapshot::SizedNumeric(Primitive::Numerus, NumericWidth::I16)),
                    "i32" => Some(InterfaceTypeSnapshot::SizedNumeric(Primitive::Numerus, NumericWidth::I32)),
                    "i64" => Some(InterfaceTypeSnapshot::SizedNumeric(Primitive::Numerus, NumericWidth::I64)),
                    "u8" => Some(InterfaceTypeSnapshot::SizedNumeric(Primitive::Numerus, NumericWidth::U8)),
                    "u16" => Some(InterfaceTypeSnapshot::SizedNumeric(Primitive::Numerus, NumericWidth::U16)),
                    "u32" => Some(InterfaceTypeSnapshot::SizedNumeric(Primitive::Numerus, NumericWidth::U32)),
                    "u64" => Some(InterfaceTypeSnapshot::SizedNumeric(Primitive::Numerus, NumericWidth::U64)),
                    "f16" => Some(InterfaceTypeSnapshot::SizedNumeric(Primitive::Fractus, NumericWidth::F16)),
                    "f32" => Some(InterfaceTypeSnapshot::SizedNumeric(Primitive::Fractus, NumericWidth::F32)),
                    "f64" => Some(InterfaceTypeSnapshot::SizedNumeric(Primitive::Fractus, NumericWidth::F64)),
                    // The unbounded integer is a bare width marker, never a nominal.
                    "inf" => Some(InterfaceTypeSnapshot::SizedNumeric(Primitive::Numerus, NumericWidth::Inf)),
                    name => Some(nominal_snapshot(name, nominal_kinds, ctx.module_identity.as_ref())),
                };
            }
            if params.len() == 1 && base == "lista" {
                let elem = interface_type_from_generic_arg(&params[0], interner, type_params, nominal_kinds, ctx)?;
                return Some(InterfaceTypeSnapshot::Array(Box::new(elem)));
            }
            if params.len() == 2 && (base == "tabula" || base == "dictum") {
                let key = interface_type_from_generic_arg(&params[0], interner, type_params, nominal_kinds, ctx)?;
                let val = interface_type_from_generic_arg(&params[1], interner, type_params, nominal_kinds, ctx)?;
                return Some(InterfaceTypeSnapshot::Map(Box::new(key), Box::new(val)));
            }
            if params.len() == 1 && base == "copia" {
                let elem = interface_type_from_generic_arg(&params[0], interner, type_params, nominal_kinds, ctx)?;
                return Some(InterfaceTypeSnapshot::Set(Box::new(elem)));
            }
            if params.len() == 1 {
                // Applied form `numerus<u32>` / `int<u32>` / `fractus<f32>` /
                // `float<f32>` (bare width markers are handled above).
                if let Some(width_ident) = generic_arg_bare_name(&params[0]) {
                    let width_name = interner.resolve(width_ident.name);
                    let family = match base {
                        "numerus" | "int" => Some(Primitive::Numerus),
                        "fractus" | "float" => Some(Primitive::Fractus),
                        _ => None,
                    };
                    if let Some(prim) = family
                        && let Ok(width) = NumericWidth::from_marker(width_name, prim)
                    {
                        return Some(InterfaceTypeSnapshot::SizedNumeric(prim, width));
                    }
                }
            }
            if params.len() == 1
                && base == "instans"
                && let Some(marker_ident) = generic_arg_bare_name(&params[0])
                && let Ok(precision) = InstansPrecision::from_marker(interner.resolve(marker_ident.name))
            {
                return Some(InterfaceTypeSnapshot::SizedInstans(precision));
            }
            // Tuple type `iuncta<A, B, …>`. Callables returning tuples (and
            // signatures containing them) otherwise fail the whole export, so
            // the convert-path snapshot must carry the element list.
            if base == "iuncta" && params.len() >= 2 {
                let elements = params
                    .iter()
                    .map(|element| interface_type_from_generic_arg(element, interner, type_params, nominal_kinds, ctx))
                    .collect::<Option<Vec<_>>>()?;
                return Some(InterfaceTypeSnapshot::Tuple(elements));
            }
            // Unknown applied form — fail this export only.
            None
        }
        TypeExprKind::Func(func) => {
            // Function-type reference (`(A, B) → Ret`), e.g. a closure-valued
            // parameter. Keep the callable contract so the export installs.
            let no_params = BTreeSet::new();
            let mut params = Vec::new();
            for param in &func.params {
                params.push(InterfaceParam {
                    ty: interface_type_from_type_expr(param, interner, &no_params, nominal_kinds, ctx)?,
                    mode: SemanticParamMode::Owned,
                    optional: false,
                    sponte: false,
                    axis: false,
                });
            }
            let ret = interface_type_from_type_expr(&func.ret, interner, &no_params, nominal_kinds, ctx)?;
            let err = match &func.err {
                Some(err_ty) => Some(Box::new(interface_type_from_type_expr(
                    err_ty,
                    interner,
                    &no_params,
                    nominal_kinds,
                    ctx,
                )?)),
                None => None,
            };
            Some(InterfaceTypeSnapshot::Func(InterfaceCallable {
                type_params: Vec::new(),
                params,
                ret: Box::new(ret),
                err,
                is_async: false,
                is_generator: false,
                tensor_edge_policy_requirements: Vec::new(),
                device: None,
            }))
        }
        TypeExprKind::PromissumFailable(success, alternate) => {
            let success = interface_type_from_type_expr(success, interner, type_params, nominal_kinds, ctx)?;
            let alternate = interface_type_from_type_expr(alternate, interner, type_params, nominal_kinds, ctx)?;
            Some(InterfaceTypeSnapshot::PromissumFailable(Box::new(success), Box::new(alternate)))
        }
        TypeExprKind::Qualified(path, params) if params.is_empty() => {
            let name = path.last().map(|segment| interner.resolve(segment.name))?;
            // The leading path segment is the importing module's binding for
            // the sibling module (`visus.Visus`); resolve it to the sibling's
            // canonical identity so the consumer binds the nominal through
            // its own canonical key.
            let qualifier_identity = path
                .first()
                .map(|segment| interner.resolve(segment.name))
                .and_then(|binding| ctx.bindings.get(binding))
                .cloned();
            Some(nominal_snapshot_qualified(name, nominal_kinds, qualifier_identity))
        }
        TypeExprKind::Union(members) if members.len() == 2 => {
            let left = interface_type_from_type_expr(&members[0], interner, type_params, nominal_kinds, ctx)?;
            let right = interface_type_from_type_expr(&members[1], interner, type_params, nominal_kinds, ctx)?;
            match (left, right) {
                (InterfaceTypeSnapshot::Primitive(Primitive::Nihil), other)
                | (other, InterfaceTypeSnapshot::Primitive(Primitive::Nihil)) => {
                    Some(InterfaceTypeSnapshot::Option(Box::new(other)))
                }
                (left, right) => Some(InterfaceTypeSnapshot::Union(vec![left, right])),
            }
        }
        TypeExprKind::Union(members) if members.len() > 2 => {
            let mut parts = Vec::new();
            for member in members {
                parts.push(interface_type_from_type_expr(
                    member,
                    interner,
                    type_params,
                    nominal_kinds,
                    ctx,
                )?);
            }
            Some(InterfaceTypeSnapshot::Union(parts))
        }
        TypeExprKind::Vector(element, figura) => Some(InterfaceTypeSnapshot::Vector(
            Box::new(interface_type_from_type_expr(
                element,
                interner,
                type_params,
                nominal_kinds,
                ctx,
            )?),
            interface_index_snapshot(figura, interner)?,
        )),
        TypeExprKind::Matrix(element, figura) => Some(InterfaceTypeSnapshot::Matrix(
            Box::new(interface_type_from_type_expr(
                element,
                interner,
                type_params,
                nominal_kinds,
                ctx,
            )?),
            interface_index_snapshot(figura, interner)?,
        )),
        TypeExprKind::Tensor(element, figura) => Some(InterfaceTypeSnapshot::Tensor(
            Box::new(interface_type_from_type_expr(
                element,
                interner,
                type_params,
                nominal_kinds,
                ctx,
            )?),
            interface_index_snapshot(figura, interner)?,
        )),
        TypeExprKind::Sparsa(element, figura) => Some(InterfaceTypeSnapshot::Sparsa(
            Box::new(interface_type_from_type_expr(
                element,
                interner,
                type_params,
                nominal_kinds,
                ctx,
            )?),
            interface_index_snapshot(figura, interner)?,
        )),
        TypeExprKind::Atomic(element) => Some(InterfaceTypeSnapshot::Atomic(Box::new(interface_type_from_type_expr(
            element,
            interner,
            type_params,
            nominal_kinds,
            ctx,
        )?))),
        _ => None,
    }
}

/// Snapshot a type-level index expression (`vector` width / `matrix` shape).
///
/// Mirrors the analysis-side `snapshot_interface_index`: declared `size`
/// parameters remain symbolic, affine offsets and exact constant division
/// retain structured nodes, and `[R, C]` shapes remain tuples. Concrete
/// arithmetic is folded only after natural-range and exactness checks.
fn interface_index_snapshot(figura: &FiguraExpr, interner: &crate::lexer::Interner) -> Option<InterfaceIndexSnapshot> {
    match figura {
        FiguraExpr::Literal(value) => Some(InterfaceIndexSnapshot::Literal(*value)),
        FiguraExpr::Var(ident) => Some(InterfaceIndexSnapshot::Param(interner.resolve(ident.name).to_owned())),
        FiguraExpr::Add(_, _) | FiguraExpr::Subtract(_, _) => {
            let mut offset = 0i128;
            let base = interface_figura_affine_base(figura, &mut offset)?;
            let base = interface_index_snapshot(base, interner)?;
            match base {
                InterfaceIndexSnapshot::Literal(value) => {
                    let value = i128::from(value).checked_add(offset)?;
                    Some(InterfaceIndexSnapshot::Literal(u64::try_from(value).ok()?))
                }
                base if offset == 0 => Some(base),
                base => Some(InterfaceIndexSnapshot::Affine { base: Box::new(base), offset }),
            }
        }
        FiguraExpr::Divide(value, divisor) => {
            if *divisor == 0 {
                return None;
            }
            let numerator = interface_index_snapshot(value, interner)?;
            match numerator {
                InterfaceIndexSnapshot::Literal(value) if value % *divisor == 0 => {
                    Some(InterfaceIndexSnapshot::Literal(value / *divisor))
                }
                InterfaceIndexSnapshot::Literal(_) => None,
                numerator => Some(InterfaceIndexSnapshot::Divide { numerator: Box::new(numerator), divisor: *divisor }),
            }
        }
        FiguraExpr::Tuple(dims) => dims
            .iter()
            .map(|dim| interface_index_snapshot(dim, interner))
            .collect::<Option<Vec<_>>>()
            .map(InterfaceIndexSnapshot::Tuple),
        FiguraExpr::Lift(lift) => match &**lift {
            // A spelled size lift snapshots like its payload: a literal
            // folds and a name stays symbolic (same as `Var`).
            radix_syntax::LiftExtent::Literal(value) => Some(InterfaceIndexSnapshot::Literal(*value)),
            radix_syntax::LiftExtent::Name(ident) => {
                Some(InterfaceIndexSnapshot::Param(interner.resolve(ident.name).to_owned()))
            }
            // Negative literals are rejected at resolve; whole-shape lifts
            // occur only in call type arguments, never in signatures.
            radix_syntax::LiftExtent::Negative { .. } | radix_syntax::LiftExtent::Shape(_) => None,
            radix_syntax::LiftExtent::Paren(inner) => interface_index_snapshot(inner, interner),
        },
        FiguraExpr::Infer => None,
    }
}

fn interface_figura_affine_base<'a>(figura: &'a FiguraExpr, offset: &mut i128) -> Option<&'a FiguraExpr> {
    match figura {
        FiguraExpr::Add(value, amount) => {
            *offset = offset.checked_add(i128::from(*amount))?;
            interface_figura_affine_base(value, offset)
        }
        FiguraExpr::Subtract(value, amount) => {
            *offset = offset.checked_sub(i128::from(*amount))?;
            interface_figura_affine_base(value, offset)
        }
        _ => Some(figura),
    }
}

/// Nominal snapshot for a name declared in the snapshotting module itself
/// (or unknown): local names bind to the module's own canonical identity so
/// same-named classes across providers never collide in the consumer's
/// name-keyed table.
fn nominal_snapshot(
    name: &str,
    nominal_kinds: &BTreeMap<String, InterfaceNominalKind>,
    module_identity: Option<&InterfaceLibraryIdentity>,
) -> InterfaceTypeSnapshot {
    let is_local = nominal_kinds.contains_key(name);
    InterfaceTypeSnapshot::Nominal(InterfaceNominalType {
        kind: nominal_kinds
            .get(name)
            .copied()
            .unwrap_or(InterfaceNominalKind::Struct),
        name: name.to_owned(),
        identity: if is_local { module_identity.cloned() } else { None },
        // Local names have an exact kind; names the module does not declare
        // locally fall back to a Struct guess (kind_guessed).
        kind_guessed: !is_local,
    })
}

/// Nominal snapshot for a module-qualified type reference: the nominal belongs
/// to the qualifier target's module, so the snapshot carries that module's
/// identity when the exporting module's import binding resolves (else the
/// legacy name-keyed path applies). The kind of a qualified name is a guess
/// unless it happens to match a locally-declared name.
fn nominal_snapshot_qualified(
    name: &str,
    nominal_kinds: &BTreeMap<String, InterfaceNominalKind>,
    qualifier_identity: Option<InterfaceLibraryIdentity>,
) -> InterfaceTypeSnapshot {
    InterfaceTypeSnapshot::Nominal(InterfaceNominalType {
        kind: nominal_kinds
            .get(name)
            .copied()
            .unwrap_or(InterfaceNominalKind::Struct),
        name: name.to_owned(),
        identity: qualifier_identity,
        kind_guessed: !nominal_kinds.contains_key(name),
    })
}

fn parse_program_for_namespace_exports(
    name: &str,
    source: &str,
    code_pack: ReaderPackRef<'_>,
) -> Option<(Program, crate::lexer::Interner)> {
    let peeled = crate::driver::peel_raw_source(name, source).ok()?;
    let lex_result = if let Some(pack) = code_pack {
        crate::lexer::lex_with_locale_pack(peeled.body, pack)
    } else {
        let pack = crate::driver::default_code_pack(&crate::driver::Config::default()).ok()?;
        crate::lexer::lex_with_locale_pack(peeled.body, &pack)
    };
    if !lex_result.success() {
        return None;
    }

    let parse_result = crate::parser::parse(lex_result);
    if !parse_result.success() {
        return None;
    }

    let crate::parser::ParseResult { program, interner, .. } = parse_result;
    Some((program?, interner))
}

pub(crate) use crate::import_resolve::resolve_namespace_import;

/// One implendum method as an importable signature (bodyless).
fn interface_method_from_ast(
    method: &crate::syntax::InterfaceMethod,
    interner: &crate::lexer::Interner,
    type_param_names: &BTreeSet<&str>,
    nominal_kinds: &BTreeMap<String, InterfaceNominalKind>,
    ctx: &SnapshotContext,
) -> Option<InterfaceMethodExport> {
    let mut params = Vec::new();
    for param in &method.params {
        params.push(InterfaceParam {
            ty: interface_type_from_type_expr(&param.ty, interner, type_param_names, nominal_kinds, ctx)?,
            mode: semantic_param_mode(param.mode),
            optional: param.sponte || param.default.is_some(),
            sponte: param.sponte,
            axis: param.mode == AstParamMode::Axis,
        });
    }
    let ret = match &method.ret {
        Some(ret) => interface_type_from_type_expr(ret, interner, type_param_names, nominal_kinds, ctx)?,
        None => InterfaceTypeSnapshot::Primitive(Primitive::Vacuum),
    };
    let err = match &method.err {
        Some(err_ty) => Some(Box::new(interface_type_from_type_expr(
            err_ty,
            interner,
            type_param_names,
            nominal_kinds,
            ctx,
        )?)),
        None => None,
    };
    Some(InterfaceMethodExport {
        name: interner.resolve(method.name.name).to_owned(),
        callable: InterfaceCallable {
            type_params: Vec::new(),
            params,
            ret: Box::new(ret),
            err,
            is_async: method.posture.is_async()
                || annotations_include_callable_family(&method.annotations, interner, "futura"),
            is_generator: method.posture.is_stream()
                || annotations_include_callable_family(&method.annotations, interner, "cursor"),
            tensor_edge_policy_requirements: Vec::new(),
            device: None,
        },
        visibility: crate::syntax::MemberVisibility::Publica,
        unavailable: Vec::new(),
    })
}

fn annotations_include_callable_family(
    annotations: &[Annotation],
    interner: &crate::lexer::Interner,
    family: &str,
) -> bool {
    annotations.iter().any(|annotation| match &annotation.kind {
        AnnotationKind::Futura => family == "futura",
        AnnotationKind::Cursor => family == "cursor",
        AnnotationKind::Nucleum(_) => family == "nucleum",
        AnnotationKind::Statement(stmt) => interner.resolve(stmt.name.name) == family,
        AnnotationKind::Braced(braced) => interner.resolve(braced.name.name) == family,
        _ => false,
    })
}

/// v2 device contract for one exported function declaration (DFV2-3).
///
/// The import link carries the device fact and the body channel together:
/// `@ nucleum` (or the reader-locale `@ kernel` spelling) sets the role, the
/// `@ publica` tier sets launch visibility, and a missing declaration body is
/// an explicit [`InterfaceBodyChannel::Missing`] — never a silent
/// signature-only export. Conflicting nucleum annotations yield no contract
/// (mirroring the HIR role-exclusivity check).
fn device_contract_from_stmt(
    stmt: &Stmt,
    decl: &crate::syntax::FuncDecl,
    interner: &crate::lexer::Interner,
) -> Option<InterfaceDeviceContract> {
    let mut role: Option<InterfaceDeviceRole> = None;
    for annotation in &stmt.annotations {
        let AnnotationKind::Nucleum(nucleum) = &annotation.kind else {
            continue;
        };
        let candidate = if nucleum.is_fragment {
            InterfaceDeviceRole::Fragment
        } else {
            InterfaceDeviceRole::Kernel
        };
        if role.is_some_and(|found| found != candidate) {
            return None;
        }
        role = Some(candidate);
    }
    let role = role?;
    let visibility = if matches!(
        crate::semantic::passes::visibility_annotation::statement_export_tier(stmt, interner),
        Some(VisibilityTier::Publica)
    ) {
        InterfaceDeviceVisibility::Public
    } else {
        InterfaceDeviceVisibility::Private
    };
    let body = if decl.body.is_some() {
        InterfaceBodyChannel::Source
    } else {
        InterfaceBodyChannel::Missing
    };
    Some(InterfaceDeviceContract { role, visibility, body })
}

fn annotation_contract_from_stmt(
    stmt: &Stmt,
    decl: &ClassDecl,
    interner: &crate::lexer::Interner,
) -> Option<InterfaceAnnotationContract> {
    let marker = stmt
        .annotations
        .iter()
        .find(|annotation| is_annotatio(annotation, interner))?;
    let target = annotatio_target(marker, interner).unwrap_or_else(|| "functio".to_owned());
    let fields = decl
        .members
        .iter()
        .filter_map(|member| {
            let ClassMemberKind::Field(field) = &member.kind else {
                return None;
            };
            let (ty, optional_union) = contract_field_type_spelling(&field.ty, interner)?;
            Some(InterfaceAnnotationContractField {
                name: interner.resolve(field.name.name).to_owned(),
                ty,
                optional: field.sponte || optional_union || field.init.is_some(),
            })
        })
        .collect();
    Some(InterfaceAnnotationContract { target, fields, qualified_identity: None })
}

fn is_annotatio(annotation: &Annotation, interner: &crate::lexer::Interner) -> bool {
    match &annotation.kind {
        AnnotationKind::Braced(braced) => interner.resolve(braced.name.name) == "annotatio",
        AnnotationKind::Statement(stmt) => interner.resolve(stmt.name.name) == "annotatio",
        _ => false,
    }
}

fn annotatio_target(annotation: &Annotation, interner: &crate::lexer::Interner) -> Option<String> {
    match &annotation.kind {
        AnnotationKind::Statement(stmt) if stmt.args.is_empty() => Some("functio".to_owned()),
        AnnotationKind::Braced(braced) if braced.fields.is_empty() => Some("functio".to_owned()),
        AnnotationKind::Braced(braced) => braced
            .fields
            .iter()
            .find(|field| interner.resolve(field.name.name) == "target")
            .and_then(|field| annotatio_target_field_value(&field.value, interner))
            .or_else(|| Some("functio".to_owned())),
        _ => None,
    }
}

fn annotatio_target_field_value(value: &AnnotationFieldValue, interner: &crate::lexer::Interner) -> Option<String> {
    match value {
        AnnotationFieldValue::Expr(expr) => match &expr.kind {
            ExprKind::Ident(ident) => Some(interner.resolve(ident.name).to_owned()),
            ExprKind::Literal(Literal::String(sym)) => Some(interner.resolve(*sym).to_owned()),
            _ => None,
        },
        AnnotationFieldValue::Type(ty) => match &ty.kind {
            TypeExprKind::Named(name, params) if params.is_empty() => Some(interner.resolve(name.name).to_owned()),
            _ => None,
        },
    }
}

fn contract_field_type_spelling(ty: &TypeExpr, interner: &crate::lexer::Interner) -> Option<(String, bool)> {
    match &ty.kind {
        TypeExprKind::Named(name, params) if params.is_empty() => {
            let text = interner.resolve(name.name);
            ContractFieldType::from_type_name(text).map(|_ty| (text.to_owned(), false))
        }
        TypeExprKind::Union(members) => {
            let mut core = None;
            let mut saw_nihil = false;
            for member in members {
                match &member.kind {
                    TypeExprKind::Named(name, params) if params.is_empty() => {
                        let text = interner.resolve(name.name);
                        if text == "nihil" {
                            saw_nihil = true;
                        } else if ContractFieldType::from_type_name(text).is_some() {
                            if core.is_some() {
                                return None;
                            }
                            core = Some(text.to_owned());
                        } else {
                            return None;
                        }
                    }
                    _ => return None,
                }
            }
            core.map(|ty| (ty, saw_nihil))
        }
        _ => None,
    }
}

/// Pre-check: reject import specifiers that `validate_import_paths` would later
/// reject, so resolution + filesystem I/O never touch policy-illegal targets.
///
/// Mirrors the checks in `crate::semantic::passes::import_path::validate_import_paths`.
/// Traversal and absolute-path guards shared by the direct contract walk and
/// the transitive same-package walk. Scheme allowlisting stays in
/// [`import_contract_path_allowed`].
fn import_path_shape_allowed(path: &str) -> bool {
    !(path == ".."
        || path.starts_with("../")
        || path.contains("/../")
        || path.ends_with("/..")
        || path.starts_with('/')
        || path.chars().nth(1) == Some(':'))
}

fn provider_segments(path: &str) -> Option<(&str, &str)> {
    path.split_once(':')
        .filter(|(provider, module)| !provider.is_empty() && !module.is_empty() && !module.contains(':'))
}

fn import_contract_path_allowed(path: &str, templates: Option<&BTreeMap<String, String>>) -> bool {
    if !import_path_shape_allowed(path) {
        return false;
    }
    if path.starts_with('§') {
        return templates.is_some();
    }
    if let Some((provider, module)) = path.split_once(':')
        && !provider.is_empty()
        && !module.is_empty()
        && !module.contains(':')
    {
        return true;
    }
    if path.starts_with("./") {
        return true;
    }
    false
}

#[cfg(test)]
#[path = "compile_test.rs"]
mod tests;
