//! Package-mode CLI commands: `build`, `check`, and `emit`.
//!
//! Routes package-shaped inputs to target-specific builders (Rust/Cargo,
//! MIR images, Go, Wasm, …) while single-file and stdin paths keep legacy
//! emit behaviour. Target availability follows compile-time feature gates.

use radix::module::CompileResult;
use radix::module::codegen::Target;
use radix::services::DiagnosticMode;
use std::fs;
use std::path::{Path, PathBuf};

use crate::input_shape::locale_without_package_error;

#[cfg(feature = "hir-rust")]
use super::cargo::{
    emit_generated_crate_with_runtime_plan, invoke_cargo_build, invoke_cargo_build_library,
    lock_generated_crate_build,
};
#[cfg(feature = "hir-rust")]
use super::manifest::FaberManifest;
use super::manifest_build_target_for_input;
#[cfg(feature = "hir-rust")]
use super::package_host_selection_diagnostic;
#[cfg(any(feature = "hir-rust", feature = "hir-ts"))]
use super::read_manifest;
#[cfg(feature = "mir-llvm")]
use super::{LlvmHostProfile, build_host_program_in};
use super::{MANIFEST_FILE, config_with_locale};

#[cfg(any(
    feature = "hir-rust",
    feature = "hir-go",
    feature = "hir-ts",
    feature = "mir-wasm",
    feature = "mir-llvm"
))]
pub(super) mod build_layout {
    use std::path::Path;

    use radix::program::discovery::BuildLayout;

    use crate::package::{MANIFEST_FILE, absolutize_path};

    /// Layout for a package-mode build of `input`.
    ///
    /// A manifest-backed package owns its `target/` tree. A single `.fab` file
    /// with no `faber.toml` above it has no package of its own: its generated
    /// project and `target/` go under `out_dir`, never into the source directory.
    /// The layout's `package_root` and `manifest_path` still name the source side.
    pub(in crate::package) fn discover_build_layout(
        input: &Path,
        out_dir: &Path,
    ) -> Result<BuildLayout, Box<radix::module::Diagnostic>> {
        let layout = crate::package::discover_build_layout(input)?;
        if !is_manifestless_single_file(input) {
            return Ok(layout);
        }
        let relocated =
            BuildLayout::from_package_root(absolutize_path(out_dir), layout.binary_name());
        Ok(BuildLayout {
            package_root: layout.package_root,
            manifest_path: layout.manifest_path,
            ..relocated
        })
    }

    /// True for a regular source file that is not a manifest and has no
    /// `faber.toml` in any ancestor directory.
    pub(in crate::package) fn is_manifestless_single_file(input: &Path) -> bool {
        input.is_file()
            && input.file_name().and_then(|name| name.to_str()) != Some(MANIFEST_FILE)
            && !input
                .ancestors()
                .skip(1)
                .any(|dir| dir.join(MANIFEST_FILE).exists())
    }
}
#[cfg(any(
    feature = "hir-rust",
    feature = "hir-go",
    feature = "hir-ts",
    feature = "mir-wasm",
    feature = "mir-llvm"
))]
pub(super) use build_layout::discover_build_layout;

/// Print `message` and terminate with the internal-failure class: the caller
/// reports a compile step that produced nothing without a diagnostic.
fn fail(message: &str) -> ! {
    eprintln!("{message}");
    crate::exit_status::exit_software();
}

/// Terminate after a compile that produced no output; the diagnostics were
/// already printed.
fn fail_compilation(diagnostics: &[radix::module::Diagnostic]) -> ! {
    eprintln!("compilation failed");
    crate::exit_status::exit_for_compile_failure(diagnostics);
}

/// Exit when a build target was requested but this `faber` binary lacks its feature.
///
/// WHY: the gate takes the `Target`, not a spelling, so the name shown is
/// always the canonical one (`wasm-text`) and never a caller-surface alias
/// (`wasm`) the user may not have typed.
#[cfg(any(
    not(feature = "hir-fhir"),
    not(feature = "mir-llvm"),
    not(feature = "mir-wasm"),
    not(feature = "mir-metal"),
    not(feature = "hir-go"),
))]
fn exit_target_feature_unavailable(target: Target) -> ! {
    super::target_gate::exit_target_not_in_this_build(target)
}

#[cfg(not(feature = "runner"))]
fn exit_runner_feature_unavailable() -> ! {
    eprintln!(
        "error: MIR package commands are not available in this faber build; rebuild with feature `runner`"
    );
    crate::exit_status::exit_unavailable();
}

/// Report a diagnostic's message and terminate.
fn fail_with_diagnostic(diag: &radix::module::Diagnostic) -> ! {
    let has_path_context = ["path", "base", "artifact"]
        .iter()
        .all(|name| diag.args.iter().any(|arg| arg.name == *name));
    if diag.phase == radix::module::diagnostics::DiagnosticPhase::Io && diag.code.is_none() {
        eprintln!("{}", radix::module::diagnostics::render_plain(diag));
    } else if has_path_context {
        if let Some(label) = diag.display_label() {
            eprintln!("error[{label}]: {}", diag.message);
        } else {
            eprintln!("error: {}", diag.message);
        }
    } else if diag.phase == radix::module::diagnostics::DiagnosticPhase::Io {
        eprintln!("{}", radix::module::diagnostics::render_plain(diag));
    } else {
        eprintln!("error: {}", diag.message);
    }
    crate::exit_status::exit_for_diagnostic(diag);
}

/// Print `diagnostics` in normal mode, then terminate with `message`.
fn report_diagnostics_and_exit(
    diagnostics: &[radix::module::Diagnostic],
    locale_pack: Option<&radix::module::locale::LocalePack>,
    message: &str,
) -> ! {
    crate::postprocess::print_diagnostics(diagnostics, DiagnosticMode::Normal, locale_pack);
    eprintln!("{message}");
    crate::exit_status::exit_for_diagnostics(diagnostics);
}

fn report_diagnostics_and_exit_mode(
    diagnostics: &[radix::module::Diagnostic],
    locale_pack: Option<&radix::module::locale::LocalePack>,
    message: &str,
    mode: DiagnosticMode,
) -> ! {
    crate::postprocess::print_diagnostics(diagnostics, mode, locale_pack);
    eprintln!("{message}");
    crate::exit_status::exit_for_diagnostics(diagnostics);
}

/// Execute the user-facing `faber build` command.
///
/// Package Rust builds emit a generated Cargo crate and then delegate binary
/// production to Cargo. Direct-file builds and non-Rust targets keep the legacy
/// single-output behavior so package ergonomics do not change unrelated command
/// paths.
pub fn cmd_build(command: radix::services::BuildCommand, format: bool, linter: bool, safe: bool) {
    if let Some(message) = locale_without_package_error(
        command.locale.as_deref(),
        std::slice::from_ref(&command.input),
        command.package,
    ) {
        eprintln!("error: {message}");
        crate::exit_status::exit_usage();
    }

    let raw_input_path = PathBuf::from(&command.input);
    let target = resolve_build_target(&command, &raw_input_path);
    if !super::target_gate::target_compiled_in(target) {
        super::target_gate::exit_target_not_in_this_build(target);
    }
    let is_package = use_package_compiler(target, &raw_input_path, command.package);
    let input_path = if is_package {
        super::absolutize_path(&raw_input_path)
    } else {
        raw_input_path
    };
    let warn_policy = radix::module::driver::WarnPolicy {
        deny_all_warnings: command.deny_warnings,
        deny_codes: command.deny_codes,
    };
    let safe = super::resolve_safe_kernels(safe, &input_path);
    let (config, locale_pack) = if is_package {
        match config_with_locale(
            target,
            &input_path,
            command.locale.as_deref(),
            command.diagnostics_locale.as_deref(),
        ) {
            Ok((config, pack)) => (config.with_warn_policy(warn_policy), pack),
            Err(diag) => {
                let diag = super::attach_path_diagnostic_context(diag);
                fail_with_diagnostic(&diag)
            }
        }
    } else {
        let config = match super::locale::default_config_with_locale(target) {
            Ok(config) => config.with_dev_locale_root().with_warn_policy(warn_policy),
            Err(diag) => fail_with_diagnostic(&diag),
        };
        let locale_pack = config.locale_pack.clone();
        (config, locale_pack)
    };
    let config = if safe {
        config.with_safe_kernels()
    } else {
        config
    };

    if is_package && target == Target::MirScena {
        #[cfg(not(feature = "runner"))]
        exit_runner_feature_unavailable();
        #[cfg(feature = "runner")]
        {
            let artifact = match radix::program::mir::build_package_mir_artifact(
                &config,
                &input_path,
                &[],
                &super::mir::faber_mir_seams(),
            ) {
                Ok(artifact) => artifact,
                Err(diagnostics) => report_diagnostics_and_exit_mode(
                    &diagnostics,
                    locale_pack.as_ref(),
                    "scena artifact build failed",
                    command.diagnostic_mode,
                ),
            };
            println!("{}", artifact.manifest_path.display());
            return;
        }
    }

    if is_package && target == Target::MirFmir {
        #[cfg(not(feature = "runner"))]
        exit_runner_feature_unavailable();
        #[cfg(feature = "runner")]
        {
            let image = match radix::program::mir::build_package_fmir_text_image(
                &config,
                &input_path,
                &[],
                &super::mir::faber_mir_seams(),
            ) {
                Ok(image) => image,
                Err(diagnostics) => report_diagnostics_and_exit_mode(
                    &diagnostics,
                    locale_pack.as_ref(),
                    "fmir-text image build failed",
                    command.diagnostic_mode,
                ),
            };
            println!("{}", image.image_path.display());
            return;
        }
    }

    if is_package && target == Target::MirFmirBinary {
        #[cfg(not(feature = "runner"))]
        exit_runner_feature_unavailable();
        #[cfg(feature = "runner")]
        {
            let image = match radix::program::mir::build_package_fmir_image(
                &config,
                &input_path,
                &[],
                &super::mir::faber_mir_seams(),
            ) {
                Ok(image) => image,
                Err(diagnostics) => report_diagnostics_and_exit_mode(
                    &diagnostics,
                    locale_pack.as_ref(),
                    "fmir image build failed",
                    command.diagnostic_mode,
                ),
            };
            println!("{}", image.image_path.display());
            return;
        }
    }

    if is_package && target == Target::HirFhir {
        #[cfg(not(feature = "hir-fhir"))]
        exit_target_feature_unavailable(Target::HirFhir);
        #[cfg(feature = "hir-fhir")]
        let artifact = match radix::program::fhir::build_package_fhir(
            &config,
            &input_path,
            &super::faber_analysis_seams(),
        ) {
            Ok(artifact) => artifact,
            Err(diagnostics) => report_diagnostics_and_exit_mode(
                &diagnostics,
                locale_pack.as_ref(),
                "fhir package build failed",
                command.diagnostic_mode,
            ),
        };
        #[cfg(feature = "hir-fhir")]
        println!("{}", artifact.package_path.display());
        #[cfg(feature = "hir-fhir")]
        return;
    }

    // Stage 9 S9.2: `faber build --target llvm-host` routes to the shared
    // package-to-LLVM builder + native verify/link (the SAME builder the
    // pairwise harness uses). Never Rust codegen for the program, never a `cc`
    // fallback; fails with a structured diagnostic when the toolchain or
    // runtime archive is unavailable or the host triple is unsupported.
    if is_package && target == Target::MirLlvmHost {
        #[cfg(not(feature = "mir-llvm"))]
        exit_target_feature_unavailable(Target::MirLlvmHost);
        #[cfg(feature = "mir-llvm")]
        {
            let profile = if command.release {
                LlvmHostProfile::Release
            } else {
                LlvmHostProfile::Debug
            };
            let build = match build_host_program_in(&config, &input_path, profile, &command.out_dir)
            {
                Ok(build) => build,
                Err(diagnostics) => super::build_recipe::report_and_exit_mode(
                    &diagnostics,
                    locale_pack.as_ref(),
                    command.diagnostic_mode,
                ),
            };
            println!("{}", build.binary_path.display());
            return;
        }
    }

    // U6-D: package Wasm builds emit one package-aware module per unit and
    // print the module output directory. Linking/running is external (the
    // product host's job); the build artifact is the module set + link
    // manifest. The `wasm` target is the text target — binary conversion is
    // an external-tool step (`wat2wasm` / `wasm-tools parse`).
    if is_package && target == Target::MirWasm {
        #[cfg(not(feature = "mir-wasm"))]
        exit_target_feature_unavailable(Target::MirWasm);
        #[cfg(feature = "mir-wasm")]
        {
            let output_dir = match discover_build_layout(&input_path, &command.out_dir) {
                Ok(layout) => layout.cargo_target_dir.join("faber").join("wasm"),
                Err(d) => fail_with_diagnostic(&d),
            };
            let options = radix::program::wasm::PackageWasmOptions::new(output_dir);
            let build = match radix::program::wasm::build_package_wasm(
                &config,
                &input_path,
                &options,
                &super::faber_analysis_seams(),
            ) {
                Ok(build) => build,
                Err(diagnostics) => report_diagnostics_and_exit_mode(
                    &diagnostics,
                    locale_pack.as_ref(),
                    "wasm package build failed",
                    command.diagnostic_mode,
                ),
            };
            println!("{}", build.manifest.output.display());
            return;
        }
    }

    // SGR-U2: `.fab` metal-text builds use the package lane. The interpreted
    // merged consumer instantiates library imports so concrete imported device
    // registrations join the entry-role map before the probe; the single-file
    // lane has no registrations and fails the empty-kernels gate.
    if is_package && target == Target::MirMetal {
        #[cfg(not(feature = "mir-metal"))]
        exit_target_feature_unavailable(Target::MirMetal);
        #[cfg(feature = "mir-metal")]
        {
            let code = match compile_package_metal_text(&config, &input_path) {
                Ok(code) => code,
                Err(diagnostics) => report_diagnostics_and_exit_mode(
                    &diagnostics,
                    locale_pack.as_ref(),
                    "metal-text package build failed",
                    command.diagnostic_mode,
                ),
            };
            let output_path = radix::services::build_output_path(
                &command.out_dir,
                &input_path,
                target,
                is_package,
            );
            let code = crate::postprocess::postprocess_code(code, target, format, linter);
            write_output_file(&output_path, code.as_bytes());
            println!("{}", output_path.display());
            return;
        }
    }

    // WHY: browser products are TypeScript-target builds (private backend).
    #[cfg(feature = "hir-ts")]
    if is_package && target == Target::HirTypeScript {
        let layout = match discover_build_layout(&input_path, &command.out_dir) {
            Ok(l) => l,
            Err(d) => fail_with_diagnostic(&d),
        };
        if layout.manifest_path.exists() {
            let manifest = read_manifest(&layout.manifest_path)
                .unwrap_or_else(|diag| fail_with_diagnostic(&diag));
            if let Some(product) = manifest.product.as_ref() {
                match super::build_browser_product_with_postprocess(
                    &config,
                    &input_path,
                    product,
                    format,
                    linter,
                ) {
                    Ok(build) => {
                        println!("{}", build.esm_entry.display());
                        return;
                    }
                    Err(diag) => fail_with_diagnostic(&diag),
                }
            }
        }
        // Otherwise the package is a Node program: write a self-contained
        // project under target/faber/ts, compile it with the backend recipe
        // and print the launcher.
        match super::ts_build::build_ts_program(&config, &input_path, &layout) {
            Ok(program) => {
                println!("{}", program.launcher.display());
                return;
            }
            Err(diagnostics) => super::build_recipe::report_and_exit_mode(
                &diagnostics,
                locale_pack.as_ref(),
                command.diagnostic_mode,
            ),
        }
    }

    // Package Go builds write target/faber/go (module, vendored runtime) and
    // run the backend recipe's build command. The multi-module file
    // collection travels inside the compile result itself (FBR-P2-003).
    if is_package && target == Target::HirGo {
        #[cfg(not(feature = "hir-go"))]
        exit_target_feature_unavailable(Target::HirGo);
        #[cfg(feature = "hir-go")]
        {
            let layout = match discover_build_layout(&input_path, &command.out_dir) {
                Ok(l) => l,
                Err(d) => fail_with_diagnostic(&d),
            };
            match super::go_build::go_program::build_go_program(
                &config,
                &input_path,
                &layout,
                format,
                linter,
                locale_pack.as_ref(),
                command.diagnostic_mode,
            ) {
                Ok(program) => {
                    println!("{}", program.binary.display());
                    return;
                }
                Err(failure) => failure.exit(locale_pack.as_ref(), command.diagnostic_mode),
            }
        }
    }

    let result = if is_package {
        package_compile_result(&config, &input_path, target)
    } else {
        let compiler = radix::module::Compiler::new(config.clone());
        compiler.compile(&input_path)
    };

    crate::postprocess::print_diagnostics(
        &result.diagnostics,
        command.diagnostic_mode,
        locale_pack.as_ref(),
    );

    let Some(output) = result.output else {
        fail_compilation(&result.diagnostics);
    };

    // Native `clif` builds: the compiler emits an object, Faber links it
    // against the runtime archive with the system C linker.
    #[cfg(feature = "mir-clif")]
    if target == Target::MirClif {
        let radix::module::Output::ClifObject(object) = output else {
            fail("clif compilation produced no object");
        };
        let stem = input_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .filter(|stem| !stem.is_empty())
            .unwrap_or("out");
        match super::clif_build::link_executable(
            &object.bytes,
            &command.out_dir,
            stem,
            command.release,
        ) {
            Ok(path) => {
                println!("{}", path.display());
                return;
            }
            Err(diagnostic) => fail_with_diagnostic(&diagnostic),
        }
    }

    // Package Rust builds own a generated crate under target/faber/ and let
    // Cargo place artifacts in sibling debug/release directories.
    #[cfg(feature = "hir-rust")]
    if is_package && target == Target::HirRust {
        let layout = match discover_build_layout(&input_path, &command.out_dir) {
            Ok(l) => l,
            Err(d) => fail_with_diagnostic(&d),
        };
        // FBR-P2-004: the per-package advisory lock spans the full emit + cargo
        // sequence (library crates, generated crate snapshot, Cargo invocation)
        // so concurrent builds of this package cannot interleave files from
        // different runtime plans. Dropped when the block ends.
        let _build_lock = match lock_generated_crate_build(&layout) {
            Ok(lock) => lock,
            Err(d) => fail_with_diagnostic(&d),
        };
        let meta = if layout.manifest_path.exists() {
            read_manifest(&layout.manifest_path).ok()
        } else {
            None
        };
        let mut runtime_plan = match radix::program::emit::package_rust_runtime_plan(
            &config,
            &input_path,
            &super::faber_analysis_seams(),
            super::rust_runtime_plan_for_package,
        ) {
            Ok(plan) => plan,
            Err(diagnostics) => report_diagnostics_and_exit_mode(
                &diagnostics,
                locale_pack.as_ref(),
                "runtime plan failed",
                command.diagnostic_mode,
            ),
        };
        if let Some(diagnostic) =
            package_host_selection_diagnostic(&runtime_plan, &layout.manifest_path)
        {
            report_diagnostics_and_exit_mode(
                &[diagnostic],
                locale_pack.as_ref(),
                "runtime plan failed",
                command.diagnostic_mode,
            );
        }
        // G4: emit native-binding library crates before the application crate links them.
        match super::library_link::emit_linked_library_crates(&layout.package_root, &layout) {
            Ok(linked) => {
                runtime_plan.library_path_deps = linked
                    .into_iter()
                    .map(|lib| (lib.crate_name, lib.crate_root))
                    .collect();
            }
            Err(diagnostics) => report_diagnostics_and_exit_mode(
                &diagnostics,
                locale_pack.as_ref(),
                "library dependency graph failed",
                command.diagnostic_mode,
            ),
        }
        let package_code = crate::postprocess::postprocess_code(
            radix::services::output_code(output),
            target,
            format,
            linter,
        );
        let binary_path = emit_crate_and_build(
            &layout,
            &package_code,
            meta.as_ref(),
            &runtime_plan,
            command.release,
        );
        println!("{}", binary_path.display());
        return;
    }

    let code = radix::services::output_code(output);

    // Single-file Rust builds that need the `faber` runtime / tokio (from HIR facts,
    // not emitted-text sniffing) emit a generated Cargo crate under
    // `target/faber/`. Programs without those deps keep the bare `.rs` path.
    #[cfg(feature = "hir-rust")]
    if target == Target::HirRust {
        match single_file_rust_runtime_plan(&config, &input_path) {
            Ok(plan) if plan.requires_generated_crate() => {
                let package_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                let stem = input_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .filter(|s| !s.is_empty())
                    .unwrap_or("faber_out");
                let layout =
                    radix::program::discovery::BuildLayout::from_package_root(&package_root, stem);
                // Same per-package lock as package builds: the emit + cargo
                // sequence shares `target/faber/` under the current directory.
                let _build_lock = match lock_generated_crate_build(&layout) {
                    Ok(lock) => lock,
                    Err(d) => fail_with_diagnostic(&d),
                };
                let generated_code =
                    crate::postprocess::postprocess_code(code.clone(), target, format, linter);
                let binary_path =
                    emit_crate_and_build(&layout, &generated_code, None, &plan, command.release);
                println!("{}", binary_path.display());
                return;
            }
            Ok(_) => {}
            Err(diagnostics) => report_diagnostics_and_exit_mode(
                &diagnostics,
                locale_pack.as_ref(),
                "runtime plan failed",
                command.diagnostic_mode,
            ),
        }
    }

    // Legacy single-file path (direct .fab files, other targets, or --out-dir override cases)
    let output_path =
        radix::services::build_output_path(&command.out_dir, &input_path, target, is_package);

    let code = crate::postprocess::postprocess_code(code, target, format, linter);
    write_output_file(&output_path, code.as_bytes());

    println!("{}", output_path.display());
}

/// Emit the generated crate with its runtime plan and delegate binary
/// production to Cargo, returning the produced binary path.
#[cfg(feature = "hir-rust")]
fn emit_crate_and_build(
    layout: &radix::program::discovery::BuildLayout,
    code: &str,
    meta: Option<&FaberManifest>,
    plan: &super::RustRuntimePlan,
    release: bool,
) -> PathBuf {
    if let Err(d) = emit_generated_crate_with_runtime_plan(layout, code, meta, plan) {
        fail_with_diagnostic(&d);
    }
    let built = if plan.library_crate {
        invoke_cargo_build_library(layout, release)
    } else {
        invoke_cargo_build(layout, release)
    };
    match built {
        Ok(path) => path,
        Err(d) => fail_with_diagnostic(&d),
    }
}

/// Create the output path's parent and write `bytes`, exiting with a
/// structured error message on failure.
fn write_output_file(output_path: &Path, bytes: &[u8]) {
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).unwrap_or_else(|err| {
            eprintln!("error: failed to create '{}': {}", parent.display(), err);
            crate::exit_status::exit_cant_create();
        });
    }
    fs::write(output_path, bytes).unwrap_or_else(|err| {
        eprintln!(
            "error: failed to write '{}': {}",
            output_path.display(),
            err
        );
        crate::exit_status::exit_cant_create();
    });
}

/// Structured runtime plan for a single `.fab` file (no package manifest).
#[cfg(feature = "hir-rust")]
fn single_file_rust_runtime_plan(
    config: &radix::module::Config,
    input_path: &Path,
) -> Result<super::RustRuntimePlan, Vec<radix::module::Diagnostic>> {
    use radix::module::driver::Session;
    use radix::program::analyze_prepared_program_path;
    use std::fs;

    let source = fs::read_to_string(input_path).map_err(|err| {
        vec![crate::package_diagnostic_error(format!(
            "failed to read '{}': {err}",
            input_path.display()
        ))]
    })?;
    let session = Session::new(config.clone());
    let analysis = analyze_prepared_program_path(&session, input_path, &source)?;
    let needs = radix_hir_rust::needs::collect_rust_needs(
        &analysis.hir,
        &analysis.types,
        std::collections::BTreeSet::new(),
        None,
    );
    let needs_tokio = analysis.hir.entry_is_async
        || analysis.hir.items.iter().any(
            |item| matches!(&item.kind, radix::module::hir::HirItemKind::Function(f) if f.is_async),
        );
    Ok(super::RustRuntimePlan {
        needs_faber: needs.needs_faber_runtime,
        needs_tokio,
        needs_regex: needs.needs_regex,
        host: None,
        non_runtime_routes: std::collections::BTreeSet::new(),
        library_path_deps: Vec::new(),
        // A file with no entry point cannot be a binary crate: build a library.
        library_crate: analysis.hir.entry.is_none(),
    })
}

/// Resolve the build target: an explicit CLI target wins, otherwise the
/// package manifest's `build.target`. A manifest read failure or unsupported
/// target is fatal, including when the CLI explicitly overrides a valid target.
fn resolve_build_target(command: &radix::services::BuildCommand, input_path: &Path) -> Target {
    let manifest_target = match manifest_build_target_for_input(input_path) {
        Ok(target) => target,
        Err(diag) => fail_with_diagnostic(&diag),
    };
    if command.target_explicit {
        return command.target;
    }

    match manifest_target {
        Some(target) => target,
        None => command.target,
    }
}

/// Decide whether an input path should enter package-mode command handling.
///
/// Directory, manifest, and `.fab` entry files are package-shaped by default so
/// builtin library imports (`norma:…`) resolve through the package loader.
/// Stdin and non-Faber paths still use legacy single-file commands unless the
/// caller forces package mode.
fn should_treat_as_package(path: &std::path::Path) -> bool {
    path.is_dir()
        || path.file_name().and_then(|name| name.to_str()) == Some(MANIFEST_FILE)
        || path.extension().is_some_and(|ext| ext == "fab")
}

/// Target-aware package routing for emit/build.
///
/// Single `.fab` files use the package loader for Rust, package-image, and
/// metal-text targets so `norma:*` imports resolve through the package graph
/// and metal-text sees the merged consumer's imported device registrations.
/// MIR probe targets and HIR inspection targets (`go`, `ts`) use the radix
/// single-file path, matching `radix emit`.
pub fn use_package_compiler(target: Target, path: &std::path::Path, force_package: bool) -> bool {
    if force_package {
        return true;
    }
    if path.is_dir() || path.file_name().and_then(|name| name.to_str()) == Some(MANIFEST_FILE) {
        return true;
    }
    if path.extension().is_some_and(|ext| ext == "fab") {
        if matches!(
            target,
            Target::HirRust
                | Target::MirScena
                | Target::MirFmir
                | Target::MirFmirBinary
                | Target::HirFhir
                | Target::MirLlvmHost
                | Target::MirWasm
                | Target::MirMetal
        ) {
            return true;
        }
        // A lone file that imports a sibling module is package-shaped for the
        // native `clif` lanes: a single-file object carries no linked sibling
        // code, so the imported call would decline as CODEGEN001
        // `external definition call`. The package-MIR emitter links the import
        // graph, the same route `faber run` already interprets such files
        // through. Files without such imports keep the single-file route.
        if target == Target::MirClif && super::manifestless_file_declares_non_kernel_import(path) {
            return true;
        }
    }
    false
}

/// CLI-argument variant of [`use_package_compiler`].
///
/// See [`should_treat_as_package_from_args`] for the stdin guard shared with
/// check-only routing.
pub fn use_package_compiler_from_args(
    target: Target,
    input: &[String],
    force_package: bool,
) -> bool {
    if input.is_empty() || input[0] == "-" {
        return false;
    }
    let path = super::absolutize_path(std::path::Path::new(&input[0]));
    use_package_compiler(target, &path, force_package)
}

/// CLI-argument variant of package-mode detection for commands without a target
/// flag (for example `faber check`).
///
/// Standard-input builds (`-`) cannot be package builds because package
/// discovery needs filesystem paths for imports, manifests, and generated
/// layouts.
pub fn should_treat_as_package_from_args(input: &[String]) -> bool {
    if input.is_empty() || input[0] == "-" {
        return false;
    }
    let path = super::absolutize_path(std::path::Path::new(&input[0]));
    should_treat_as_package(&path)
}

/// Execute the package-aware `faber check` command.
///
/// The permissive mode intentionally downgrades only unresolved/import-driven
/// semantic errors; manifest, I/O, parse, and package-policy errors remain
/// fatal because they prevent reliable package graph construction.
///
/// `complexity_budget` is the `--complexity-budget` CLI override and
/// `kernel_complexity_budget` the `--kernel-complexity-budget` one (the
/// advisory kernel-entry budget). Each falls back independently to its
/// manifest `[check]` key, then to the driver's baked-in default.
pub fn cmd_check_package(
    command: radix::services::CheckCommand,
    complexity_budget: Option<u32>,
    kernel_complexity_budget: Option<u32>,
    lower: bool,
    device: Option<DeviceAdviceRequest>,
) {
    if command.input.is_empty() || command.input[0] == "-" {
        eprintln!("error: package checking requires a path input");
        crate::exit_status::exit_usage();
    }
    let advice_target = device.map(advice_target);

    let input_path = super::absolutize_path(std::path::Path::new(&command.input[0]));

    // Resolve target from faber.toml so AIR-lane exempla with MIR-backed
    // targets (e.g. fmir-text) do not trigger TARGETLANE001.
    // A manifest that cannot be read or names an unsupported target is fatal,
    // exactly as in `faber build`; a package with no manifest checks as
    // `Target::HirRust`.
    let check_target = match manifest_build_target_for_input(&input_path) {
        Ok(Some(target)) => target,
        Ok(None) => Target::HirRust,
        Err(diag) => fail_with_diagnostic(&diag),
    };

    let (config, locale_pack) = match config_with_locale(
        check_target,
        &input_path,
        command.locale.as_deref(),
        command.diagnostics_locale.as_deref(),
    ) {
        Ok((config, pack)) => {
            let config = config.with_warn_policy(radix::module::driver::WarnPolicy {
                deny_all_warnings: command.deny_warnings,
                deny_codes: command.deny_codes,
            });
            // CC-U3/CC-U6 layering, one budget at a time: each CLI flag
            // beats its manifest `[check]` key, which beats the baked
            // default; the two budgets never borrow each other's layers.
            let check = super::manifest::manifest_check_section(&input_path);
            let budget = complexity_budget.or_else(|| check.as_ref()?.complexity_budget);
            let kernel_budget =
                kernel_complexity_budget.or_else(|| check.as_ref()?.kernel_complexity_budget);
            let config = match budget {
                Some(budget) => config.with_complexity_budget(budget),
                None => config,
            };
            let config = match kernel_budget {
                Some(budget) => config.with_kernel_complexity_budget(budget),
                None => config,
            };
            (config, pack)
        }
        Err(diag) => {
            let diag = super::attach_path_diagnostic_context(diag);
            fail_with_diagnostic(&diag)
        }
    };
    let mut diagnostics = radix::program::analyze::check_package(
        &config,
        &input_path,
        &super::faber_analysis_seams(),
    );
    diagnostics = radix::services::normalize_locale002_notes(diagnostics);
    diagnostics = crate::postprocess::drop_declared_locale002_notes(diagnostics);
    radix::module::apply_warn_policy(&mut diagnostics, &config.warn_policy);

    let mut fatal_errors = 0usize;
    let mut fatal_diagnostics = Vec::new();
    let mut downgraded = 0usize;
    if command.diagnostic_mode == DiagnosticMode::Diagnostics && !diagnostics.is_empty() {
        match locale_pack.as_ref() {
            Some(pack) => {
                match radix::module::diagnostics::render_expanded_diagnostics_with_pack(
                    &diagnostics,
                    pack,
                ) {
                    Ok(rendered) => eprintln!("{rendered}"),
                    Err(err) => {
                        eprintln!("error: failed to render reader-pack diagnostics: {err}");
                        crate::exit_status::exit_software();
                    }
                }
            }
            None => eprintln!(
                "{}",
                radix::module::diagnostics::render_expanded_diagnostics(&diagnostics)
            ),
        }
        crate::postprocess::print_explain_footer(&diagnostics);
    }

    for diag in &diagnostics {
        let downgraded_error =
            command.permissive && diag.is_error() && is_permissive_check_code(diag.code);
        if command.diagnostic_mode == DiagnosticMode::Normal {
            let display = if downgraded_error {
                diag.clone()
                    .with_severity(radix::module::diagnostics::Severity::Warning)
            } else {
                diag.clone()
            };
            eprintln!("{}", radix::module::diagnostics::render_plain(&display));
        }
        if diag.is_error() {
            if downgraded_error {
                downgraded += 1;
            } else {
                fatal_errors += 1;
                fatal_diagnostics.push(diag.clone());
            }
        }
    }

    if command.permissive && downgraded > 0 {
        eprintln!(
            "warning:{}: downgraded {} unresolved/import-driven semantic error(s) in permissive mode",
            input_path.display(),
            downgraded
        );
    }

    if fatal_errors == 0 && lower {
        #[cfg(not(feature = "runner"))]
        exit_runner_feature_unavailable();
        #[cfg(feature = "runner")]
        lower_checked_package(
            &config,
            &input_path,
            command.diagnostic_mode,
            locale_pack.as_ref(),
        );
    }

    if fatal_errors == 0 && advice_target.is_some() {
        print_region_report(&config, &input_path, command.diagnostic_mode);
    }

    if fatal_errors == 0
        && let Some((device, partitions)) = advice_target
    {
        print_device_advice(
            &config,
            &input_path,
            device,
            partitions,
            command.diagnostic_mode,
            locale_pack.as_ref(),
        );
    }

    if fatal_errors == 0 {
        eprintln!("ok: {}", input_path.display());
    } else {
        crate::exit_status::exit_for_compile_failure(&fatal_diagnostics);
    }
}

/// Final line of a `check --lower` run whose program passed semantic
/// analysis but failed to lower. Scripts that sweep many inputs (the
/// `scripta/check-lower-sweep` gate) tell this apart from an ordinary check
/// error by this text.
pub const CHECKS_BUT_DOES_NOT_LOWER: &str = "checks but does not lower";

/// The `--lower` step of `faber check`: lower the checked package to MIR and
/// run the MIR validator through the same package-MIR driver `faber run` and
/// `faber build -t fmir` use, without executing anything. This lane plans the
/// CLI surface without a runtime argument vector. Failures print as located
/// diagnostics and exit 1.
///
/// A package with no entry unit (a library such as Norma) has nothing to
/// start from, so each of its modules is lowered on its own, as if it were
/// the single-file entry.
#[cfg(feature = "runner")]
fn lower_checked_package(
    config: &radix::module::Config,
    input_path: &Path,
    mode: DiagnosticMode,
    locale_pack: Option<&radix::module::locale::LocalePack>,
) {
    let mut diagnostics = Vec::new();
    for unit in lowering_units(config, input_path) {
        if let Err(errors) = super::mir::with_check_lowered_package_mir(config, &unit, |_| ()) {
            diagnostics.extend(errors);
        }
    }
    if diagnostics.is_empty() {
        return;
    }
    exit_does_not_lower(&diagnostics, input_path, mode, locale_pack);
}

/// Print the diagnostics of a checked package that does not lower and exit 1
/// with [`CHECKS_BUT_DOES_NOT_LOWER`].
#[cfg(feature = "runner")]
fn exit_does_not_lower(
    diagnostics: &[radix::module::diagnostics::Diagnostic],
    input_path: &Path,
    mode: DiagnosticMode,
    locale_pack: Option<&radix::module::locale::LocalePack>,
) -> ! {
    if mode == DiagnosticMode::Diagnostics {
        let rendered = match locale_pack {
            Some(pack) => {
                radix::module::diagnostics::render_expanded_diagnostics_with_pack(diagnostics, pack)
                    .unwrap_or_else(|err| {
                        eprintln!("error: failed to render reader-pack diagnostics: {err}");
                        crate::exit_status::exit_software();
                    })
            }
            None => radix::module::diagnostics::render_expanded_diagnostics(diagnostics),
        };
        eprintln!("{rendered}");
        crate::postprocess::print_explain_footer(diagnostics);
    } else {
        for diag in diagnostics {
            eprintln!("{}", radix::module::diagnostics::render_plain(diag));
        }
    }
    eprintln!(
        "error: {}: {CHECKS_BUT_DOES_NOT_LOWER}",
        input_path.display()
    );
    crate::exit_status::exit_for_compile_failure(diagnostics);
}

/// What `faber check --device D [--partitions N]` asked for: advice on where
/// the package's kernels run serialized on that device.
#[derive(Debug, Clone, Copy)]
pub struct DeviceAdviceRequest {
    pub device: super::DeviceChoice,
    pub partitions: Option<u32>,
}

/// The device and partition count of an advice request. Fails closed (exit 1)
/// on a selection that is not a device (`auto`, `reference`) and on
/// `--partitions` with a device that cannot span (the `faber run` rule).
#[cfg(all(feature = "runner", feature = "device-runtime"))]
fn advice_target(request: DeviceAdviceRequest) -> (super::device::AdviceDevice, u32) {
    use super::DeviceChoice;
    use super::device::AdviceDevice;
    let device = match request.device {
        DeviceChoice::Cpu => AdviceDevice::Cpu,
        DeviceChoice::Metal => AdviceDevice::Metal,
        DeviceChoice::Cuda => AdviceDevice::Cuda,
        DeviceChoice::Auto | DeviceChoice::Reference => {
            eprintln!("error: `check --device` names the device to advise for: cpu, metal or cuda");
            crate::exit_status::exit_usage();
        }
    };
    (device, request.partitions.unwrap_or(1))
}

#[cfg(not(all(feature = "runner", feature = "device-runtime")))]
fn advice_target(_: DeviceAdviceRequest) -> ((), u32) {
    eprintln!("error: `check --device` needs a faber built with the device runtime");
    crate::exit_status::exit_unavailable();
}

/// Print the "serialized by the backend" advice (`INFO001`) of the checked
/// package on `device` over `partitions` partitions. Advice is `info`: it
/// never changes the exit status. A package that does not lower, or whose
/// kernels do not build into one device image, exits 1 as `check --lower`
/// does.
#[cfg(all(feature = "runner", feature = "device-runtime"))]
fn print_device_advice(
    config: &radix::module::Config,
    input_path: &Path,
    device: super::device::AdviceDevice,
    partitions: u32,
    mode: DiagnosticMode,
    locale_pack: Option<&radix::module::locale::LocalePack>,
) {
    let mut advice = Vec::new();
    let mut audit = Vec::new();
    let mut placement = Vec::new();
    let mut failures = Vec::new();
    for unit in lowering_units(config, input_path) {
        match super::mir::with_check_lowered_package_mir(config, &unit, |lowered| {
            let rows = super::device::split_advice(lowered, device, partitions)?;
            let syncs = super::device::span_audit(lowered, device, partitions)?;
            let table = super::device::placement_table(lowered, device, partitions)?;
            Ok((rows, syncs, table))
        }) {
            Ok(Ok((rows, syncs, table))) => {
                advice.extend(rows);
                audit.extend(syncs);
                placement.extend(table);
            }
            Ok(Err(errors)) | Err(errors) => failures.extend(errors),
        }
    }
    if !failures.is_empty() {
        exit_does_not_lower(&failures, input_path, mode, locale_pack);
    }
    // A library's modules lower one by one and may share kernels.
    advice.sort_by(|a, b| a.kernel.cmp(&b.kernel));
    advice.dedup();
    audit.sort();
    audit.dedup();
    placement.sort();
    placement.dedup();
    let file = input_path.display().to_string();
    let diagnostics: Vec<_> = advice
        .iter()
        .map(|row| super::device::advice_diagnostic(row, device, partitions, &file))
        .chain(
            audit
                .iter()
                .map(|row| super::device::audit_diagnostic(row, device, partitions, &file)),
        )
        .collect();
    if mode == DiagnosticMode::Diagnostics {
        if !diagnostics.is_empty() {
            let rendered = match locale_pack {
                Some(pack) => radix::module::diagnostics::render_expanded_diagnostics_with_pack(
                    &diagnostics,
                    pack,
                )
                .unwrap_or_else(|err| {
                    eprintln!("error: failed to render reader-pack diagnostics: {err}");
                    crate::exit_status::exit_software();
                }),
                None => radix::module::diagnostics::render_expanded_diagnostics(&diagnostics),
            };
            eprintln!("{rendered}");
        }
    } else {
        for diag in &diagnostics {
            eprintln!("{}", radix::module::diagnostics::render_plain(diag));
        }
    }
    // The placement table is a table, not a diagnostic: one line per tensor.
    if !placement.is_empty() {
        eprintln!("placement ({partitions} partitions):");
        for line in &placement {
            eprintln!("  {line}");
        }
    }
}

/// Print one `info` row per kernel (its tier) and one per device region (`do
/// device` / `fac machina`) of the checked package. A region row names what
/// the block shares with the code around it, classified by writes, its kernel
/// call sites, and its host syncs. The rows are advice (`INFO001`) and never
/// change the exit status.
fn print_region_report(config: &radix::module::Config, input_path: &Path, mode: DiagnosticMode) {
    let Ok(package) = radix::program::analyze::analyze_package(
        config,
        input_path,
        &super::faber_analysis_seams(),
    ) else {
        return;
    };
    let mut rows = Vec::new();
    for node in package.nodes.values() {
        let analysis = &node.analysis;
        // One row per kernel with its tier: the author's `@ kernel relaxed`
        // or `@ kernel unsafe`, or the default tier. Under `--safe` the
        // analysis already reads every kernel as the default tier.
        for kernel in &analysis.kernel_requirements.kernels {
            let message = format!("kernel {}: {} tier", kernel.name, kernel.tier.label());
            let mut row = radix::module::diagnostics::Diagnostic::new(
                radix::module::diagnostics::Severity::Info,
                message,
            )
            .with_phase(radix::module::diagnostics::DiagnosticPhase::Codegen)
            .with_code("INFO001");
            node.path.display().to_string().clone_into(&mut row.file);
            rows.push(row);
        }
        let regions = &analysis.kernel_requirements.regions;
        if regions.is_empty() {
            continue;
        }
        let source = std::fs::read_to_string(&node.path).unwrap_or_default();
        for region in regions {
            let line = 1 + source
                .as_bytes()
                .iter()
                .take(region.span.start as usize)
                .filter(|byte| **byte == b'\n')
                .count();
            let render =
                |bindings: &[radix::module::semantic::passes::device_region::RegionBinding]| {
                    if bindings.is_empty() {
                        return "none".to_owned();
                    }
                    bindings
                        .iter()
                        .map(|binding| format!("{}: {}", binding.name, binding.ty))
                        .collect::<Vec<_>>()
                        .join(", ")
                };
            let facts = &region.facts;
            let launches = if facts.guarded_launches > 0 {
                format!(
                    "{} (up to {})",
                    facts.loop_launches,
                    facts.loop_launches + facts.guarded_launches
                )
            } else {
                facts.loop_launches.to_string()
            };
            let message = format!(
                "device region at line {line}: inputs [{}]; in-outs [{}]; launches per iteration {launches}, {} outside the loop; {} host syncs",
                render(&facts.inputs),
                render(&facts.in_outs),
                facts.other_launches,
                region.host_syncs,
            );
            let mut row = radix::module::diagnostics::Diagnostic::new(
                radix::module::diagnostics::Severity::Info,
                message,
            )
            .with_phase(radix::module::diagnostics::DiagnosticPhase::Codegen)
            .with_code("INFO001");
            node.path.display().to_string().clone_into(&mut row.file);
            rows.push(row);
        }
    }
    for row in &rows {
        match mode {
            DiagnosticMode::Diagnostics => {
                eprintln!(
                    "{}",
                    radix::module::diagnostics::render_expanded_diagnostics(std::slice::from_ref(
                        row
                    ))
                );
            }
            DiagnosticMode::Normal => {
                eprintln!("{}", radix::module::diagnostics::render_plain(row))
            }
        }
    }
}

#[cfg(not(all(feature = "runner", feature = "device-runtime")))]
fn print_device_advice(
    _: &radix::module::Config,
    _: &Path,
    _: (),
    _: u32,
    _: DiagnosticMode,
    _: Option<&radix::module::locale::LocalePack>,
) {
}

/// What `check --lower` hands to the package-MIR driver: the input itself,
/// or, for a package directory/manifest with no entry unit, each of its
/// modules.
#[cfg(feature = "runner")]
fn lowering_units(config: &radix::module::Config, input_path: &Path) -> Vec<PathBuf> {
    if input_path.is_file()
        && input_path
            .file_name()
            .is_none_or(|name| name != MANIFEST_FILE)
    {
        return vec![input_path.to_path_buf()];
    }
    match radix::program::analyze::analyze_package(
        config,
        input_path,
        &super::faber_analysis_seams(),
    ) {
        Ok(package) if !package.nodes.values().any(|node| node.is_entry) => package
            .nodes
            .values()
            .map(|node| node.path.clone())
            .collect(),
        _ => vec![input_path.to_path_buf()],
    }
}

/// Execute package emission and print generated code to stdout.
///
/// Unlike `cmd_build`, this command does not materialize the generated Cargo
/// crate. It is a compiler-inspection surface for the assembled backend output.
pub fn cmd_emit_package(command: radix::services::EmitCommand, format: bool, linter: bool) {
    // SGR-U2: `.fab` metal-text emits take the package lane's merged
    // consumer, mirroring the build branch — radix-program::emit has no
    // MirMetal artifact plan and reports the input as unsupported.
    if use_package_compiler_from_args(command.target, &command.input, command.package)
        && command.target == Target::MirMetal
    {
        #[cfg(not(feature = "mir-metal"))]
        exit_target_feature_unavailable(Target::MirMetal);
        #[cfg(feature = "mir-metal")]
        {
            let path = super::absolutize_path(std::path::Path::new(&command.input[0]));
            let (config, locale_pack) = match config_with_locale(
                command.target,
                &path,
                command.locale.as_deref(),
                command.diagnostics_locale.as_deref(),
            ) {
                Ok(selection) => selection,
                Err(diag) => {
                    let diag = super::attach_path_diagnostic_context(diag);
                    fail_with_diagnostic(&diag)
                }
            };
            let code = match compile_package_metal_text(&config, &path) {
                Ok(code) => code,
                Err(diagnostics) => report_diagnostics_and_exit(
                    &diagnostics,
                    locale_pack.as_ref(),
                    "metal-text package emit failed",
                ),
            };
            if command.reflection {
                eprintln!("error: target does not expose GPU reflection metadata");
                crate::exit_status::exit_usage();
            }
            let code = crate::postprocess::postprocess_code(code, command.target, format, linter);
            if let Some(out) = command.output {
                crate::postprocess::write_text_artifact(&out, &code);
            } else {
                print!("{code}");
            }
            return;
        }
    }
    let (result, locale_pack) = compile_package_input(
        &command.input,
        command.package,
        command.target,
        command.locale.as_deref(),
        command.diagnostics_locale.as_deref(),
    );

    crate::postprocess::print_diagnostics(
        &result.diagnostics,
        command.diagnostic_mode,
        locale_pack.as_ref(),
    );

    let Some(output) = result.output else {
        fail_compilation(&result.diagnostics);
    };

    if command.reflection {
        #[cfg(not(feature = "runner"))]
        exit_runner_feature_unavailable();
        #[cfg(feature = "runner")]
        {
            let reflection_json = match radix::services::output_reflection_json(&output) {
                Ok(Some(json)) => json,
                Ok(None) => {
                    eprintln!("error: target does not expose GPU reflection metadata");
                    crate::exit_status::exit_usage();
                }
                Err(err) => {
                    eprintln!("error: failed to serialize GPU reflection metadata: {err}");
                    crate::exit_status::exit_software();
                }
            };
            if let Some(path) = command.output {
                crate::postprocess::write_text_artifact(&path, &reflection_json);
            } else {
                println!("{reflection_json}");
            }
            return;
        }
    }

    if let Some(path) = command.output {
        crate::postprocess::write_output_artifact(&path, output, command.target, format, linter);
        return;
    }

    let code = crate::postprocess::postprocess_code(
        radix::services::output_code(output),
        command.target,
        format,
        linter,
    );
    print!("{code}");
}

/// Route a package build to the emitter that owns the target.
///
/// `clif` is emitted by the package-MIR object emitter, whose capability
/// classifier names the package shape when it is outside the CLIF subset;
/// every other target keeps the plan-driven package emitter.
#[cfg_attr(not(feature = "mir-clif"), allow(unused_variables))]
pub(super) fn package_compile_result(
    config: &radix::module::Config,
    input_path: &Path,
    target: Target,
) -> CompileResult {
    #[cfg(feature = "mir-clif")]
    if target == Target::MirClif {
        return radix::program::emit::compile_package_clif(
            config,
            input_path,
            &super::mir::faber_mir_seams(),
        );
    }
    radix::program::emit::compile_package(config, input_path, &super::faber_emission_seams())
}

/// Compile a lone `.fab` file that imports sibling modules through the
/// package-MIR `clif` emitter — the route `faber build -t clif` gives it.
/// A single-file object carries no linked sibling code, so the imported call
/// would decline as CODEGEN001 `external definition call`; the package-MIR
/// emitter links the import graph and lowers the call internally.
///
/// The corpus `clif` lane uses this seam so its file-mode rows exercise the
/// same object the product builds for the same input.
#[cfg(feature = "mir-clif")]
pub fn compile_clif_package_input(input: &Path) -> CompileResult {
    let path = super::absolutize_path(input);
    let (config, _) = match config_with_locale(Target::MirClif, &path, None, None) {
        Ok(selection) => selection,
        Err(diagnostic) => {
            let diagnostic = super::attach_path_diagnostic_context(diagnostic);
            return CompileResult {
                output: None,
                diagnostics: vec![*diagnostic],
            };
        }
    };
    package_compile_result(&config, &path, Target::MirClif)
}

/// Compile a package `.fab` entry to metal-text through the interpreted
/// merged consumer: the package-MIR driver links library imports into the
/// merged program and registers their concrete device instances, and the
/// emit joins those registrations into the entry-role map the same way the
/// module driver's text-target handoff does. Local entry roles come from the
/// lowered unit's explicit metadata carrier, never from name inference.
///
/// Returns the generated Metal Shading Language source.
#[cfg(feature = "mir-metal")]
pub(super) fn compile_package_metal_text(
    config: &radix::module::Config,
    input_path: &Path,
) -> Result<String, Vec<radix::module::Diagnostic>> {
    radix::program::mir::with_interpreted_lowered_package_mir(
        config,
        input_path,
        &super::mir::faber_mir_seams(),
        |lowered| -> Result<String, Vec<radix::module::Diagnostic>> {
            let mut local_entry_roles = radix::module::semantic::FxHashMap::default();
            for function in &lowered.validated.program().functions {
                if let Some(source) = function.source
                    && let Some(role) = lowered
                        .device_function_metadata
                        .get(&function.id)
                        .and_then(radix_mir::device::MirDeviceFunctionMetadata::entry_role)
                {
                    local_entry_roles.insert(source, role);
                }
            }
            let device_roles = radix_mir::device::merge_imported_entry_roles(
                local_entry_roles,
                &radix::program::mir::imported_entry_facts(lowered),
                lowered.validated.program(),
            )
            .map_err(|error| {
                vec![
                    crate::package_diagnostic_error(error.message())
                        .with_file(input_path.display().to_string())
                        .with_arg("issue", error.issue()),
                ]
            })?;
            radix_mir_metal::emit_metal_text_probe_with_reflection_and_function_metadata(
                &device_roles,
                &lowered.device_function_metadata,
                &lowered.validated,
                &lowered.interner,
            )
            .map(|output| output.source)
            .map_err(|error| {
                vec![
                    crate::package_diagnostic_error(error.message)
                        .with_file(input_path.display().to_string())
                        .with_arg("issue", error.issue)
                        .with_arg("shape", error.shape),
                ]
            })
        },
    )?
}

fn compile_package_input(
    input: &[String],
    force_package: bool,
    target: Target,
    locale: Option<&str>,
    diagnostics_locale: Option<&str>,
) -> (CompileResult, Option<radix::module::locale::LocalePack>) {
    if input.is_empty() || input[0] == "-" {
        eprintln!("error: package compilation requires a path input");
        crate::exit_status::exit_usage();
    }

    let path = super::absolutize_path(std::path::Path::new(&input[0]));
    let package = use_package_compiler_from_args(target, input, force_package);
    if !package {
        eprintln!("error: expected a package directory, manifest, or entry file");
        crate::exit_status::exit_usage();
    }

    let (config, locale_pack) = match config_with_locale(target, &path, locale, diagnostics_locale)
    {
        Ok(selection) => selection,
        Err(diag) => {
            let diag = super::attach_path_diagnostic_context(diag);
            fail_with_diagnostic(&diag)
        }
    };
    // The `clif` package lane is the package-MIR object emitter
    // (`package_compile_result`); the plan-driven package emitter has no
    // `MirClif` artifact assembly.
    #[cfg(feature = "mir-clif")]
    if target == Target::MirClif {
        return (package_compile_result(&config, &path, target), locale_pack);
    }
    (
        radix::program::emit::compile_package(&config, &path, &super::faber_emission_seams()),
        locale_pack,
    )
}

fn is_permissive_check_code(code: Option<&'static str>) -> bool {
    matches!(
        code,
        Some("SEM001" | "SEM002" | "SEM003" | "SEM004" | "SEM006")
    )
}

#[cfg(test)]
#[path = "cmd_test.rs"]
mod tests;
