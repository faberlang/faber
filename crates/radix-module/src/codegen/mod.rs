//! Shared code generation boundary for `HIR`-backed target emitters.
//!
//! Codegen is the last `HIR`-facing compiler phase. Semantic analysis has already
//! produced a typed [`HirModule`] and [`TypeTable`]; this module selects a
//! backend, rejects poisoned `HIR` that still contains recovery error nodes, and
//! returns the target-specific source artifact consumed by callers in `radix`
//! and the user-facing `faber` package builder.
//!
//! The per-target backends own language mapping policy. The shared layer owns
//! only contracts that every backend must obey: a common dispatch surface,
//! a single codegen error type, access to semantic types, and fail-closed
//! handling for `HIR` that semantic recovery could not make safe to emit.
//!
//! # Invariants
//!
//! - Backends receive `HIR` only after [`reject_hir_errors`] proves that recovery
//!   expressions are absent.
//! - The shared [`Codegen`] trait does not prescribe formatting, naming, or
//!   runtime strategy; those are backend policies.
//! - [`CodegenError`] marks an emission boundary problem, not a replacement for
//!   semantic diagnostics that should have been collected earlier.
//! - Target dispatch is explicit so adding a backend must also choose its
//!   public [`crate::Output`] variant.

pub use radix_semantic::coverage;
pub mod conversio_coverage;
#[cfg(feature = "hir-faber")]
pub mod faber;
#[cfg(feature = "hir-go")]
pub mod go;
#[cfg(feature = "hir-haskell")]
pub mod haskell;
pub(crate) mod hygiene;
mod names;
pub mod needs;
#[cfg(feature = "hir-python")]
pub mod python;
#[cfg(feature = "hir-rust")]
pub mod rust;
#[cfg(feature = "hir-swift")]
pub mod swift;
#[cfg(feature = "hir-ts")]
pub mod ts;

#[cfg(test)]
mod coherence_test;

// The failable-facts conversion under test is gated to the non-rust backends
// that call it, so its tests carry the same feature gate.
#[cfg(all(
    test,
    any(
        feature = "hir-go",
        feature = "hir-ts",
        feature = "hir-swift",
        feature = "hir-haskell"
    )
))]
#[path = "failable_facts_test.rs"]
mod failable_facts_test;

#[cfg(all(test, feature = "hir-haskell"))]
#[path = "haskell_emit_test.rs"]
mod haskell_emit_test;

#[cfg(feature = "hir-go")]
pub use needs::{GoNeeds, collect_go_needs};
#[cfg(feature = "hir-rust")]
pub use needs::{RustNeeds, collect_rust_needs};
/// Shared indentation-aware source writer (lives in `radix-codegen-shared`).
pub use radix_codegen_shared::{CodeWriter, Codegen, CodegenError};
#[cfg(feature = "hir-rust")]
pub use radix_hir_rust::representation_plan;
#[cfg(feature = "hir-rust")]
pub use rust::build_representation_plan;

/// Conversio (`↦`) emit-arm detection probe — measurement path only, inert in
/// production. The exempla conversio matrix harness uses it to measure the
/// dedicated-arm vs unspecialized-fallback tier per fixture.
pub use radix_codegen_shared::conversio_arm;
pub use radix_codegen_shared::conversio_arm::{
    ConversioArm, ConversioArmGuard, ConversioArmRecord, conversio_arm_begin, conversio_arm_clear,
    conversio_arm_mark_fallback, conversio_arm_take,
};

/// Output mode controlling access modifiers and entry-point emission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    /// Emit a runnable application with `@main` entry point (default).
    Application,
    /// Emit a library module with `public` declarations and no `@main`.
    Library,
}

use crate::diagnostics::DiagnosticArg;
use crate::hir::HirModule;
use crate::lexer::Interner;
use crate::locale::KeywordSurface;
#[cfg(all(test, feature = "hir-faber"))]
use crate::locale::latin_locale_pack;
use crate::semantic::TypeTable;
#[cfg(feature = "hir-rust")]
use crate::semantic::{NumericWidth, Primitive, Type, TypeId};

/// Compilation-target identity lives in the neutral `radix-target` crate;
/// re-exported here for path stability (`crate::codegen::Target`,
/// `radix::Target`, `radix_module::codegen::Target`).
pub use radix_target::Target;

fn unavailable_target_error(target: Target) -> CodegenError {
    let feature = target.required_feature().unwrap_or("full-targets");
    CodegenError {
        message: format!("target `{target}` is not available in this radix build; rebuild with feature `{feature}`"),
        args: vec![
            DiagnosticArg::new("issue", "target_unavailable"),
            DiagnosticArg::new("target", target.to_string()),
            DiagnosticArg::new("feature", feature),
        ],
    }
}

/// Rejection error for [`Target::HirFhir`]: serialization dispatch, not a
/// codegen backend.
fn hir_fhir_requires_emit_workflow() -> CodegenError {
    CodegenError {
        message: "Target::HirFhir is a serialization dispatch, not a codegen backend. Use the emit workflow."
            .to_owned(),
        args: vec![DiagnosticArg::new("issue", "hir_fhir_requires_emit_workflow")],
    }
}

/// Rejection error for `MIR`-backed targets: routing belongs to the driver.
fn mir_backed_requires_driver() -> CodegenError {
    CodegenError {
        message: "target is MIR-backed and must be routed through the driver".to_owned(),
        args: vec![DiagnosticArg::new("issue", "mir_backed_requires_driver")],
    }
}

/// Build the failable-def set and per-definition error-type overrides shared by
/// the non-Rust backend fact structs.
///
/// Dispatch-boundary conversion that consumes the monolith's
/// `FunctionFactTable`. Leaf crates deliberately never depend on
/// `crate::semantic`, so the conversion cannot move into any single leaf
/// without inventing a cross-leaf dependency. Gated to the non-rust
/// backends that call it: a rust-only build compiles neither the helper
/// nor its callers.
#[cfg(any(
    feature = "hir-go",
    feature = "hir-ts",
    feature = "hir-swift",
    feature = "hir-python",
    feature = "hir-haskell"
))]
fn failable_facts_parts(
    facts: &crate::semantic::FunctionFactTable,
) -> (
    rustc_hash::FxHashSet<crate::hir::DefId>,
    rustc_hash::FxHashMap<crate::hir::DefId, crate::semantic::TypeId>,
) {
    let mut err_tys = rustc_hash::FxHashMap::default();
    for (def_id, fact) in facts.iter() {
        if let Some(err_ty) = fact.err_ty {
            err_tys.insert(def_id, err_ty);
        }
    }
    (facts.failable_defs(), err_tys)
}

#[cfg(feature = "hir-go")]
fn go_failable_facts(facts: &crate::semantic::FunctionFactTable) -> radix_hir_go::FailableFacts {
    let (failable, err_tys) = failable_facts_parts(facts);
    radix_hir_go::FailableFacts { failable, err_tys }
}

#[cfg(feature = "hir-ts")]
fn ts_failable_facts(facts: &crate::semantic::FunctionFactTable) -> radix_hir_ts::FailableFacts {
    let (failable, err_tys) = failable_facts_parts(facts);
    radix_hir_ts::FailableFacts { failable, err_tys }
}

#[cfg(feature = "hir-swift")]
fn swift_failable_facts(facts: &crate::semantic::FunctionFactTable) -> radix_hir_swift::SwiftFailableFacts {
    let (failable, err_tys) = failable_facts_parts(facts);
    radix_hir_swift::SwiftFailableFacts { failable, err_tys }
}

#[cfg(feature = "hir-python")]
fn python_failable_facts(facts: &crate::semantic::FunctionFactTable) -> radix_hir_python::PythonFailableFacts {
    let (failable, err_tys) = failable_facts_parts(facts);
    radix_hir_python::PythonFailableFacts { failable, err_tys }
}

#[cfg(feature = "hir-haskell")]
fn haskell_failable_facts(facts: &crate::semantic::FunctionFactTable) -> radix_hir_haskell::HaskellFailableFacts {
    let (failable, err_tys) = failable_facts_parts(facts);
    radix_hir_haskell::HaskellFailableFacts { failable, err_tys }
}

/// Dispatch the analyzed `HIR` program to the selected target backend.
///
/// The shared entrypoint rejects `HIR` recovery artifacts before backend code can
/// accidentally print placeholders into generated source. After that guard, all
/// language-specific choices are delegated to the backend selected by
/// [`Target`].
///
/// This is the backward-compatible entry; use [`generate_from_analyzed_with_options`]
/// to specify output mode and module name for targets that support them.
///
/// # Errors
///
/// Returns [`CodegenError`] if the `HIR` still contains error recovery expressions
/// or if the target is `MIR`-backed and must be routed through the driver.
pub fn generate_from_analyzed(
    target: Target,
    analysis: &crate::driver::AnalyzedModule,
    surface: &KeywordSurface<'_>,
) -> Result<crate::Output, CodegenError> {
    generate_from_analyzed_with_options(target, analysis, surface, OutputMode::Application, None)
}

/// Dispatch the analyzed `HIR` program with output-mode options.
///
/// When `output_mode` is [`OutputMode::Library`], Swift backends emit `public`
/// declarations and suppress `@main` entry-point emission. `module_name` sets
/// the emitted Swift module comment.
///
/// # Errors
///
/// Returns [`CodegenError`] if the `HIR` still contains error recovery expressions
/// or if the target is `MIR`-backed and must be routed through the driver.
pub fn generate_from_analyzed_with_options(
    target: Target,
    analysis: &crate::driver::AnalyzedModule,
    #[cfg_attr(not(feature = "hir-faber"), allow(unused_variables))] surface: &KeywordSurface<'_>,
    output_mode: OutputMode,
    module_name: Option<&str>,
) -> Result<crate::Output, CodegenError> {
    radix_codegen_shared::reject_hir_errors(&analysis.hir)?;
    if !target.is_available() {
        return Err(unavailable_target_error(target));
    }
    match target {
        Target::HirRust => {
            #[cfg(feature = "hir-rust")]
            {
                reject_rust_unsupported_types(&analysis.types)?;

                let mut namespace_info = rust::build_file_import_namespaces(
                    &analysis.hir,
                    &analysis.interner,
                    &rust::ResolverImportsBridge(&analysis.resolver),
                );
                let provider_modules = emit_provider_modules(analysis)?;
                let gpu_builtins = rust::rust_gpu_builtins(&analysis.gpu_builtins);
                let function_facts = rust::rust_function_facts(&analysis.function_facts);
                let used_imports = rust::used_import_defs(&analysis.resolved_uses);

                // Provider-emitted signatures decide importer call-site
                // unwrapping: an `ad`-verb provider function emits `Result`
                // even without a declared `⇥` channel, so widen the imported
                // namespace effects for those members.
                for ((def_id, member), effects) in namespace_info.function_effects.iter_mut() {
                    let Some(modules) = namespace_info.modules.get(def_id) else {
                        continue;
                    };
                    if let Some(failable_exports) = provider_modules
                        .iter()
                        .find(|(name, _, _)| modules.iter().any(|segment| segment == name))
                        .map(|(_, _, failable_exports)| failable_exports)
                        && failable_exports.contains(member) {
                            effects.is_failable = true;
                        }
                }
                let mut r#gen = rust::RustCodegen::new_with_types(&analysis.hir, &analysis.interner, &analysis.types);
                r#gen.set_gpu_builtins(&gpu_builtins);
                r#gen.set_function_facts(&function_facts);
                r#gen.set_used_import_defs(&used_imports);
                r#gen.set_imported_namespace_info(namespace_info);
                let mut output = r#gen.generate(&analysis.hir, &analysis.types, &analysis.interner)?;

                if !provider_modules.is_empty() {
                    let mut module_block = String::new();
                    for (module_name, module_code, _failable) in &provider_modules {
                        module_block.push_str("pub mod ");
                        module_block.push_str(module_name);
                        module_block.push_str(" {\n");
                        module_block.push_str(module_code);
                        if !module_code.ends_with('\n') {
                            module_block.push('\n');
                        }
                        module_block.push_str("}\n\n");
                    }
                    output.code = insert_modules_after_leading_crate_attributes(&output.code, &module_block);
                }

                Ok(crate::Output::Rust(crate::RustOutput { code: output.code }))
            }
            #[cfg(not(feature = "hir-rust"))]
            {
                Err(unavailable_target_error(Target::HirRust))
            }
        }
        Target::HirTypeScript => {
            #[cfg(feature = "hir-ts")]
            {
                // Library-mode TypeScript must use the module entry path so
                // provider files carry named exports and their namespace
                // aggregate. Clone the analyzed interner only to intern the
                // output module name; HIR symbol ids remain stable.
                let mut module_interner = analysis.interner.clone();
                let ts_surface = ts::TsKeywordSurfaceBridge(*surface);
                let r#gen = if output_mode == OutputMode::Library {
                    let namespace = module_interner.intern(module_name.unwrap_or("FaberModule"));
                    ts::TsCodegen::new_module(&analysis.hir, &module_interner, namespace)
                } else {
                    ts::TsCodegen::new(&analysis.hir, &module_interner)
                };
                let mut r#gen = r#gen;
                r#gen.set_failable_facts(&ts_failable_facts(&analysis.function_facts));
                r#gen.set_library_registry(&analysis.libraries);
                r#gen.set_gpu_builtins(&ts_gpu_builtins(&analysis.gpu_builtins));
                r#gen.set_intrinsic_surface(&ts_surface);
                register_cross_file_defids(&mut r#gen, &analysis.hir, &analysis.resolver);
                let output = r#gen.generate(&analysis.hir, &analysis.types, &module_interner)?;
                Ok(crate::Output::TypeScript(crate::TypeScriptOutput { code: output.code }))
            }
            #[cfg(not(feature = "hir-ts"))]
            {
                Err(unavailable_target_error(Target::HirTypeScript))
            }
        }
        Target::HirGo => generate_go_from_analyzed(analysis),
        Target::HirSwift => generate_swift_from_analyzed(analysis, output_mode, module_name),
        Target::HirPython => generate_python_from_analyzed(analysis),
        Target::HirHaskell => generate_haskell_from_analyzed(analysis),
        #[cfg(feature = "hir-faber")]
        Target::HirFaber => {
            let imported_spellings = analysis.resolver.imported_spellings(&analysis.interner);
            let mut by_def = imported_spellings.by_def;
            by_def.extend(
                analysis
                    .resolver
                    .generated_companion_spellings(&analysis.interner),
            );
            let output = faber::emit(
                &analysis.hir,
                &analysis.presentation,
                &analysis.types,
                &analysis.interner,
                &faber::LocaleKeywordBridge(*surface),
                &imported_spellings.by_type,
                &by_def,
                &imported_spellings.by_def_pattern,
            )?;
            Ok(crate::Output::Faber(crate::FaberOutput { code: output.code }))
        }
        #[cfg(not(feature = "hir-faber"))]
        Target::HirFaber => Err(unavailable_target_error(target)),
        Target::HirFhir => Err(hir_fhir_requires_emit_workflow()),
        Target::MirStepper
        | Target::MirWasm
        | Target::MirLlvm
        | Target::MirLlvmHost
        | Target::MirAmd
        | Target::MirMetal
        | Target::MirWgsl
        | Target::MirSexp
        | Target::MirScena
        | Target::MirFmir
        | Target::MirFmirBinary
        | Target::MirFmirBundle => Err(mir_backed_requires_driver()),
    }
}

/// Register cross-file DefIds from sibling module imports into the TS codegen.
///
/// When a package file imports another local file (`importa ex "./bridge"`),
/// the type checker assigns DefIds for the exported types via the file
/// interface system. The TS `NameCatalog` only collects local HIR declarations,
/// so those cross-file DefIds would be unresolvable during code generation.
/// This mirrors the Rust backend's `build_file_import_namespaces` by reading
/// the resolver's file interface exports and registering them as external defs.
///
/// Transitive library leaves (no consumer HIR binding) are the rust DEFID001
/// class: facade signatures name those types, so they must land in the emit
/// table even when the consumer never wrote `import from "leaf"`.
#[cfg(feature = "hir-ts")]
fn register_cross_file_defids(r#gen: &mut ts::TsCodegen<'_>, hir: &HirModule, resolver: &crate::semantic::Resolver) {
    for item in &hir.items {
        let crate::hir::HirItemKind::Import(import) = &item.kind else {
            continue;
        };
        for import_item in &import.items {
            let binding = import_item.alias.unwrap_or(import_item.name);
            if !resolver.namespace_has_file_interface(binding) {
                continue;
            }
            for export in resolver.imported_file_type_exports(binding) {
                r#gen.register_external_def(export.def_id, export.member, binding);
            }
        }
    }
    for namespace in resolver.file_interface_namespaces() {
        for export in resolver.imported_file_type_exports(namespace) {
            r#gen.register_named_def(export.def_id, export.member);
        }
    }
}

/// Map compiler-owned GPU builtin `DefId`s to their TypeScript spellings.
///
/// GPU builtins are resolver-owned symbols with no local HIR declaration, so
/// the TS name catalog cannot resolve them; the backend emits a self-contained
/// host placeholder for these `DefId`s. Mirrors the Rust backend's
/// `set_gpu_builtins` seam at the same dispatch boundary.
#[cfg(feature = "hir-ts")]
fn ts_gpu_builtins(
    gpu_builtins: &[(crate::hir::DefId, radix_types::MirKernelBuiltin)],
) -> Vec<(crate::hir::DefId, &'static str)> {
    gpu_builtins
        .iter()
        .map(|(def_id, builtin)| (*def_id, builtin.spelling()))
        .collect()
}

#[cfg(feature = "hir-go")]
fn generate_go_from_analyzed(analysis: &crate::driver::AnalyzedModule) -> Result<crate::Output, CodegenError> {
    let mut r#gen = go::GoCodegen::new(&analysis.hir, &analysis.interner);
    r#gen.set_failable_facts(&go_failable_facts(&analysis.function_facts));
    r#gen.set_library_registry(&analysis.libraries);
    let output = r#gen.generate(&analysis.hir, &analysis.types, &analysis.interner)?;
    Ok(crate::Output::Go(crate::GoOutput { code: output.code }))
}

#[cfg(not(feature = "hir-go"))]
fn generate_go_from_analyzed(_analysis: &crate::driver::AnalyzedModule) -> Result<crate::Output, CodegenError> {
    Err(unavailable_target_error(Target::HirGo))
}

#[cfg(feature = "hir-swift")]
fn generate_swift_from_analyzed(
    analysis: &crate::driver::AnalyzedModule,
    output_mode: OutputMode,
    module_name: Option<&str>,
) -> Result<crate::Output, CodegenError> {
    let mut r#gen = swift::SwiftCodegen::new(&analysis.hir, &analysis.interner);
    r#gen.set_failable_facts(&swift_failable_facts(&analysis.function_facts));
    r#gen.set_output_mode(
        match output_mode {
            OutputMode::Application => swift::OutputMode::Application,
            OutputMode::Library => swift::OutputMode::Library,
        },
        module_name,
    );
    let output = r#gen.generate(&analysis.hir, &analysis.types, &analysis.interner)?;
    Ok(crate::Output::Swift(crate::SwiftOutput { code: output.code }))
}

#[cfg(not(feature = "hir-swift"))]
fn generate_swift_from_analyzed(
    _analysis: &crate::driver::AnalyzedModule,
    _output_mode: OutputMode,
    _module_name: Option<&str>,
) -> Result<crate::Output, CodegenError> {
    Err(unavailable_target_error(Target::HirSwift))
}

#[cfg(feature = "hir-python")]
fn generate_python_from_analyzed(analysis: &crate::driver::AnalyzedModule) -> Result<crate::Output, CodegenError> {
    let mut r#gen = python::PythonCodegen::new(&analysis.hir, &analysis.interner);
    r#gen.set_failable_facts(&python_failable_facts(&analysis.function_facts));
    let output = r#gen.generate(&analysis.hir, &analysis.types, &analysis.interner)?;
    Ok(crate::Output::Python(crate::PythonOutput { code: output.code }))
}

#[cfg(not(feature = "hir-python"))]
fn generate_python_from_analyzed(_analysis: &crate::driver::AnalyzedModule) -> Result<crate::Output, CodegenError> {
    Err(unavailable_target_error(Target::HirPython))
}

#[cfg(feature = "hir-haskell")]
fn generate_haskell_from_analyzed(analysis: &crate::driver::AnalyzedModule) -> Result<crate::Output, CodegenError> {
    let mut r#gen = haskell::HaskellCodegen::new(&analysis.hir, &analysis.interner);
    r#gen.set_failable_facts(&haskell_failable_facts(&analysis.function_facts));
    let output = r#gen.generate(&analysis.hir, &analysis.types, &analysis.interner)?;
    Ok(crate::Output::Haskell(crate::HaskellOutput { code: output.code }))
}

#[cfg(not(feature = "hir-haskell"))]
fn generate_haskell_from_analyzed(_analysis: &crate::driver::AnalyzedModule) -> Result<crate::Output, CodegenError> {
    Err(unavailable_target_error(Target::HirHaskell))
}

/// Analyze and emit each distinct file-namespace provider binding as a module.
#[cfg(feature = "hir-rust")]
fn emit_provider_modules(
    analysis: &crate::driver::AnalyzedModule,
) -> Result<Vec<(String, String, std::collections::BTreeSet<String>)>, CodegenError> {
    use std::collections::BTreeMap;
    use std::path::Path;

    let mut modules = BTreeMap::new();
    collect_provider_modules(analysis, Path::new("."), &mut modules)?;
    Ok(modules
        .into_iter()
        .map(|(name, (code, failable))| (name, code, failable))
        .collect())
}

#[cfg(feature = "hir-rust")]
fn collect_provider_modules(
    analysis: &crate::driver::AnalyzedModule,
    from_file: &std::path::Path,
    modules: &mut std::collections::BTreeMap<String, (String, std::collections::BTreeSet<String>)>,
) -> Result<(), CodegenError> {
    use crate::hir::HirItemKind;
    use crate::import_resolve::resolve_namespace_import;

    for item in &analysis.hir.items {
        let HirItemKind::Import(import) = &item.kind else {
            continue;
        };
        let import_path = analysis.interner.resolve(import.path);
        for import_item in &import.items {
            let binding = import_item.alias.unwrap_or(import_item.name);
            if !analysis.resolver.namespace_has_file_interface(binding) {
                continue;
            }
            let module_name = analysis.interner.resolve(binding).replace('-', "_");
            if modules.contains_key(&module_name) {
                continue;
            }
            let provider_path = resolve_namespace_import(from_file, import_path, None).map_err(|err| CodegenError {
                message: format!("cannot resolve provider import `{import_path}` for Rust module emission: {err}"),
                args: vec![
                    DiagnosticArg::new("issue", "provider_import_resolution_failed"),
                    DiagnosticArg::new("import_path", import_path),
                ],
            })?;
            let source = std::fs::read_to_string(&provider_path).map_err(|err| CodegenError {
                message: format!("cannot read provider source {}: {err}", provider_path.display()),
                args: vec![
                    DiagnosticArg::new("issue", "provider_source_read_failed"),
                    DiagnosticArg::new("path", provider_path.display().to_string()),
                ],
            })?;
            // Re-analysis is a program-layer entry: discover contracts from
            // the provider path, then fail-closed module analysis. Nested
            // imports (norma:valor → norma:chorda) stay empty without this.
            // The package `[reader] locale` (e.g. tela's `locale = "en"` for
            // `tela:valida`) is threaded as a non-explicit session pack so
            // an English provider stays English under an otherwise
            // Latin-default session instead of failing PARSE001.
            let mut config = crate::driver::Config::default()
                .with_bodyless_functions()
                .with_dev_locale_root();
            if let Some(pack) = crate::program::compile::package_reader_locale_pack(&provider_path) {
                config = config.with_locale_pack(pack);
            }
            let session = crate::driver::Session::new(config);
            let name = provider_path.display().to_string();
            let prepared =
                crate::program::compile::prepare_program_module(&provider_path, &source, &session).map_err(|diag| {
                    CodegenError {
                        message: format!("cannot prepare provider module `{module_name}`: {}", diag.message),
                        args: vec![
                            DiagnosticArg::new("issue", "provider_module_prepare_failed"),
                            DiagnosticArg::new("module", module_name.clone()),
                        ],
                    }
                })?;
            let unit = crate::driver::analyze_source(
                &prepared.session,
                &name,
                &source,
                crate::driver::AnalyzeRequest { cli_program: None, contract: prepared.contract },
            )
            .map_err(|diags| CodegenError {
                message: format!(
                    "provider module `{}` failed analysis: {}",
                    module_name,
                    diags
                        .iter()
                        .map(|d| d.message.as_str())
                        .collect::<Vec<_>>()
                        .join("; ")
                ),
                args: vec![
                    DiagnosticArg::new("issue", "provider_module_analysis_failed"),
                    DiagnosticArg::new("module", module_name.clone()),
                ],
            })?;
            if unit
                .diagnostics
                .iter()
                .any(radix_diagnostics::Diagnostic::is_error)
            {
                return Err(CodegenError {
                    message: format!("provider module `{module_name}` has analysis errors"),
                    args: vec![
                        DiagnosticArg::new("issue", "provider_module_analysis_errors"),
                        DiagnosticArg::new("module", module_name.clone()),
                    ],
                });
            }
            // Recurse first so nested namespace imports (norma:value →
            // norma:text) emit their own sibling modules; the provider's own
            // namespace calls then target `crate::<module>::…` paths that
            // exist. The `contains_key` guard above is the cycle brake.
            let nested_from = provider_path
                .parent()
                .map(std::path::Path::to_path_buf)
                .unwrap_or_else(|| std::path::Path::new(".").to_path_buf());
            collect_provider_modules(&unit, &nested_from, modules)?;
            let namespace_info = rust::build_file_import_namespaces(
                &unit.hir,
                &unit.interner,
                &rust::ResolverImportsBridge(&unit.resolver),
            );
            let module_out = radix_hir_rust::generate_with_library_registry_test_selection_and_imports(
                radix_hir_rust::ModuleGenerationRequest {
                    hir: &unit.hir,
                    types: &unit.types,
                    interner: &unit.interner,
                    libraries: &radix_hir::LibraryRegistry::default(),
                    test_selection: None,
                    function_facts: None,
                    module_mode: true,
                    cli_program: None,
                    imported_function_params: None,
                    imported_namespace_info: Some(namespace_info),
                    gpu_builtins: &[],
                    field_name_policy: Default::default(),
                    native_host_bootstrap: false,
                },
            )?;
            // The provider's emitted signatures (not its Faber declarations)
            // decide importer call-site unwrapping: `ad`-verb functions emit
            // `Result` even without a declared `⇥` channel.
            let failable_exports = radix_hir_rust::provider_failable_export_names(&unit.hir, &unit.interner);
            modules.insert(module_name, (module_out.code, failable_exports));
        }
    }

    Ok(())
}

/// Loose `HIR`+types entry used by unit tests and transitional callers.
///
/// Production package/driver paths must use [`generate_from_analyzed`] so
/// required effect facts cannot be omitted. This entry computes the shared
/// function-fact table from the supplied `HIR` before emission.
///
/// This is the backward-compatible entry; use [`generate_with_options`]
/// to specify output mode and module name for targets that support them.
///
/// # Errors
///
/// Returns [`CodegenError`] if the `HIR` contains error recovery expressions
/// or if the target is `MIR`-backed and must be routed through the driver.
#[cfg(test)]
pub fn generate(
    target: Target,
    hir: &HirModule,
    types: &TypeTable,
    interner: &Interner,
) -> Result<crate::Output, CodegenError> {
    generate_with_options(target, hir, types, interner, OutputMode::Application, None)
}

/// Loose `HIR`+types entry with output-mode options.
///
/// See [`generate_from_analyzed_with_options`] for option semantics.
///
/// # Errors
///
/// Returns [`CodegenError`] if the `HIR` contains error recovery expressions
/// or if the target is `MIR`-backed and must be routed through the driver.
#[cfg(test)]
pub fn generate_with_options(
    target: Target,
    hir: &HirModule,
    types: &TypeTable,
    interner: &Interner,
    output_mode: OutputMode,
    module_name: Option<&str>,
) -> Result<crate::Output, CodegenError> {
    radix_codegen_shared::reject_hir_errors(hir)?;
    if !target.is_available() {
        return Err(unavailable_target_error(target));
    }
    let facts = crate::semantic::analyze_function_facts(hir, types);

    match target {
        Target::HirRust => generate_rust_with_facts(hir, types, interner, &[], &facts),
        Target::HirTypeScript => generate_ts_with_facts(hir, types, interner, &facts),
        Target::HirGo => generate_go_with_facts(hir, types, interner, &facts),
        Target::HirSwift => generate_swift_with_facts(hir, types, interner, &facts, output_mode, module_name),
        Target::HirPython => generate_python_with_facts(hir, types, interner, &facts),
        Target::HirHaskell => generate_haskell_with_facts(hir, types, interner, &facts),
        #[cfg(feature = "hir-faber")]
        Target::HirFaber => {
            let latin = latin_locale_pack();
            let surface = KeywordSurface::new(&latin);
            // Loose HIR+types entry: no presentation sidecar, so render with
            // an empty presentation (trivia-free canonical output).
            let output = faber::emit(
                hir,
                &faber::empty_presentation(),
                types,
                interner,
                &faber::LocaleKeywordBridge(surface),
                &rustc_hash::FxHashMap::default(),
                &rustc_hash::FxHashMap::default(),
                &rustc_hash::FxHashMap::default(),
            )?;
            Ok(crate::Output::Faber(crate::FaberOutput { code: output.code }))
        }
        #[cfg(not(feature = "hir-faber"))]
        Target::HirFaber => Err(unavailable_target_error(target)),
        Target::HirFhir => Err(hir_fhir_requires_emit_workflow()),
        Target::MirStepper
        | Target::MirWasm
        | Target::MirLlvm
        | Target::MirLlvmHost
        | Target::MirAmd
        | Target::MirMetal
        | Target::MirWgsl
        | Target::MirSexp
        | Target::MirScena
        | Target::MirFmir
        | Target::MirFmirBinary
        | Target::MirFmirBundle => Err(mir_backed_requires_driver()),
    }
}

#[cfg(all(test, feature = "hir-ts"))]
fn generate_ts_with_facts(
    hir: &HirModule,
    types: &TypeTable,
    interner: &Interner,
    facts: &crate::semantic::FunctionFactTable,
) -> Result<crate::Output, CodegenError> {
    let mut r#gen = ts::TsCodegen::new(hir, interner);
    r#gen.set_failable_facts(&ts_failable_facts(facts));
    let output = r#gen.generate(hir, types, interner)?;
    Ok(crate::Output::TypeScript(crate::TypeScriptOutput { code: output.code }))
}

#[cfg(all(test, not(feature = "hir-ts")))]
fn generate_ts_with_facts(
    _hir: &HirModule,
    _types: &TypeTable,
    _interner: &Interner,
    _facts: &crate::semantic::FunctionFactTable,
) -> Result<crate::Output, CodegenError> {
    Err(unavailable_target_error(Target::HirTypeScript))
}

#[cfg(all(test, feature = "hir-go"))]
fn generate_go_with_facts(
    hir: &HirModule,
    types: &TypeTable,
    interner: &Interner,
    facts: &crate::semantic::FunctionFactTable,
) -> Result<crate::Output, CodegenError> {
    let mut r#gen = go::GoCodegen::new(hir, interner);
    r#gen.set_failable_facts(&go_failable_facts(facts));
    let output = r#gen.generate(hir, types, interner)?;
    Ok(crate::Output::Go(crate::GoOutput { code: output.code }))
}

#[cfg(all(test, not(feature = "hir-go")))]
fn generate_go_with_facts(
    _hir: &HirModule,
    _types: &TypeTable,
    _interner: &Interner,
    _facts: &crate::semantic::FunctionFactTable,
) -> Result<crate::Output, CodegenError> {
    Err(unavailable_target_error(Target::HirGo))
}

#[cfg(all(test, feature = "hir-swift"))]
fn generate_swift_with_facts(
    hir: &HirModule,
    types: &TypeTable,
    interner: &Interner,
    facts: &crate::semantic::FunctionFactTable,
    output_mode: OutputMode,
    module_name: Option<&str>,
) -> Result<crate::Output, CodegenError> {
    let mut r#gen = swift::SwiftCodegen::new(hir, interner);
    r#gen.set_failable_facts(&swift_failable_facts(facts));
    r#gen.set_output_mode(
        match output_mode {
            OutputMode::Application => swift::OutputMode::Application,
            OutputMode::Library => swift::OutputMode::Library,
        },
        module_name,
    );
    let output = r#gen.generate(hir, types, interner)?;
    Ok(crate::Output::Swift(crate::SwiftOutput { code: output.code }))
}

#[cfg(all(test, not(feature = "hir-swift")))]
fn generate_swift_with_facts(
    _hir: &HirModule,
    _types: &TypeTable,
    _interner: &Interner,
    _facts: &crate::semantic::FunctionFactTable,
    _output_mode: OutputMode,
    _module_name: Option<&str>,
) -> Result<crate::Output, CodegenError> {
    Err(unavailable_target_error(Target::HirSwift))
}

#[cfg(all(test, feature = "hir-python"))]
fn generate_python_with_facts(
    hir: &HirModule,
    types: &TypeTable,
    interner: &Interner,
    facts: &crate::semantic::FunctionFactTable,
) -> Result<crate::Output, CodegenError> {
    let mut r#gen = python::PythonCodegen::new(hir, interner);
    r#gen.set_failable_facts(&python_failable_facts(facts));
    let output = r#gen.generate(hir, types, interner)?;
    Ok(crate::Output::Python(crate::PythonOutput { code: output.code }))
}

#[cfg(all(test, not(feature = "hir-python")))]
fn generate_python_with_facts(
    _hir: &HirModule,
    _types: &TypeTable,
    _interner: &Interner,
    _facts: &crate::semantic::FunctionFactTable,
) -> Result<crate::Output, CodegenError> {
    Err(unavailable_target_error(Target::HirPython))
}

#[cfg(all(test, feature = "hir-haskell"))]
fn generate_haskell_with_facts(
    hir: &HirModule,
    types: &TypeTable,
    interner: &Interner,
    facts: &crate::semantic::FunctionFactTable,
) -> Result<crate::Output, CodegenError> {
    let mut r#gen = haskell::HaskellCodegen::new(hir, interner);
    r#gen.set_failable_facts(&haskell_failable_facts(facts));
    let output = r#gen.generate(hir, types, interner)?;
    Ok(crate::Output::Haskell(crate::HaskellOutput { code: output.code }))
}

#[cfg(all(test, not(feature = "hir-haskell")))]
fn generate_haskell_with_facts(
    _hir: &HirModule,
    _types: &TypeTable,
    _interner: &Interner,
    _facts: &crate::semantic::FunctionFactTable,
) -> Result<crate::Output, CodegenError> {
    Err(unavailable_target_error(Target::HirHaskell))
}

#[cfg(all(test, feature = "hir-rust"))]
fn generate_rust_with_facts(
    hir: &HirModule,
    types: &TypeTable,
    interner: &Interner,
    gpu_builtins: &[(crate::hir::DefId, radix_types::MirKernelBuiltin)],
    facts: &crate::semantic::FunctionFactTable,
) -> Result<crate::Output, CodegenError> {
    radix_codegen_shared::reject_hir_errors(hir)?;
    reject_rust_unsupported_types(types)?;
    let gpu = rust::rust_gpu_builtins(gpu_builtins);
    let function_facts = rust::rust_function_facts(facts);
    let mut r#gen = rust::RustCodegen::new_with_types(hir, interner, types);
    r#gen.set_gpu_builtins(&gpu);
    r#gen.set_function_facts(&function_facts);
    let output = r#gen.generate(hir, types, interner)?;
    Ok(crate::Output::Rust(crate::RustOutput { code: output.code }))
}

#[cfg(all(test, not(feature = "hir-rust")))]
fn generate_rust_with_facts(
    _hir: &HirModule,
    _types: &TypeTable,
    _interner: &Interner,
    _gpu_builtins: &[(crate::hir::DefId, radix_types::MirKernelBuiltin)],
    _facts: &crate::semantic::FunctionFactTable,
) -> Result<crate::Output, CodegenError> {
    Err(unavailable_target_error(Target::HirRust))
}

/// Emit Rust CLI product code from a CLI program.
///
/// # Errors
///
/// Returns [`CodegenError`] if the `HIR` contains error recovery expressions,
/// the types table contains Rust-unsupported types, or the Rust CLI backend
/// emission fails.
pub fn generate_rust_cli(
    #[cfg_attr(not(feature = "hir-rust"), allow(unused_variables))] hir: &HirModule,
    #[cfg_attr(not(feature = "hir-rust"), allow(unused_variables))] types: &TypeTable,
    #[cfg_attr(not(feature = "hir-rust"), allow(unused_variables))] interner: &Interner,
    #[cfg_attr(not(feature = "hir-rust"), allow(unused_variables))] cli_program: &crate::cli::CliProgram,
) -> Result<crate::RustOutput, CodegenError> {
    #[cfg(not(feature = "hir-rust"))]
    {
        return Err(unavailable_target_error(Target::HirRust));
    }
    #[cfg(feature = "hir-rust")]
    {
        radix_codegen_shared::reject_hir_errors(hir)?;
        reject_rust_unsupported_types(types)?;
        let ir = rust::to_cli_ir(cli_program);
        let output = rust::RustCodegen::new_with_types(hir, interner, types).generate_cli(hir, types, &ir)?;
        Ok(crate::RustOutput { code: output.code })
    }
}

/// Emit Go CLI product code for a `SupportedNarrow` program.
///
/// # Errors
///
/// Returns [`CodegenError`] if the HIR contains error recovery expressions,
/// the types table contains Go-unsupported types, or the Go backend emission
/// fails.
pub fn generate_go_cli(
    #[cfg_attr(not(feature = "hir-go"), allow(unused_variables))] hir: &HirModule,
    #[cfg_attr(not(feature = "hir-go"), allow(unused_variables))] types: &TypeTable,
    #[cfg_attr(not(feature = "hir-go"), allow(unused_variables))] interner: &Interner,
    #[cfg_attr(not(feature = "hir-go"), allow(unused_variables))] cli_program: &crate::cli::CliProgram,
) -> Result<crate::GoOutput, CodegenError> {
    #[cfg(feature = "hir-go")]
    {
        go::generate_go_cli(hir, types, interner, cli_program)
    }
    #[cfg(not(feature = "hir-go"))]
    {
        Err(unavailable_target_error(Target::HirGo))
    }
}

#[cfg(feature = "hir-rust")]
fn reject_rust_unsupported_types(types: &TypeTable) -> Result<(), CodegenError> {
    for index in 0..types.type_count() {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "TypeId is u32 by design; type_count is bounded by u32::MAX"
        )]
        let ty = TypeId(index as u32);
        if matches!(types.get(ty), Type::SizedNumeric(Primitive::Fractus, NumericWidth::F16)) {
            return Err(CodegenError {
                message: "Rust target does not support fractus<f16> until a half-float runtime representation exists"
                    .to_owned(),
                args: vec![DiagnosticArg::new("issue", "rust_target_fractus_f16_unsupported")],
            });
        }
        if matches!(types.get(ty), Type::SizedNumeric(Primitive::Fractus, NumericWidth::BF16)) {
            return Err(CodegenError {
                message: "Rust target does not support fractus<bf16> until a half-float runtime representation exists"
                    .to_owned(),
                args: vec![DiagnosticArg::new("issue", "rust_target_fractus_bf16_unsupported")],
            });
        }
        if matches!(types.get(ty), Type::Atomic(_)) {
            return Err(CodegenError {
                message: "Rust target does not support atomic types until a storage representation exists".to_owned(),
                args: vec![DiagnosticArg::new("issue", "rust_target_atomic_unsupported")],
            });
        }
    }
    Ok(())
}

/// Splice provider `modules` into entry code after its leading crate
/// attributes (comments/blank lines/`#![...]`), mirroring the package crate
/// assembler in `radix-program`. Prepending before the attributes would strand
/// the entry's inner-attribute preamble mid-file, where Rust rejects it.
fn insert_modules_after_leading_crate_attributes(entry: &str, modules: &str) -> String {
    let mut insert_at = 0usize;
    let mut scanned = 0usize;
    for line in entry.lines() {
        let trimmed = line.trim();
        scanned += line.len() + 1;
        if trimmed.is_empty() || trimmed.starts_with("//") {
            insert_at = scanned;
            continue;
        }
        if trimmed.starts_with("#![") {
            insert_at = scanned;
            continue;
        }
        break;
    }
    let insert_at = insert_at.min(entry.len());
    let mut assembled = String::with_capacity(entry.len() + modules.len());
    assembled.push_str(&entry[..insert_at]);
    assembled.push_str(modules);
    assembled.push_str(&entry[insert_at..]);
    assembled
}
