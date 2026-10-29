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
#[cfg(feature = "hir-c")]
pub mod c;
pub mod cli_ir;
#[cfg(feature = "hir-cobol")]
pub mod cobol;
pub mod conversio_coverage;
#[cfg(feature = "hir-faber")]
pub mod faber;
#[cfg(feature = "hir-fortran")]
pub mod fortran;
#[cfg(feature = "hir-go")]
pub mod go;
#[cfg(feature = "hir-haskell")]
pub mod haskell;
pub(crate) mod hygiene;
pub mod input;
mod names;
pub mod needs;
#[cfg(feature = "hir-odin")]
pub mod odin;
#[cfg(feature = "hir-pascal")]
pub mod pascal;
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

#[cfg(test)]
mod coverage_test;

#[cfg(test)]
#[path = "await_store_emit_test.rs"]
mod await_store_emit_test;

#[cfg(test)]
#[path = "leaf_gate_emit_test.rs"]
mod leaf_gate_emit_test;

#[cfg(test)]
#[path = "indexed_apud_emit_test.rs"]
mod indexed_apud_emit_test;

#[cfg(all(test, feature = "hir-rust"))]
#[path = "rust_half_float_cell_test.rs"]
mod rust_half_float_cell_test;

#[cfg(test)]
#[path = "tensor_checked_method_emit_test.rs"]
mod tensor_checked_method_emit_test;

#[cfg(test)]
#[path = "saturating_fuzzy_incdec_emit_test.rs"]
mod saturating_fuzzy_incdec_emit_test;

#[cfg(test)]
#[path = "statica_emit_test.rs"]
mod statica_emit_test;

#[cfg(test)]
#[path = "copy_into_emit_test.rs"]
mod copy_into_emit_test;

#[cfg(test)]
#[path = "bits_type_gate_test.rs"]
mod bits_type_gate_test;

#[cfg(test)]
#[path = "ledger_type_gate_test.rs"]
mod ledger_type_gate_test;
#[cfg(test)]
#[path = "list_window_gate_test.rs"]
mod list_window_gate_test;
#[cfg(test)]
#[path = "lista_lift_gate_test.rs"]
mod lista_lift_gate_test;
#[cfg(test)]
#[path = "ranged_list_gate_test.rs"]
mod ranged_list_gate_test;
#[cfg(test)]
#[path = "tensor_lift_gate_test.rs"]
mod tensor_lift_gate_test;

#[cfg(test)]
#[path = "vector_lift_gate_test.rs"]
mod vector_lift_gate_test;

#[cfg(test)]
#[path = "scalar_math_emit_test.rs"]
mod scalar_math_emit_test;

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

#[cfg(all(
    test,
    feature = "hir-rust",
    feature = "hir-go",
    feature = "hir-ts",
    feature = "hir-swift",
    feature = "hir-python"
))]
#[path = "float_integer_conversio_emit_test.rs"]
mod float_integer_conversio_emit_test;

#[cfg(all(test, feature = "hir-swift"))]
#[path = "swift_contract_call_label_test.rs"]
mod swift_contract_call_label_test;

#[cfg(all(test, feature = "hir-swift"))]
#[path = "swift_record_hole_test.rs"]
mod swift_record_hole_test;

#[cfg(all(
    test,
    feature = "hir-rust",
    feature = "hir-go",
    feature = "hir-swift",
    feature = "hir-python"
))]
#[path = "approx_integer_run_test.rs"]
mod approx_integer_run_test;

#[cfg(all(test, feature = "hir-swift"))]
#[path = "swift_keyword_names_run_test.rs"]
mod swift_keyword_names_run_test;

#[cfg(all(
    test,
    feature = "hir-rust",
    feature = "hir-go",
    feature = "hir-ts",
    feature = "hir-swift"
))]
#[path = "ordo_conversio_emit_test.rs"]
mod ordo_conversio_emit_test;

#[cfg(all(
    test,
    feature = "hir-rust",
    feature = "hir-go",
    feature = "hir-ts",
    feature = "hir-swift"
))]
#[path = "littera_conversio_emit_test.rs"]
mod littera_conversio_emit_test;

#[cfg(all(
    test,
    feature = "hir-rust",
    feature = "hir-go",
    feature = "hir-ts",
    feature = "hir-swift"
))]
#[path = "conversio_function_emit_test.rs"]
mod conversio_function_emit_test;

#[cfg(all(
    test,
    feature = "hir-rust",
    feature = "hir-go",
    feature = "hir-ts",
    feature = "hir-swift",
    feature = "hir-python"
))]
#[path = "unhandled_call_main_emit_test.rs"]
mod unhandled_call_main_emit_test;

#[cfg(all(
    test,
    feature = "hir-rust",
    feature = "hir-go",
    feature = "hir-ts",
    feature = "hir-faber",
    feature = "hir-swift",
    feature = "hir-python",
    feature = "hir-haskell"
))]
#[path = "non_null_method_call_emit_test.rs"]
mod non_null_method_call_emit_test;

#[cfg(all(
    test,
    feature = "hir-rust",
    feature = "hir-go",
    feature = "hir-ts",
    feature = "hir-faber",
    feature = "hir-swift",
    feature = "hir-python",
    feature = "hir-haskell"
))]
#[path = "nondum_function_emit_test.rs"]
mod nondum_function_emit_test;

#[cfg(all(test, feature = "hir-rust", feature = "hir-go"))]
#[path = "optional_list_literal_emit_test.rs"]
mod optional_list_literal_emit_test;

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

use crate::codegen::input::{CodegenAnalysis, CodegenProvider};
use crate::diagnostics::DiagnosticArg;
use crate::hir::HirModule;
use crate::lexer::Interner;
use crate::locale::KeywordSurface;
use crate::semantic::TypeTable;
#[cfg(feature = "hir-rust")]
use crate::semantic::{NumericWidth, Primitive, Type, TypeId};

/// Compilation-target identity lives in the neutral `radix-target` crate;
/// re-exported here for path stability (`crate::codegen::Target`,
/// `crate::codegen::OutputMode`, `radix::Target`,
/// `radix_module::codegen::Target`).
pub use radix_target::{Leaf, OutputMode, Target};

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

/// Convert analyzed function facts once at the dispatch boundary.
///
/// All non-Rust HIR backends consume this neutral carrier, so target routing
/// does not maintain one adapter and nominal wrapper per backend.
#[cfg(any(
    feature = "hir-go",
    feature = "hir-ts",
    feature = "hir-swift",
    feature = "hir-python",
    feature = "hir-haskell"
))]
fn failable_facts(facts: &crate::semantic::FunctionFactTable) -> radix_hir::FailableFacts {
    let mut err_tys = handroll::FxHashMap::default();
    for (def_id, fact) in facts.iter() {
        if let Some(err_ty) = fact.err_ty {
            err_tys.insert(def_id, err_ty);
        }
    }
    radix_hir::FailableFacts { failable: facts.failable_defs(), err_tys, async_defs: facts.async_defs() }
}

#[cfg(feature = "hir-rust")]
fn imported_tensor_edge_policy_callback_identities(
    resolver: &crate::semantic::Resolver,
    interner: &crate::lexer::Interner,
) -> Vec<(radix_hir::DefId, radix_hir::DefId, String, String)> {
    resolver
        .imported_file_method_identities()
        .into_iter()
        .map(|(callback, owner, namespace, owner_name, method)| {
            let namespace = interner.resolve(namespace);
            let owner_name = interner.resolve(owner_name);
            let method = interner.resolve(method).to_owned();
            (callback, owner, format!("{namespace}::{owner_name}::{method}"), method)
        })
        .collect()
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
    analysis: &dyn CodegenAnalysis,
    surface: &KeywordSurface<'_>,
) -> Result<crate::Output, CodegenError> {
    generate_from_analyzed_with_options(target, analysis, surface, OutputMode::Application, None)
}

/// Dispatch the analyzed `HIR` program with output-mode options.
///
/// When `output_mode` is [`OutputMode::Library`], Swift backends emit `public`
/// declarations and suppress `@main` entry-point emission, and the TypeScript
/// backend routes through its module entry path. `module_name` sets the emitted
/// Swift module comment and the TypeScript module namespace.
///
/// # Errors
///
/// Returns [`CodegenError`] if the `HIR` still contains error recovery expressions
/// or if the target is `MIR`-backed and must be routed through the driver.
pub fn generate_from_analyzed_with_options(
    target: Target,
    analysis: &dyn CodegenAnalysis,
    #[cfg_attr(not(feature = "hir-faber"), allow(unused_variables))] surface: &KeywordSurface<'_>,
    output_mode: OutputMode,
    module_name: Option<&str>,
) -> Result<crate::Output, CodegenError> {
    radix_codegen_shared::reject_hir_errors(analysis.hir())?;
    radix_codegen_shared::reject_bits_type(analysis.types(), target)?;
    radix_codegen_shared::reject_ledger_type(analysis.types(), target)?;
    radix_codegen_shared::reject_ranged_int_type(analysis.types(), target)?;
    radix_codegen_shared::reject_ranged_list_type(analysis.types(), target)?;
    reject_unsupported_leaves(analysis, target)?;
    if !target.is_available() {
        return Err(unavailable_target_error(target));
    }
    // Declaration-only `@ nondum` functions have nothing to emit. The Faber
    // re-emitter below reads the full module so the declaration survives.
    let emit_hir = analysis
        .hir()
        .without_unavailable_declarations(analysis.interner());
    let hir = &*emit_hir;
    match target {
        Target::HirRust => generate_rust_from_analyzed(analysis, hir),
        Target::HirTypeScript => {
            #[cfg(feature = "hir-ts")]
            {
                // Library-mode TypeScript must use the module entry path so
                // provider files carry named exports and their namespace
                // aggregate. Clone the analyzed interner only to intern the
                // output module name; HIR symbol ids remain stable.
                let mut module_interner = analysis.interner().clone();
                let ts_surface = ts::TsKeywordSurfaceBridge(*surface);
                let mut r#gen = if output_mode == OutputMode::Library {
                    let namespace = module_interner.intern(module_name.unwrap_or("FaberModule"));
                    ts::TsCodegen::new_module(hir, analysis.types(), &module_interner, namespace)
                } else {
                    ts::TsCodegen::new(hir, analysis.types(), &module_interner)
                };
                r#gen.set_display_tokens(display_tokens_for_module(analysis));
                r#gen.set_failable_facts(&failable_facts(analysis.function_facts()));
                r#gen.set_library_registry(analysis.libraries());
                r#gen.set_gpu_builtins(&ts_gpu_builtins(analysis.gpu_builtins()));
                r#gen.set_intrinsic_surface(&ts_surface);
                r#gen.set_ordo_order_rows(&analysis.tensor_edge_policies().ordo_order.comparisons);
                register_cross_file_defids(&mut r#gen, hir, analysis.resolver(), analysis.libraries());
                let output = r#gen.generate(hir, analysis.types(), &module_interner)?;
                Ok(crate::Output::TypeScript(crate::TypeScriptOutput { code: output.code }))
            }
            #[cfg(not(feature = "hir-ts"))]
            {
                Err(unavailable_target_error(Target::HirTypeScript))
            }
        }
        Target::HirGo => generate_go_from_analyzed(analysis, hir),
        Target::HirSwift => generate_swift_from_analyzed(analysis, hir, output_mode, module_name),
        Target::HirPython => generate_python_from_analyzed(analysis, hir),
        Target::HirHaskell => generate_haskell_from_analyzed(analysis, hir),
        Target::HirPascal => generate_pascal_from_analyzed(analysis, hir),
        Target::HirCobol => generate_cobol_from_analyzed(analysis, hir),
        Target::HirFortran => generate_fortran_from_analyzed(analysis, hir),
        Target::HirOdin => generate_odin_from_analyzed(analysis, hir),
        Target::HirC => generate_c_from_analyzed(analysis, hir),
        #[cfg(feature = "hir-faber")]
        Target::HirFaber => {
            let imported_spellings = analysis.resolver().imported_spellings(analysis.interner());
            let mut presentation = analysis.presentation().clone();
            crate::hir::presentation_witness::normalize_presentation_witnesses(
                analysis.hir(),
                &mut presentation,
                analysis.types(),
            );
            let mut by_def = imported_spellings.by_def;
            by_def.extend(
                analysis
                    .resolver()
                    .generated_companion_spellings(analysis.interner()),
            );
            let output = faber::FaberCodegen::new(analysis.types(), analysis.interner())
                .with_imported_spellings(&imported_spellings.by_type, &by_def, &imported_spellings.by_def_pattern)
                .generate(analysis.hir(), &presentation, &faber::LocaleKeywordBridge(*surface))?;
            Ok(crate::Output::Faber(crate::FaberOutput { code: output.code }))
        }
        #[cfg(not(feature = "hir-faber"))]
        Target::HirFaber => Err(unavailable_target_error(target)),
        Target::HirFhir => Err(hir_fhir_requires_emit_workflow()),
        Target::MirStepper
        | Target::MirWasm
        | Target::MirLlvm
        | Target::MirLlvmHost
        | Target::MirClif
        | Target::MirAmd
        | Target::MirMetal
        | Target::MirWgsl
        | Target::MirSexp
        | Target::MirBasic
        | Target::MirScena
        | Target::MirFmir
        | Target::MirFmirBinary => Err(mir_backed_requires_driver()),
    }
}

#[cfg(feature = "hir-rust")]
fn generate_rust_from_analyzed(analysis: &dyn CodegenAnalysis, hir: &HirModule) -> Result<crate::Output, CodegenError> {
    reject_rust_unsupported_types(analysis.types())?;

    let mut namespace_info =
        rust::build_file_import_namespaces(hir, analysis.interner(), &rust::ResolverImportsBridge(analysis.resolver()));
    let callback_identities = imported_tensor_edge_policy_callback_identities(analysis.resolver(), analysis.interner());
    let mut emitted_tensor_policy_providers = handroll::FxHashSet::default();
    let mut policy_identity_codegen = rust::RustCodegen::new_with_types(hir, analysis.interner(), analysis.types());
    policy_identity_codegen.set_imported_namespace_type_paths(namespace_info.type_paths.clone());
    for provider in &analysis.tensor_edge_policies().provider_impls {
        if let Some(key) = policy_identity_codegen.tensor_edge_policy_provider_key(provider, analysis.types(), None)? {
            emitted_tensor_policy_providers.insert(key);
        }
    }
    let provider_modules = emit_provider_modules(analysis, &mut emitted_tensor_policy_providers)?;
    let gpu_builtins = rust::rust_gpu_builtins(analysis.gpu_builtins());
    let function_facts = rust::rust_function_facts(analysis.function_facts());
    let used_imports = rust::used_import_defs(analysis.resolved_uses());

    // Provider-emitted signatures decide importer call-site
    // unwrapping: an `ad`-verb provider function emits `Result`
    // even without a declared `⇥` channel, so widen the imported
    // namespace effects for those members.
    let mut provider_failable_exports = handroll::FxHashMap::default();
    for (name, _, failable_exports) in &provider_modules {
        provider_failable_exports
            .entry(name.as_str())
            .or_insert(failable_exports);
    }
    for ((def_id, member), effects) in &mut namespace_info.function_effects {
        let Some(modules) = namespace_info.modules.get(def_id) else {
            continue;
        };
        if let Some(failable_exports) = modules
            .iter()
            .find_map(|segment| provider_failable_exports.get(segment.as_str()))
            && failable_exports.contains(member)
        {
            effects.is_failable = true;
        }
    }
    let mut r#gen = rust::RustCodegen::new_with_types(hir, analysis.interner(), analysis.types());
    r#gen.set_display_tokens(display_tokens_for_module(analysis));
    r#gen.set_shape_obligations(rust::rust_shape_obligations(analysis.shape_obligations()));
    r#gen.set_tensor_edge_policy_table(Some(analysis.tensor_edge_policies()));
    r#gen.set_tensor_edge_policy_callback_identities(callback_identities.iter().cloned());
    r#gen.set_gpu_builtins(&gpu_builtins);
    r#gen.set_function_facts(&function_facts);
    r#gen.set_used_import_defs(&used_imports);
    r#gen.set_imported_namespace_info(namespace_info);
    let mut output = r#gen.generate(hir, analysis.types(), analysis.interner())?;

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
fn generate_rust_from_analyzed(
    _analysis: &dyn CodegenAnalysis,
    _hir: &HirModule,
) -> Result<crate::Output, CodegenError> {
    Err(unavailable_target_error(Target::HirRust))
}

/// Output tokens used by literals emitted from this analyzed module.
#[cfg(any(
    feature = "hir-rust",
    feature = "hir-ts",
    feature = "hir-go",
    feature = "hir-swift",
    feature = "hir-python",
    feature = "hir-haskell"
))]
pub(crate) fn display_tokens_for_module(analysis: &dyn CodegenAnalysis) -> radix_codegen_shared::names::DisplayTokens {
    analysis.display_tokens().clone()
}

#[cfg(all(
    test,
    feature = "hir-rust",
    feature = "hir-ts",
    feature = "hir-go",
    feature = "hir-swift",
    feature = "hir-python",
    feature = "hir-haskell"
))]
#[path = "display_tokens_test.rs"]
mod display_tokens_test;

#[cfg(all(test, feature = "hir-ts", feature = "hir-go"))]
#[path = "cross_locale_import_names_test.rs"]
mod cross_locale_import_names_test;

/// Register cross-file `DefIds` from sibling module imports into the TS codegen.
///
/// When a package file imports another local file (`importa ex "./bridge"`),
/// the type checker assigns `DefIds` for the exported types via the file
/// interface system. The TS `NameCatalog` only collects local HIR declarations,
/// so those cross-file `DefIds` would be unresolvable during code generation.
/// This mirrors the Rust backend's `build_file_import_namespaces` by reading
/// the resolver's file interface exports and registering them as external defs.
///
/// Transitive library leaves (no consumer HIR binding) are the rust DEFID001
/// class: facade signatures name those types, so they must land in the emit
/// table even when the consumer never wrote `import from "leaf"`.
///
/// The interface's member is the consumer-locale surface spelling; the
/// exporting unit declares the canonical spelling, so every registration maps
/// through [`Resolver::imported_export_canonical_name`] exactly as the Rust
/// backend does. Library items keep their library-registry spelling: the
/// bindingless walk skips them so a file interface cannot shadow `libraries`.
#[cfg(feature = "hir-ts")]
fn register_cross_file_defids(
    r#gen: &mut ts::TsCodegen<'_>,
    hir: &HirModule,
    resolver: &crate::semantic::Resolver,
    libraries: &crate::hir::LibraryRegistry,
) {
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
                let canonical = resolver.imported_export_canonical_name(binding, export.member);
                r#gen.register_external_def(export.def_id, canonical, binding);
            }
        }
    }
    for namespace in resolver.file_interface_namespaces() {
        for export in resolver.imported_file_type_exports(namespace) {
            if libraries.items.contains_key(&export.def_id) {
                continue;
            }
            let canonical = resolver.imported_export_canonical_name(namespace, export.member);
            r#gen.register_named_def(export.def_id, canonical);
        }
    }
    // A `ns.f(..)` call to an imported failable function: the importing
    // module's own function facts do not cover the callee, so its `⇥` channel
    // is read from the file interface.
    for item in &hir.items {
        let crate::hir::HirItemKind::Import(import) = &item.kind else {
            continue;
        };
        for import_item in &import.items {
            let binding = import_item.alias.unwrap_or(import_item.name);
            for (member, sig) in resolver.namespace_file_function_exports(binding) {
                if sig.err.is_some() {
                    r#gen.register_external_failable_fn(import_item.def_id, member);
                }
            }
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

/// Register cross-file `DefIds` from sibling module imports into the Go codegen.
///
/// When a package file imports another local file, the type checker assigns
/// `DefIds` for the exported types via the file interface system. The Go name
/// catalog only collects local HIR declarations, so those cross-file `DefIds`
/// would be unresolvable during code generation. The flattened Go package
/// declares the imported items in a sibling file, so registering the exported
/// member name is enough for the reference to resolve.
///
/// The interface's member is the consumer-locale surface spelling; the
/// exporting unit declares the canonical spelling, so every registration maps
/// through [`Resolver::imported_export_canonical_name`] exactly as the Rust
/// backend does. Library items keep their library-registry spelling: the
/// bindingless walk skips them so a file interface cannot shadow `libraries`.
#[cfg(feature = "hir-go")]
fn register_go_cross_file_defids(
    r#gen: &mut go::GoCodegen<'_>,
    hir: &HirModule,
    resolver: &crate::semantic::Resolver,
    libraries: &crate::hir::LibraryRegistry,
) {
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
                let canonical = resolver.imported_export_canonical_name(binding, export.member);
                r#gen.register_external_def(export.def_id, canonical);
            }
        }
    }
    for namespace in resolver.file_interface_namespaces() {
        for export in resolver.imported_file_type_exports(namespace) {
            if libraries.items.contains_key(&export.def_id) {
                continue;
            }
            let canonical = resolver.imported_export_canonical_name(namespace, export.member);
            r#gen.register_external_def(export.def_id, canonical);
        }
    }
}

#[cfg(feature = "hir-go")]
fn generate_go_from_analyzed(analysis: &dyn CodegenAnalysis, hir: &HirModule) -> Result<crate::Output, CodegenError> {
    /// Imported functions that declare a `⇥` channel, keyed by (module path def,
    /// member) with the declared error type. The file interface carries each
    /// member's signature; the facts table only covers this unit's own functions.
    fn go_imported_failable_members(
        analysis: &dyn CodegenAnalysis,
        hir: &HirModule,
    ) -> handroll::FxHashMap<(crate::hir::DefId, crate::lexer::Symbol), radix_types::TypeId> {
        use crate::hir::HirExpressionKind;
        use crate::hir::visit::HirVisitor;
        struct Reads<'u> {
            analysis: &'u dyn CodegenAnalysis,
            found: handroll::FxHashMap<(crate::hir::DefId, crate::lexer::Symbol), radix_types::TypeId>,
        }
        impl Reads<'_> {
            fn note(&mut self, object: &crate::hir::HirExpression, member: crate::lexer::Symbol) {
                if let HirExpressionKind::Path(module) = &object.kind
                    && let Some(namespace) = self.analysis.resolver().get_symbol(*module)
                    && namespace.kind == crate::semantic::SymbolKind::Module
                    && let Some(err) = self
                        .analysis
                        .resolver()
                        .namespace_file_function_canonical(namespace.name, member)
                        .and_then(|sig| sig.err)
                {
                    self.found.insert((*module, member), err);
                }
            }
        }
        impl HirVisitor for Reads<'_> {
            fn visit_expr(&mut self, expr: &crate::hir::HirExpression) {
                match &expr.kind {
                    HirExpressionKind::Call(callee, ..) => {
                        if let HirExpressionKind::Field(object, member) = &callee.kind {
                            self.note(object, *member);
                        }
                    }
                    HirExpressionKind::MethodCall(receiver, method, ..) => self.note(receiver, *method),
                    _ => {}
                }
                crate::hir::visit::walk_expr(self, expr);
            }
        }
        let mut reads = Reads { analysis, found: handroll::FxHashMap::default() };
        crate::hir::visit::walk_program(&mut reads, hir);
        reads.found
    }

    let mut r#gen = go::GoCodegen::new(hir, analysis.types(), analysis.interner());
    r#gen.set_display_tokens(display_tokens_for_module(analysis));
    r#gen.set_failable_facts(&failable_facts(analysis.function_facts()));
    r#gen.set_ordo_order_rows(&analysis.tensor_edge_policies().ordo_order.comparisons);
    r#gen.set_library_registry(analysis.libraries());
    r#gen.set_imported_constants(go_imported_constants(analysis, hir));
    r#gen.set_imported_generators(go_imported_generators(analysis, hir));
    r#gen.set_imported_failable_members(go_imported_failable_members(analysis, hir));
    register_go_cross_file_defids(&mut r#gen, hir, analysis.resolver(), analysis.libraries());
    let mut statics = handroll::FxHashMap::default();
    let mut imported_structs = Vec::new();
    for (def_id, fields, _, _) in analysis.resolver().imported_file_structs() {
        let mut instance_fields = Vec::with_capacity(fields.len());
        for field in fields {
            if field.modifier == Some(radix_syntax::FieldModifier::Const) {
                if let Some(init) = field.init {
                    statics.insert((def_id, field.name), init);
                }
                continue;
            }
            // The resolver rebuilt this default in the importer's symbol and
            // type space; the declaring file's own expression indexes a
            // different interner and must never be read here.
            instance_fields.push(go::ImportedStructField {
                name: field.name,
                ty: field.ty,
                sponte: field.optional,
                init: field.init,
            });
        }
        imported_structs.push((def_id, instance_fields));
    }
    r#gen.set_imported_static_fields(statics);
    r#gen.set_imported_structs(imported_structs);
    let output = r#gen.generate(hir, analysis.types(), analysis.interner())?;
    Ok(crate::Output::Go(crate::GoOutput { code: output.code }))
}

/// Folded imported module constants read as `module.NAME` in `hir`.
///
/// Go output is one `main` package, so a provider module has no Go namespace:
/// each such read prints the folded literal the file interface carries (the
/// same value source as the MIR lowering's imported-constant map).
#[cfg(feature = "hir-go")]
fn go_imported_constants(
    analysis: &dyn CodegenAnalysis,
    hir: &HirModule,
) -> handroll::FxHashMap<(crate::hir::DefId, crate::lexer::Symbol), go::ImportedConst> {
    use crate::file_interface::InterfaceConstValue;
    use crate::hir::HirExpressionKind;
    use crate::hir::visit::HirVisitor;
    struct Reads<'u> {
        analysis: &'u dyn CodegenAnalysis,
        found: handroll::FxHashMap<(crate::hir::DefId, crate::lexer::Symbol), go::ImportedConst>,
    }
    impl HirVisitor for Reads<'_> {
        fn visit_expr(&mut self, expr: &crate::hir::HirExpression) {
            if let HirExpressionKind::Field(object, member) = &expr.kind
                && let HirExpressionKind::Path(module) = &object.kind
                && let Some(namespace) = self.analysis.resolver().get_symbol(*module)
                && namespace.kind == crate::semantic::SymbolKind::Module
                && let Some(value) = self
                    .analysis
                    .resolver()
                    .namespace_file_const_value_canonical(namespace.name, *member)
            {
                let value = match value {
                    InterfaceConstValue::Int(v) => go::ImportedConst::Int(*v),
                    InterfaceConstValue::Float(v) => go::ImportedConst::Float(*v),
                    InterfaceConstValue::Bool(v) => go::ImportedConst::Bool(*v),
                    InterfaceConstValue::Text(v) => go::ImportedConst::Text(v.clone()),
                    // The Go emitter inlines scalars only; a list keeps the
                    // package-linked read.
                    InterfaceConstValue::List(_) => return crate::hir::visit::walk_expr(self, expr),
                };
                self.found.insert((*module, *member), value);
            }
            crate::hir::visit::walk_expr(self, expr);
        }
    }
    let mut reads = Reads { analysis, found: handroll::FxHashMap::default() };
    crate::hir::visit::walk_program(&mut reads, hir);
    reads.found
}

/// Imported lazy generator callees called as `module.member(..)` in `hir`.
#[cfg(feature = "hir-go")]
fn go_imported_generators(
    analysis: &dyn CodegenAnalysis,
    hir: &HirModule,
) -> handroll::FxHashMap<(crate::hir::DefId, crate::lexer::Symbol), radix_types::TypeId> {
    use crate::hir::HirExpressionKind;
    use crate::hir::visit::HirVisitor;
    struct Reads<'u> {
        analysis: &'u dyn CodegenAnalysis,
        found: handroll::FxHashMap<(crate::hir::DefId, crate::lexer::Symbol), radix_types::TypeId>,
    }
    impl HirVisitor for Reads<'_> {
        fn visit_expr(&mut self, expr: &crate::hir::HirExpression) {
            if let HirExpressionKind::Call(callee, ..) = &expr.kind
                && let HirExpressionKind::Field(object, member) = &callee.kind
                && let HirExpressionKind::Path(module) = &object.kind
                && let Some(namespace) = self.analysis.resolver().get_symbol(*module)
                && namespace.kind == crate::semantic::SymbolKind::Module
                && let Some(sig) = self
                    .analysis
                    .resolver()
                    .namespace_file_function_canonical(namespace.name, *member)
                && sig.is_generator
                && !sig.is_async
                && sig.err.is_none()
                && let radix_types::Type::Array(elem) = self.analysis.types().get(sig.ret)
            {
                self.found.insert((*module, *member), *elem);
            }
            crate::hir::visit::walk_expr(self, expr);
        }
    }
    let mut reads = Reads { analysis, found: handroll::FxHashMap::default() };
    crate::hir::visit::walk_program(&mut reads, hir);
    reads.found
}

#[cfg(not(feature = "hir-go"))]
fn generate_go_from_analyzed(_analysis: &dyn CodegenAnalysis, _hir: &HirModule) -> Result<crate::Output, CodegenError> {
    Err(unavailable_target_error(Target::HirGo))
}

#[cfg(feature = "hir-swift")]
fn generate_swift_from_analyzed(
    analysis: &dyn CodegenAnalysis,
    hir: &HirModule,
    output_mode: OutputMode,
    module_name: Option<&str>,
) -> Result<crate::Output, CodegenError> {
    let mut r#gen = swift::SwiftCodegen::new(hir, analysis.types(), analysis.interner());
    r#gen.set_display_tokens(display_tokens_for_module(analysis));
    r#gen.set_ordo_order_rows(&analysis.tensor_edge_policies().ordo_order.comparisons);
    r#gen.set_failable_facts(&failable_facts(analysis.function_facts()));
    r#gen.set_output_mode(
        match output_mode {
            OutputMode::Application => swift::OutputMode::Application,
            OutputMode::Library => swift::OutputMode::Library,
        },
        module_name,
    );
    let output = r#gen.generate(hir, analysis.types(), analysis.interner())?;
    Ok(crate::Output::Swift(crate::SwiftOutput { code: output.code }))
}

#[cfg(not(feature = "hir-swift"))]
fn generate_swift_from_analyzed(
    _analysis: &dyn CodegenAnalysis,
    _hir: &HirModule,
    _output_mode: OutputMode,
    _module_name: Option<&str>,
) -> Result<crate::Output, CodegenError> {
    Err(unavailable_target_error(Target::HirSwift))
}

#[cfg(feature = "hir-python")]
fn generate_python_from_analyzed(
    analysis: &dyn CodegenAnalysis,
    hir: &HirModule,
) -> Result<crate::Output, CodegenError> {
    let mut r#gen = python::PythonCodegen::new(hir, analysis.types(), analysis.interner());
    r#gen.set_display_tokens(display_tokens_for_module(analysis));
    r#gen.set_failable_facts(&failable_facts(analysis.function_facts()));
    r#gen.set_ordo_order_rows(&analysis.tensor_edge_policies().ordo_order.comparisons);
    let output = r#gen.generate(hir, analysis.types(), analysis.interner())?;
    Ok(crate::Output::Python(crate::PythonOutput { code: output.code }))
}

#[cfg(not(feature = "hir-python"))]
fn generate_python_from_analyzed(
    _analysis: &dyn CodegenAnalysis,
    _hir: &HirModule,
) -> Result<crate::Output, CodegenError> {
    Err(unavailable_target_error(Target::HirPython))
}

#[cfg(feature = "hir-cobol")]
fn generate_cobol_from_analyzed(
    analysis: &dyn CodegenAnalysis,
    hir: &HirModule,
) -> Result<crate::Output, CodegenError> {
    let mut r#gen = cobol::CobolCodegen::new(hir, analysis.interner());
    r#gen.set_display_tokens(analysis.display_tokens().clone());
    let output = r#gen.generate(hir, analysis.types(), analysis.interner())?;
    Ok(crate::Output::Cobol(crate::CobolOutput { code: output.code }))
}

#[cfg(not(feature = "hir-cobol"))]
fn generate_cobol_from_analyzed(
    _analysis: &dyn CodegenAnalysis,
    _hir: &HirModule,
) -> Result<crate::Output, CodegenError> {
    Err(unavailable_target_error(Target::HirCobol))
}

#[cfg(feature = "hir-haskell")]
fn generate_haskell_from_analyzed(
    analysis: &dyn CodegenAnalysis,
    hir: &HirModule,
) -> Result<crate::Output, CodegenError> {
    let mut r#gen = haskell::HaskellCodegen::new(hir, analysis.interner());
    r#gen.set_display_tokens(display_tokens_for_module(analysis));
    r#gen.set_failable_facts(&failable_facts(analysis.function_facts()));
    let output = r#gen.generate(hir, analysis.types(), analysis.interner())?;
    Ok(crate::Output::Haskell(crate::HaskellOutput { code: output.code }))
}

#[cfg(not(feature = "hir-haskell"))]
fn generate_haskell_from_analyzed(
    _analysis: &dyn CodegenAnalysis,
    _hir: &HirModule,
) -> Result<crate::Output, CodegenError> {
    Err(unavailable_target_error(Target::HirHaskell))
}

#[cfg(feature = "hir-pascal")]
fn generate_pascal_from_analyzed(
    analysis: &dyn CodegenAnalysis,
    hir: &HirModule,
) -> Result<crate::Output, CodegenError> {
    let mut r#gen = pascal::PascalCodegen::new(hir, analysis.interner());
    r#gen.set_display_tokens(analysis.display_tokens().clone());
    let output = r#gen.generate(hir, analysis.types(), analysis.interner())?;
    Ok(crate::Output::Pascal(crate::PascalOutput { code: output.code }))
}

#[cfg(not(feature = "hir-pascal"))]
fn generate_pascal_from_analyzed(
    _analysis: &dyn CodegenAnalysis,
    _hir: &HirModule,
) -> Result<crate::Output, CodegenError> {
    Err(unavailable_target_error(Target::HirPascal))
}

#[cfg(feature = "hir-fortran")]
fn generate_fortran_from_analyzed(
    analysis: &dyn CodegenAnalysis,
    hir: &HirModule,
) -> Result<crate::Output, CodegenError> {
    let mut r#gen = fortran::FortranCodegen::new(hir, analysis.interner());
    r#gen.set_display_tokens(analysis.display_tokens().clone());
    let output = r#gen.generate(hir, analysis.types(), analysis.interner())?;
    Ok(crate::Output::Fortran(crate::FortranOutput { code: output.code }))
}

#[cfg(not(feature = "hir-fortran"))]
fn generate_fortran_from_analyzed(
    _analysis: &dyn CodegenAnalysis,
    _hir: &HirModule,
) -> Result<crate::Output, CodegenError> {
    Err(unavailable_target_error(Target::HirFortran))
}

#[cfg(feature = "hir-odin")]
fn generate_odin_from_analyzed(analysis: &dyn CodegenAnalysis, hir: &HirModule) -> Result<crate::Output, CodegenError> {
    let mut r#gen = odin::OdinCodegen::new(hir, analysis.interner());
    r#gen.set_display_tokens(analysis.display_tokens().clone());
    let output = r#gen.generate(hir, analysis.types(), analysis.interner())?;
    Ok(crate::Output::Odin(crate::OdinOutput { code: output.code }))
}

#[cfg(not(feature = "hir-odin"))]
fn generate_odin_from_analyzed(
    _analysis: &dyn CodegenAnalysis,
    _hir: &HirModule,
) -> Result<crate::Output, CodegenError> {
    Err(unavailable_target_error(Target::HirOdin))
}

#[cfg(feature = "hir-c")]
fn generate_c_from_analyzed(analysis: &dyn CodegenAnalysis, hir: &HirModule) -> Result<crate::Output, CodegenError> {
    let mut r#gen = c::CCodegen::new(hir, analysis.interner());
    r#gen.set_display_tokens(analysis.display_tokens().clone());
    let output = r#gen.generate(hir, analysis.types(), analysis.interner())?;
    Ok(crate::Output::C(crate::COutput { code: output.code }))
}

#[cfg(not(feature = "hir-c"))]
fn generate_c_from_analyzed(_analysis: &dyn CodegenAnalysis, _hir: &HirModule) -> Result<crate::Output, CodegenError> {
    Err(unavailable_target_error(Target::HirC))
}

/// Emit each distinct file-namespace provider binding as a module, mapping
/// over the analyzed provider units carried on the driver's output
/// (ARCH-U4 / S01-3).
///
/// The units were analyzed once per driver run where the import-contract walk
/// already parses each target, under the contract codegen needs (bodyless
/// functions + library-registry defaults). This function no longer re-reads
/// providers from disk, re-runs the front half, or resolves imports
/// CWD-relatively: the codegen tree's last filesystem dependency is gone and
/// the O(consumers × providers) re-analysis ends.
#[cfg(feature = "hir-rust")]
fn emit_provider_modules(
    analysis: &dyn CodegenAnalysis,
    emitted_tensor_policy_providers: &mut handroll::FxHashSet<String>,
) -> Result<Vec<(String, String, std::collections::BTreeSet<String>)>, CodegenError> {
    use std::collections::BTreeMap;

    let mut modules = BTreeMap::new();
    map_provider_modules(analysis, emitted_tensor_policy_providers, &mut modules)?;
    Ok(modules
        .into_iter()
        .map(|(name, (code, failable))| (name, code, failable))
        .collect())
}

/// Walk one analyzed unit's imports and emit every distinct file-namespace
/// provider binding carried for it as a Rust module.
///
/// The same walk serves the consumer unit and each carried provider: the
/// provider's own `nested` view supplies its nested namespace imports
/// (`norma:valor` → `norma:chorda` emit as sibling modules), and the
/// `modules` map skips providers already emitted (a module diamond; the
/// insert follows the recursion, so it does not brake a cycle). A binding
/// with a file interface but no
/// carried unit fails closed — by construction the same contract walk
/// installed the interface and the carriage, so this only fires for a mid-run
/// inconsistency (or a cyclic import that interrupted carriage), where the
/// previous disk-based path failed too.
#[cfg(feature = "hir-rust")]
fn map_provider_modules(
    analysis: &dyn CodegenAnalysis,
    emitted_tensor_policy_providers: &mut handroll::FxHashSet<String>,
    modules: &mut std::collections::BTreeMap<String, (String, std::collections::BTreeSet<String>)>,
) -> Result<(), CodegenError> {
    use crate::hir::HirItemKind;

    for item in &analysis.hir().items {
        let HirItemKind::Import(import) = &item.kind else {
            continue;
        };
        let import_path = analysis.interner().resolve(import.path);
        for import_item in &import.items {
            let binding = import_item.alias.unwrap_or(import_item.name);
            if !analysis.resolver().namespace_has_file_interface(binding) {
                continue;
            }
            let module_name = analysis.interner().resolve(binding).replace('-', "_");
            if modules.contains_key(&module_name) {
                continue;
            }
            let provider = match analysis.provider(import_path) {
                None => {
                    return Err(CodegenError {
                        message: format!(
                            "cannot resolve provider import `{import_path}` for Rust module emission: \
                             no analyzed provider unit is carried for this import"
                        ),
                        args: vec![
                            DiagnosticArg::new("issue", "provider_import_resolution_failed"),
                            DiagnosticArg::new("import_path", import_path),
                        ],
                    });
                }
                Some(crate::codegen::input::CodegenProvider::Failed { message }) => {
                    return Err(CodegenError {
                        message: format!("provider module `{module_name}` {message}"),
                        args: vec![
                            DiagnosticArg::new("issue", "provider_module_analysis_failed"),
                            DiagnosticArg::new("module", module_name.clone()),
                        ],
                    });
                }
                Some(crate::codegen::input::CodegenProvider::Unit(provider)) => provider,
            };
            if provider
                .diagnostics()
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
            // exist. The `contains_key` guard above only skips modules already
            // emitted; a cyclic import ends at the missing-carriage error.
            map_provider_modules(&*provider, emitted_tensor_policy_providers, modules)?;
            emit_provider_module_code(&*provider, &module_name, emitted_tensor_policy_providers, modules)?;
        }
    }

    Ok(())
}

#[cfg(feature = "hir-rust")]
fn emit_provider_module_code(
    provider: &dyn crate::codegen::input::CodegenAnalysis,
    module_name: &str,
    emitted_tensor_policy_providers: &mut handroll::FxHashSet<String>,
    modules: &mut std::collections::BTreeMap<String, (String, std::collections::BTreeSet<String>)>,
) -> Result<(), CodegenError> {
    let namespace_info = rust::build_file_import_namespaces(
        provider.hir(),
        provider.interner(),
        &rust::ResolverImportsBridge(provider.resolver()),
    );
    let callback_identities = imported_tensor_edge_policy_callback_identities(provider.resolver(), provider.interner());
    let mut policy_identity_codegen =
        rust::RustCodegen::new_with_types(provider.hir(), provider.interner(), provider.types());
    policy_identity_codegen.set_imported_namespace_type_paths(namespace_info.type_paths.clone());
    let mut provider_emit_rows = Vec::new();
    for provider_impl in &provider.tensor_edge_policies().provider_impls {
        let Some(key) = policy_identity_codegen.tensor_edge_policy_provider_key(
            provider_impl,
            provider.types(),
            Some(module_name),
        )?
        else {
            continue;
        };
        if emitted_tensor_policy_providers.insert(key) {
            provider_emit_rows.push(*provider_impl);
        }
    }
    let shape_obligations = rust::rust_shape_obligations(provider.shape_obligations());
    // Declaration-only `@ nondum` provider functions have nothing to emit.
    let provider_hir = provider
        .hir()
        .without_unavailable_declarations(provider.interner());
    let module_out = radix_hir_rust::generate_with_library_registry_test_selection_and_imports(
        radix_hir_rust::ModuleGenerationRequest {
            hir: &provider_hir,
            types: provider.types(),
            interner: provider.interner(),
            libraries: &radix_hir::LibraryRegistry::default(),
            test_selection: None,
            function_facts: None,
            shape_obligations: Some(&shape_obligations),
            tensor_edge_policies: Some(provider.tensor_edge_policies()),
            tensor_edge_policy_provider_impls_to_emit: Some(&provider_emit_rows),
            tensor_edge_policy_callback_identities: &callback_identities,
            module_mode: true,
            cli_program: None,
            imported_function_params: None,
            imported_namespace_info: Some(namespace_info),
            gpu_builtins: &[],
            field_name_policy: radix_hir_rust::RustFieldNamePolicy::default(),
            static_route_table: None,
            display_tokens: None,
        },
    )?;
    // The provider's emitted signatures (not its Faber declarations)
    // decide importer call-site unwrapping: `ad`-verb functions emit
    // `Result` even without a declared `⇥` channel.
    let failable_exports = radix_hir_rust::provider_failable_export_names(provider.hir(), provider.interner());
    modules.insert(module_name.to_owned(), (module_out.code, failable_exports));
    Ok(())
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
    #[cfg_attr(not(feature = "hir-rust"), allow(unused_variables))]
    display_tokens: &radix_codegen_shared::names::DisplayTokens,
) -> Result<crate::RustOutput, CodegenError> {
    #[cfg(not(feature = "hir-rust"))]
    {
        return Err(unavailable_target_error(Target::HirRust));
    }
    #[cfg(feature = "hir-rust")]
    {
        radix_codegen_shared::reject_hir_errors(hir)?;
        radix_codegen_shared::reject_bits_type(types, Target::HirRust)?;
        radix_codegen_shared::reject_ledger_type(types, Target::HirRust)?;
        reject_leaf_routes_in_hir(hir, interner, Target::HirRust)?;
        reject_rust_unsupported_types(types)?;
        let hir = hir.without_unavailable_declarations(interner);
        let hir = &*hir;
        let ir = cli_ir::to_cli_ir(cli_program);
        let mut codegen = rust::RustCodegen::new_with_types(hir, interner, types);
        codegen.set_display_tokens(display_tokens.clone());
        let output = codegen.generate_cli(hir, types, &ir)?;
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
    #[cfg_attr(not(feature = "hir-go"), allow(unused_variables))]
    display_tokens: radix_codegen_shared::names::DisplayTokens,
) -> Result<crate::GoOutput, CodegenError> {
    #[cfg(feature = "hir-go")]
    {
        radix_codegen_shared::reject_unlowered_value_types(types, Target::HirGo)?;
        reject_leaf_routes_in_hir(hir, interner, Target::HirGo)?;
        let hir = hir.without_unavailable_declarations(interner);
        go::generate_go_cli(&hir, types, interner, cli_program, display_tokens)
    }
    #[cfg(not(feature = "hir-go"))]
    {
        Err(unavailable_target_error(Target::HirGo))
    }
}

/// Emit TypeScript CLI product code for the narrow single-command slice.
///
/// # Errors
///
/// Returns [`CodegenError`] if the HIR contains error recovery expressions, a
/// target-unsupported type or leaf, or the CLI surface is beyond the narrow
/// slice (options, subcommands, non-text operands, operand defaults, or a
/// non-fixed exit).
pub fn generate_ts_cli(
    #[cfg_attr(not(feature = "hir-ts"), allow(unused_variables))] hir: &HirModule,
    #[cfg_attr(not(feature = "hir-ts"), allow(unused_variables))] types: &TypeTable,
    #[cfg_attr(not(feature = "hir-ts"), allow(unused_variables))] interner: &Interner,
    #[cfg_attr(not(feature = "hir-ts"), allow(unused_variables))] cli_program: &crate::cli::CliProgram,
) -> Result<crate::TypeScriptOutput, CodegenError> {
    #[cfg(feature = "hir-ts")]
    {
        radix_codegen_shared::reject_hir_errors(hir)?;
        radix_codegen_shared::reject_unlowered_value_types(types, Target::HirTypeScript)?;
        reject_leaf_routes_in_hir(hir, interner, Target::HirTypeScript)?;
        let ir = cli_ir::to_cli_ir(cli_program);
        let hir = hir.without_unavailable_declarations(interner);
        let output = ts::TsCodegen::new(&hir, types, interner).generate_cli(&hir, types, interner, &ir)?;
        Ok(crate::TypeScriptOutput { code: output.code })
    }
    #[cfg(not(feature = "hir-ts"))]
    {
        Err(unavailable_target_error(Target::HirTypeScript))
    }
}

/// Emit Python CLI product code for the narrow single-command slice.
///
/// # Errors
///
/// Returns [`CodegenError`] if the HIR contains error recovery expressions, a
/// target-unsupported type or leaf, or the CLI surface is beyond the narrow
/// slice (options, subcommands, non-text operands, operand defaults, or a
/// non-fixed exit).
pub fn generate_python_cli(
    #[cfg_attr(not(feature = "hir-python"), allow(unused_variables))] hir: &HirModule,
    #[cfg_attr(not(feature = "hir-python"), allow(unused_variables))] types: &TypeTable,
    #[cfg_attr(not(feature = "hir-python"), allow(unused_variables))] interner: &Interner,
    #[cfg_attr(not(feature = "hir-python"), allow(unused_variables))] cli_program: &crate::cli::CliProgram,
) -> Result<crate::PythonOutput, CodegenError> {
    #[cfg(feature = "hir-python")]
    {
        radix_codegen_shared::reject_hir_errors(hir)?;
        radix_codegen_shared::reject_unlowered_value_types(types, Target::HirPython)?;
        reject_leaf_routes_in_hir(hir, interner, Target::HirPython)?;
        let ir = cli_ir::to_cli_ir(cli_program);
        let hir = hir.without_unavailable_declarations(interner);
        let output = python::PythonCodegen::new(&hir, types, interner).generate_cli(&hir, types, interner, &ir)?;
        Ok(crate::PythonOutput { code: output.code })
    }
    #[cfg(not(feature = "hir-python"))]
    {
        Err(unavailable_target_error(Target::HirPython))
    }
}

/// Collect `ad` routes from this module, imported handlers, and carried
/// provider units (Norma wrappers live there on single-file emit).
///
/// Also the whole-package `radix-program` build paths (`wasm.rs`, `llvm.rs`)
/// call this through the unit's [`crate::codegen::input::CodegenAnalysis`]
/// implementation (the analyzed-module carrier coerces).
///
/// # Errors
///
/// Returns a [`CodegenError`] (`leaf_unsupported_on_target`) when a collected
/// route names a leaf the `target` does not support.
pub fn reject_unsupported_leaves(
    analysis: &dyn crate::codegen::input::CodegenAnalysis,
    target: Target,
) -> Result<(), CodegenError> {
    let mut routes = Vec::new();
    collect_analysis_ad_routes(analysis, &mut routes);
    radix_codegen_shared::reject_unsupported_leaf(&routes, target)
}

fn collect_analysis_ad_routes(analysis: &dyn CodegenAnalysis, routes: &mut Vec<String>) {
    collect_module_ad_routes(analysis.hir(), analysis.interner(), analysis.resolver(), routes);
    for key in analysis.provider_keys() {
        if let Some(CodegenProvider::Unit(unit)) = analysis.provider(&key) {
            collect_analysis_ad_routes(&*unit, routes);
        }
    }
}

fn collect_module_ad_routes(
    hir: &HirModule,
    interner: &Interner,
    resolver: &crate::semantic::Resolver,
    routes: &mut Vec<String>,
) {
    radix_codegen_shared::collect_ad_routes(hir, interner, routes);
    for imported in resolver.imported_ad_routes() {
        let route = interner.resolve(imported.route);
        if !routes.iter().any(|existing| existing == route) {
            routes.push(route.to_owned());
        }
    }
}

fn reject_leaf_routes_in_hir(hir: &HirModule, interner: &Interner, target: Target) -> Result<(), CodegenError> {
    let mut routes = Vec::new();
    radix_codegen_shared::collect_ad_routes(hir, interner, &mut routes);
    radix_codegen_shared::reject_unsupported_leaf(&routes, target)
}

#[cfg(feature = "hir-rust")]
fn reject_rust_unsupported_types(types: &TypeTable) -> Result<(), CodegenError> {
    // The binding-probe and CLI entries reach here without `generate_output`.
    radix_codegen_shared::reject_unbounded_integer(types, Target::HirRust)?;
    for index in 0..types.type_count() {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "TypeId is u32 by design; type_count is bounded by u32::MAX"
        )]
        let ty = TypeId(index as u32);
        // N7e: a float cell is the bare float of its width, so a half-width
        // cell fails closed with the bare half float's named issue.
        if let Type::SizedNumeric(Primitive::Fractus, width) | Type::PolicyFloat(_, width) = types.get(ty)
            && matches!(width, NumericWidth::F16 | NumericWidth::BF16)
        {
            let issue = match width {
                NumericWidth::F16 => "rust_target_fractus_f16_unsupported",
                _ => "rust_target_fractus_bf16_unsupported",
            };
            return Err(CodegenError {
                message: format!(
                    "Rust target does not support {} until a half-float runtime representation exists",
                    width.faber_marker()
                ),
                args: vec![DiagnosticArg::new("issue", issue)],
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
