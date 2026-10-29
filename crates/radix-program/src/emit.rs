//! Package HIR source-emission assembly (RP4).
//!
//! The emission half of `crates/faber/src/package/compile.rs` moved here:
//! target dispatch, artifact-plan checks, Rust crate assembly entry points,
//! and the package runtime-plan entry. Go package generation stays faber-side
//! because its toolchain module is product-owned; Wasm package generation is
//! compiler-owned by [`crate::wasm`]. Nothing here reads the ambient
//! environment or writes through hidden state.
//!
//! `check_package` remains an analysis entry point; Faber supplies only the
//! product-owned resolver, locale, manifest, and Go toolchain inputs.

use std::path::Path;

use radix_module::CompileResult;
use radix_module::codegen::Target;
use radix_module::diagnostics::Diagnostic;
use radix_module::driver::Config;

use crate::analyze::{
    AnalysisSeams, AnalyzedProgram, analyze_package_spec, effective_package_config,
    package_root_for_input,
};
use crate::artifact_plan::{ArtifactPlan, plan_package};
use crate::discovery::discover_package;
use crate::frontmatter::RustTestSelection;
use crate::library::LibraryResolver;
use crate::test_source_filter::TestSourceFilter;

/// Result of a package compile that explicitly carries the Go multi-module
/// file collection produced by the Go assembly path (G6 GO3/GO4), and the
/// Python multi-file collection produced by Python package assembly.
///
/// RP4: moved from `crates/faber/src/package/compile.rs`. The module files are
/// outputs of their compile paths — never hidden thread-local state (FBR-P2-003)
/// — so failed, repeated, nested, and concurrent compile calls cannot exchange
/// module output.
pub struct PackageCompileResult {
    pub compile_result: CompileResult,
    /// Go multi-module files `(file name, file body)` for non-entry units,
    /// populated only for `Target::HirGo` packages that reached codegen.
    #[cfg(feature = "hir-go")]
    pub go_modules: Vec<(String, String)>,
    /// Python package files `(relative path, file body)` for units that reached
    /// codegen. This output is explicit, never hidden thread-local state.
    #[cfg(feature = "hir-python")]
    pub python_modules: Vec<(String, String)>,
}

/// Go package-emission seam: faber's `generate_package_go_result` (uses the
/// product `go_build` module). `None`-free under `hir-go`; the seam is the
/// explicit toolchain-bound input.
#[cfg(feature = "hir-go")]
pub type GoPackageEmit = fn(&AnalyzedProgram, &Path) -> PackageCompileResult;

/// TypeScript package-emission seam: faber's `generate_package_ts_result`.
/// TypeScript is a single-stream target (no sibling module files), so the seam
/// returns [`CompileResult`] rather than [`PackageCompileResult`].
#[cfg(feature = "hir-ts")]
pub type TsPackageEmit = fn(&AnalyzedProgram, &Path) -> CompileResult;

/// Faber-supplied package-emission seams (RP4).
///
/// Go and TypeScript emission remain product/toolchain inputs; analysis
/// supplies the shared [`AnalysisSeams`]. No ambient reads happen inside
/// `radix-program`.
#[derive(Clone, Copy)]
pub struct EmissionSeams {
    pub analysis: AnalysisSeams,
    #[cfg(feature = "hir-go")]
    pub emit_go: GoPackageEmit,
    #[cfg(feature = "hir-ts")]
    pub emit_ts: TsPackageEmit,
}

/// Failed compile: no output, and no Go modules.
#[must_use]
pub fn compile_failure(diagnostics: Vec<Diagnostic>) -> PackageCompileResult {
    PackageCompileResult {
        compile_result: CompileResult {
            output: None,
            diagnostics,
        },
        #[cfg(feature = "hir-go")]
        go_modules: Vec::new(),
        #[cfg(feature = "hir-python")]
        python_modules: Vec::new(),
    }
}

fn finalize_package_compile_result(
    mut result: CompileResult,
    warn_policy: &radix_module::driver::WarnPolicy,
) -> CompileResult {
    radix_module::apply_warn_policy(&mut result.diagnostics, warn_policy);
    if result.diagnostics.iter().any(Diagnostic::is_error) {
        result.output = None;
    }
    result
}

/// Fail closed with the planner's rejection reason for an unsupported target.
fn unsupported_plan_failure(plan: ArtifactPlan, input: &Path) -> PackageCompileResult {
    compile_failure(vec![
        crate::package_diagnostic_error(
            plan.rejection
                .unwrap_or_else(|| "package compilation does not support this target".to_owned()),
        )
        .with_file(input.display().to_string())
        .with_arg("issue", "package_target_unsupported")
        .with_arg("target", plan.target),
    ])
}

/// Compile a package source graph into one backend output.
///
/// Package compilation currently targets Rust only because it must assemble
/// multiple modules and generated CLI surfaces into a single crate-shaped
/// backend result. Unsupported targets are reported as diagnostics instead of
/// falling back to single-file compilation.
///
/// RP4: moved from `crates/faber/src/package/compile.rs`; the seam bundle
/// carries only product-owned analysis and Go toolchain inputs.
#[must_use]
pub fn compile_package(config: &Config, input: &Path, seams: &EmissionSeams) -> CompileResult {
    fold_go_modules_into_output(compile_package_result(config, input, seams))
}

/// Fold the Go multi-module collection into the single output stream.
///
/// `faber build --target go` consumes [`PackageCompileResult::go_modules`]
/// directly and writes one file per unit. The single-stream emit path
/// (`faber emit -t go`, and the `faber corpus --target go` recipe that
/// captures stdout into `main.go`) has no place for the sibling files, so
/// their declarations merge into the entry source instead.
#[cfg(feature = "hir-go")]
fn fold_go_modules_into_output(mut result: PackageCompileResult) -> CompileResult {
    if result.go_modules.is_empty() {
        return result.compile_result;
    }
    match result.compile_result.output.take() {
        Some(radix_module::Output::Go(go)) => {
            match radix_module::codegen::go::merge_package_modules(&go.code, &result.go_modules) {
                Ok(code) => {
                    result.compile_result.output =
                        Some(radix_module::Output::Go(radix_module::GoOutput { code }));
                }
                Err(err) => {
                    result.compile_result.diagnostics.push(
                        crate::package_diagnostic_error(err.to_string())
                            .with_arg("issue", "package_go_merge_shape")
                            .with_arg("target", "go"),
                    );
                }
            }
        }
        output => result.compile_result.output = output,
    }
    result.compile_result
}

/// Without `hir-go` there is no Go output to fold.
#[cfg(not(feature = "hir-go"))]
fn fold_go_modules_into_output(result: PackageCompileResult) -> CompileResult {
    result.compile_result
}

/// Compile a package and keep the full Go result envelope. Used by Faber's
/// Go package adapter; `compile_package` returns only the compiler result.
#[must_use]
pub fn compile_package_result(
    config: &Config,
    input: &Path,
    seams: &EmissionSeams,
) -> PackageCompileResult {
    let mut result = compile_package_internal(config, input, None, false, None, seams);
    result.compile_result =
        finalize_package_compile_result(result.compile_result, &config.warn_policy);
    result
}

/// Compile a package while forwarding a Rust test-selection policy to codegen.
///
/// Historical Rust harness path: still used by package integration tests that
/// assert on generated Rust. Product `faber test` uses the MIR runner instead.
///
/// RP4: moved from `crates/faber/src/package/compile.rs`.
#[must_use]
pub fn compile_package_with_test_selection(
    config: &Config,
    input: &Path,
    test_selection: Option<&RustTestSelection>,
    seams: &EmissionSeams,
) -> CompileResult {
    compile_package_with_test_options(config, input, test_selection, None, seams)
}

/// Like [`compile_package_with_test_selection`], with optional path filters for
/// which `*.proba` files are loaded (`--include` / `--exclude` on `faber test`).
///
/// RP4: moved from `crates/faber/src/package/compile.rs`.
#[must_use]
pub fn compile_package_with_test_options(
    config: &Config,
    input: &Path,
    test_selection: Option<&RustTestSelection>,
    proba_filter: Option<&TestSourceFilter>,
    seams: &EmissionSeams,
) -> CompileResult {
    let mut result =
        compile_package_internal(config, input, test_selection, true, proba_filter, seams);
    result.compile_result =
        finalize_package_compile_result(result.compile_result, &config.warn_policy);
    result.compile_result
}

/// Package runtime-plan entry (RP4).
///
/// The product `RustRuntimePlan` shape and its dispatch/provider policy are
/// faber-owned; this entry runs the shared analysis spine and hands the
/// analyzed package to the caller-supplied plan constructor (explicit input,
/// no ambient seams). `R` is the caller's plan type.
/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn package_rust_runtime_plan<R>(
    config: &Config,
    input: &Path,
    seams: &AnalysisSeams,
    build_runtime_plan: impl Fn(&AnalyzedProgram, &LibraryResolver) -> Result<R, Box<Diagnostic>>,
) -> Result<R, Vec<Diagnostic>> {
    let config = effective_package_config(config, input, seams.locale_loader)?;
    let spec = discover_package(input).map_err(|diag| vec![*diag])?;
    let package_root = package_root_for_input(input);
    let library_resolver = (seams.resolver_for_package)(&config, &package_root)?;
    let package = analyze_package_spec(&config, spec, &library_resolver, false, None, seams)?;
    build_runtime_plan(&package, &library_resolver).map_err(|diag| vec![*diag])
}

fn compile_package_internal(
    config: &Config,
    input: &Path,
    test_selection: Option<&RustTestSelection>,
    include_proba: bool,
    proba_filter: Option<&TestSourceFilter>,
    seams: &EmissionSeams,
) -> PackageCompileResult {
    // G4: analyze once before target rejection so Go/TS planners and diagnostics
    // share the same package graph (no reloading source per target).
    let config = match effective_package_config(config, input, seams.analysis.locale_loader) {
        Ok(config) => config,
        Err(diagnostics) => {
            return compile_failure(diagnostics);
        }
    };

    let spec = match discover_package(input) {
        Ok(spec) => spec,
        Err(diag) => {
            return compile_failure(vec![*diag]);
        }
    };

    let package_root = package_root_for_input(input);
    let library_resolver = match (seams.analysis.resolver_for_package)(&config, &package_root) {
        Ok(resolver) => resolver,
        Err(diagnostics) => {
            return compile_failure(diagnostics);
        }
    };
    let package = match analyze_package_spec(
        &config,
        spec,
        &library_resolver,
        include_proba,
        proba_filter,
        &seams.analysis,
    ) {
        Ok(package) => package,
        Err(diagnostics) => {
            return compile_failure(diagnostics);
        }
    };

    if config.target == Target::HirGo {
        return compile_package_go(&package, input, seams);
    }

    // U6-D: the package-aware Wasm path accepts Target::MirWasm (the text
    // target — binary conversion is external) and emits one module per unit
    // through the package-to-Wasm builder (entry + sibling units, canonical
    // `faber_external` symbol linking).
    if config.target == Target::MirWasm {
        return compile_package_wasm(package, input);
    }

    if config.target == Target::HirTypeScript {
        return compile_package_ts(&package, input, seams);
    }

    if config.target != Target::HirRust {
        let plan = plan_package(&package, config.target);
        if !plan.supported {
            return unsupported_plan_failure(plan, input);
        }
        // Planner seams exist; full product emit for TS is later deliveries.
        return compile_failure(vec![crate::package_diagnostic_error(format!(
            "package compilation has a {} artifact plan but full product assembly is not implemented yet",
            plan.target
        ))
        .with_file(input.display().to_string())
        .with_arg("issue", "package_target_assembly_pending")
        .with_arg("target", plan.target)]);
    }

    #[cfg(not(feature = "hir-rust"))]
    {
        return compile_failure(vec![crate::package_diagnostic_error(
            "target `rust` is not available in this faber build; rebuild with feature `hir-rust`",
        )
        .with_file(input.display().to_string())
        .with_arg("issue", "package_target_unavailable")
        .with_arg("target", "rust")]);
    }

    #[cfg(feature = "hir-rust")]
    {
        crate::rust_target::compile_package_rust(
            package,
            &library_resolver,
            test_selection,
            seams.analysis.locale_loader,
        )
    }
}

fn compile_package_go(
    package: &AnalyzedProgram,
    input: &Path,
    seams: &EmissionSeams,
) -> PackageCompileResult {
    #[cfg(not(feature = "hir-go"))]
    return compile_failure(vec![
        crate::package_diagnostic_error(
            "target `go` is not available in this faber build; rebuild with feature `hir-go`",
        )
        .with_file(input.display().to_string())
        .with_arg("issue", "package_target_unavailable")
        .with_arg("target", "go"),
    ]);

    #[cfg(feature = "hir-go")]
    {
        let plan = plan_package(package, Target::HirGo);
        if !plan.supported {
            return unsupported_plan_failure(plan, input);
        }
        (seams.emit_go)(package, input)
    }
}

fn compile_package_ts(
    package: &AnalyzedProgram,
    input: &Path,
    seams: &EmissionSeams,
) -> PackageCompileResult {
    #[cfg(not(feature = "hir-ts"))]
    return compile_failure(vec![
        crate::package_diagnostic_error(
            "target `ts` is not available in this faber build; rebuild with feature `hir-ts`",
        )
        .with_file(input.display().to_string())
        .with_arg("issue", "package_target_unavailable")
        .with_arg("target", "ts"),
    ]);

    #[cfg(feature = "hir-ts")]
    {
        let plan = plan_package(package, Target::HirTypeScript);
        if !plan.supported {
            return unsupported_plan_failure(plan, input);
        }
        PackageCompileResult {
            compile_result: (seams.emit_ts)(package, input),
            #[cfg(feature = "hir-go")]
            go_modules: Vec::new(),
            #[cfg(feature = "hir-python")]
            python_modules: Vec::new(),
        }
    }
}

/// Compile a package `.fab` entry to a native object through the interpreted
/// merged package MIR.
///
/// The configured [`radix_module::ClifEngine`] picks the object (for the linker)
/// or the in-memory image (`clif-jit`); both run the same lowering.
/// [`radix_mir_clif::emit_object`] runs the CLIF capability classifier itself,
/// so a package shape outside the CLIF subset fails here with its named gap —
/// the caller never classifies again.
/// # Errors
/// Returns no output and the decline diagnostics when the package-MIR link or
/// the CLIF lowering fails.
#[cfg(feature = "mir-clif")]
#[must_use]
pub fn compile_package_clif(
    config: &Config,
    input: &Path,
    mir_seams: &crate::mir::MirSeams,
) -> CompileResult {
    let emitted =
        crate::mir::with_interpreted_lowered_package_mir(config, input, mir_seams, |lowered| {
            let declined = |error: radix_mir_clif::ClifError| {
                vec![
                    crate::package_diagnostic_error(error.message)
                        .with_file(input.display().to_string())
                        .with_arg("issue", error.category)
                        .with_arg("shape", error.shape),
                ]
            };
            match &config.clif_engine {
                radix_module::ClifEngine::Object => {
                    radix_mir_clif::emit_object(&lowered.validated, &lowered.interner)
                        .map(|object| {
                            radix_module::Output::ClifObject(radix_module::ClifObjectOutput {
                                bytes: object.bytes,
                            })
                        })
                        .map_err(declined)
                }
                radix_module::ClifEngine::Jit(runtime) => radix_mir_clif::emit_jit(
                    &lowered.validated,
                    &lowered.interner,
                    &runtime.symbols(),
                )
                .map(|program| {
                    radix_module::Output::ClifJit(radix_module::ClifJitOutput { program })
                })
                .map_err(declined),
            }
        });
    match emitted {
        Ok(Ok(output)) => CompileResult {
            output: Some(output),
            diagnostics: Vec::new(),
        },
        Ok(Err(diagnostics)) | Err(diagnostics) => compile_failure(diagnostics).compile_result,
    }
}

fn compile_package_wasm(package: AnalyzedProgram, input: &Path) -> PackageCompileResult {
    #[cfg(not(feature = "mir-wasm"))]
    return compile_failure(vec![
        crate::package_diagnostic_error(
            "target `wasm` is not available in this faber build; rebuild with feature `mir-wasm`",
        )
        .with_file(input.display().to_string())
        .with_arg("issue", "package_target_unavailable")
        .with_arg("target", "wasm"),
    ]);

    #[cfg(feature = "mir-wasm")]
    {
        let plan = plan_package(&package, Target::MirWasm);
        if !plan.supported {
            return unsupported_plan_failure(plan, input);
        }
        let mut package = package;
        let options = crate::wasm::PackageWasmOptions::new(
            package
                .spec
                .package_root
                .join("target")
                .join("faber")
                .join("wasm"),
        );
        let build = match crate::wasm::build_package_wasm_from_graph(&mut package, &options) {
            Ok(build) => build,
            Err(diagnostics) => return compile_failure(diagnostics),
        };
        let entry_wat = build
            .modules
            .iter()
            .find(|module| module.is_entry)
            .map(|module| module.wat.clone())
            .unwrap_or_default();
        PackageCompileResult {
            compile_result: CompileResult {
                output: Some(radix_module::Output::WasmText(
                    radix_module::WasmTextOutput { code: entry_wat },
                )),
                diagnostics: package.diagnostics.clone(),
            },
            #[cfg(feature = "hir-go")]
            go_modules: Vec::new(),
            #[cfg(feature = "hir-python")]
            python_modules: Vec::new(),
        }
    }
}

#[cfg(test)]
#[path = "emit_test.rs"]
mod tests;
