use radix::codegen::Target;
use radix::tool::DiagnosticMode;
use radix::{CompileResult, Output};
use std::fs;
use std::path::{Path, PathBuf};

use crate::input_shape::locale_without_package_error;

#[cfg(feature = "hir-fhir")]
use super::build_package_fhir;
use super::cargo::{
    emit_generated_crate_with_runtime_plan, invoke_cargo_build, lock_generated_crate_build,
};
#[cfg(feature = "hir-go")]
use super::compile_package_go;
#[cfg(feature = "hir-go")]
use super::go_build::{emit_go_module_with_postprocess, invoke_go_build, GoBuildLayout};
use super::manifest::{manifest_build_target, FaberManifest};
use super::{
    build_host_program, build_package_fmir_binary_bundle, build_package_fmir_image,
    build_package_fmir_text_image, build_package_mir_artifact, check_package, compile_package,
    config_with_locale, discover_build_layout, package_host_selection_diagnostic,
    package_rust_runtime_plan, read_manifest, BuildLayout, LlvmHostProfile, MANIFEST_FILE,
};

/// Print `message` and terminate.
fn fail(message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(1);
}

/// Report a diagnostic's message and terminate.
fn fail_with_diagnostic(diag: &radix::Diagnostic) -> ! {
    eprintln!("error: {}", diag.message);
    std::process::exit(1);
}

/// Print `diagnostics` in normal mode, then terminate with `message`.
fn report_diagnostics_and_exit(
    diagnostics: &[radix::Diagnostic],
    locale_pack: Option<&radix::locale::LocalePack>,
    message: &str,
) -> ! {
    radix::tool::print_diagnostics(diagnostics, DiagnosticMode::Normal, locale_pack);
    eprintln!("{message}");
    std::process::exit(1);
}

/// Execute the user-facing `faber build` command.
///
/// Package Rust builds emit a generated Cargo crate and then delegate binary
/// production to Cargo. Direct-file builds and non-Rust targets keep the legacy
/// single-output behavior so package ergonomics do not change unrelated command
/// paths.
pub fn cmd_build(command: radix::tool::BuildCommand, format: bool, linter: bool) {
    if let Some(message) = locale_without_package_error(
        command.locale.as_deref(),
        std::slice::from_ref(&command.input),
        command.package,
    ) {
        eprintln!("error: {message}");
        std::process::exit(1);
    }

    let input_path = PathBuf::from(&command.input);
    let target = resolve_build_target(&command, &input_path);
    let is_package = use_package_compiler(target, &input_path, command.package);
    let warn_policy = radix::driver::WarnPolicy {
        deny_all_warnings: command.deny_warnings,
        deny_codes: command.deny_codes,
    };
    let (config, locale_pack) = if is_package {
        match config_with_locale(
            target,
            &input_path,
            command.locale.as_deref(),
            command.diagnostics_locale.as_deref(),
        ) {
            Ok((config, pack)) => (config.with_warn_policy(warn_policy), pack),
            Err(diag) => fail_with_diagnostic(&diag),
        }
    } else {
        let config = match super::locale::default_config_with_locale(target) {
            Ok(config) => config.with_dev_stdlib().with_warn_policy(warn_policy),
            Err(diag) => fail_with_diagnostic(&diag),
        };
        let locale_pack = config.locale_pack.clone();
        (config, locale_pack)
    };

    if is_package && target == Target::MirScena {
        let artifact = match build_package_mir_artifact(&config, &input_path, &[]) {
            Ok(artifact) => artifact,
            Err(diagnostics) => report_diagnostics_and_exit(
                &diagnostics,
                locale_pack.as_ref(),
                "scena artifact build failed",
            ),
        };
        println!("{}", artifact.manifest_path.display());
        return;
    }

    if is_package && target == Target::MirFmir {
        let image = match build_package_fmir_text_image(&config, &input_path, &[]) {
            Ok(image) => image,
            Err(diagnostics) => report_diagnostics_and_exit(
                &diagnostics,
                locale_pack.as_ref(),
                "fmir-text image build failed",
            ),
        };
        println!("{}", image.image_path.display());
        return;
    }

    if is_package && target == Target::MirFmirBinary {
        let image = match build_package_fmir_image(&config, &input_path, &[]) {
            Ok(image) => image,
            Err(diagnostics) => report_diagnostics_and_exit(
                &diagnostics,
                locale_pack.as_ref(),
                "fmir image build failed",
            ),
        };
        println!("{}", image.image_path.display());
        return;
    }

    if is_package && target == Target::MirFmirBundle {
        let bundle =
            match build_package_fmir_binary_bundle(&config, &input_path, &[], command.release) {
                Ok(bundle) => bundle,
                Err(diagnostics) => report_diagnostics_and_exit(
                    &diagnostics,
                    locale_pack.as_ref(),
                    "fmir-bin bundle build failed",
                ),
            };
        println!("{}", bundle.entrypoint_path.display());
        return;
    }

    if is_package && target == Target::HirFhir {
        #[cfg(not(feature = "hir-fhir"))]
        {
            eprintln!(
                "error: target `fhir` is not available in this faber build; rebuild with feature `hir-fhir`"
            );
            std::process::exit(1);
        }
        #[cfg(feature = "hir-fhir")]
        let artifact = match build_package_fhir(&config, &input_path) {
            Ok(artifact) => artifact,
            Err(diagnostics) => report_diagnostics_and_exit(
                &diagnostics,
                locale_pack.as_ref(),
                "fhir package build failed",
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
        let profile = if command.release {
            LlvmHostProfile::Release
        } else {
            LlvmHostProfile::Debug
        };
        let build = match build_host_program(&config, &input_path, profile) {
            Ok(build) => build,
            Err(diagnostics) => report_diagnostics_and_exit(
                &diagnostics,
                locale_pack.as_ref(),
                "llvm-host build failed",
            ),
        };
        println!("{}", build.binary_path.display());
        return;
    }

    // U6-D: package Wasm builds emit one package-aware module per unit and
    // print the module output directory. Linking/running is the product
    // host's job (`faber-host-wasm::WasmRtV1Host::run_package`); the build
    // artifact is the module set + link manifest.
    if is_package && target == Target::MirWasmBinary {
        #[cfg(not(feature = "mir-wasm"))]
        {
            eprintln!(
                "error: target `wasm` is not available in this faber build; rebuild with feature `mir-wasm`"
            );
            std::process::exit(1);
        }
        #[cfg(feature = "mir-wasm")]
        {
            let output_dir = match discover_build_layout(&input_path) {
                Ok(layout) => layout
                    .package_root
                    .join("target")
                    .join("faber")
                    .join("wasm"),
                Err(d) => fail_with_diagnostic(&d),
            };
            let options = super::wasm::PackageWasmOptions::new(output_dir);
            let build = match super::wasm::build_package_wasm(&config, &input_path, &options) {
                Ok(build) => build,
                Err(diagnostics) => report_diagnostics_and_exit(
                    &diagnostics,
                    locale_pack.as_ref(),
                    "wasm package build failed",
                ),
            };
            println!("{}", build.manifest.output.display());
            return;
        }
    }

    if is_package && target == Target::HirTypeScript {
        let layout = match discover_build_layout(&input_path) {
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
    }

    // G6 GO3/GO4: package Go builds write target/faber/go and invoke `go build`.
    // The multi-module file collection travels inside the compile result
    // itself (FBR-P2-003) — no hidden thread-local state.
    if is_package && target == radix::codegen::Target::HirGo {
        #[cfg(not(feature = "hir-go"))]
        {
            eprintln!(
                "error: target `go` is not available in this faber build; rebuild with feature `hir-go`"
            );
            std::process::exit(1);
        }
        #[cfg(feature = "hir-go")]
        {
            let go_result = compile_package_go(&config, &input_path);
            radix::tool::print_diagnostics(
                &go_result.compile_result.diagnostics,
                DiagnosticMode::Normal,
                locale_pack.as_ref(),
            );
            let Some(output) = go_result.compile_result.output else {
                fail("compilation failed");
            };
            let layout = match discover_build_layout(&input_path) {
                Ok(l) => l,
                Err(d) => fail_with_diagnostic(&d),
            };
            let go_layout = GoBuildLayout::from_package(&layout);
            let code = output_code(output);
            if let Err(d) = emit_go_module_with_postprocess(
                &go_layout,
                &code,
                &go_result.go_modules,
                format,
                linter,
            ) {
                fail_with_diagnostic(&d);
            }
            match invoke_go_build(&go_layout) {
                Ok(binary_path) => {
                    println!("{}", binary_path.display());
                    return;
                }
                Err(d) => fail_with_diagnostic(&d),
            }
        }
    }

    let result = if is_package {
        compile_package(&config, &input_path)
    } else {
        let compiler = radix::Compiler::new(config.clone());
        compiler.compile(&input_path)
    };

    radix::tool::print_diagnostics(
        &result.diagnostics,
        DiagnosticMode::Normal,
        locale_pack.as_ref(),
    );

    let Some(output) = result.output else {
        fail("compilation failed");
    };

    // Binary wasm output cannot travel the text `output_code` path: write the
    // module bytes to the output path (single-file wasm builds and any other
    // `Output::Wasm` producer). Package wasm builds are routed above.
    if let Output::Wasm(out) = &output {
        let output_path = radix::tool::build_output_path(
            &command.out_dir,
            &input_path,
            target,
            is_package,
        );
        write_output_file(&output_path, &out.bytes);
        println!("{}", output_path.display());
        return;
    }

    // Package Rust builds own a generated crate under target/faber/ and let
    // Cargo place artifacts in sibling debug/release directories.
    if is_package && target == radix::codegen::Target::HirRust {
        let layout = match discover_build_layout(&input_path) {
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
        let mut runtime_plan = match package_rust_runtime_plan(&config, &input_path) {
            Ok(plan) => plan,
            Err(diagnostics) => report_diagnostics_and_exit(
                &diagnostics,
                locale_pack.as_ref(),
                "runtime plan failed",
            ),
        };
        if let Some(diagnostic) =
            package_host_selection_diagnostic(&runtime_plan, &layout.manifest_path)
        {
            report_diagnostics_and_exit(
                &[diagnostic],
                locale_pack.as_ref(),
                "runtime plan failed",
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
            Err(diagnostics) => report_diagnostics_and_exit(
                &diagnostics,
                locale_pack.as_ref(),
                "library dependency graph failed",
            ),
        }
        let package_code =
            crate::postprocess::postprocess_code(output_code(output), target, format, linter);
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

    let code = output_code(output);

    // Single-file Rust builds that need faber-runtime / tokio (from HIR facts,
    // not emitted-text sniffing) emit a generated Cargo crate under
    // `target/faber/`. Programs without those deps keep the bare `.rs` path.
    if target == radix::codegen::Target::HirRust {
        match single_file_rust_runtime_plan(&config, &input_path) {
            Ok(plan) if plan.requires_generated_crate() => {
                let package_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                let stem = input_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .filter(|s| !s.is_empty())
                    .unwrap_or("faber_out");
                let layout = BuildLayout::from_package_root(&package_root, stem);
                // Same per-package lock as package builds: the emit + cargo
                // sequence shares `target/faber/` under the current directory.
                let _build_lock = match lock_generated_crate_build(&layout) {
                    Ok(lock) => lock,
                    Err(d) => fail_with_diagnostic(&d),
                };
                let generated_code =
                    crate::postprocess::postprocess_code(code.clone(), target, format, linter);
                let binary_path = emit_crate_and_build(
                    &layout,
                    &generated_code,
                    None,
                    &plan,
                    command.release,
                );
                println!("{}", binary_path.display());
                return;
            }
            Ok(_) => {}
            Err(diagnostics) => report_diagnostics_and_exit(
                &diagnostics,
                locale_pack.as_ref(),
                "runtime plan failed",
            ),
        }
    }

    // Legacy single-file path (direct .fab files, other targets, or --out-dir override cases)
    let output_path =
        radix::tool::build_output_path(&command.out_dir, &input_path, target, is_package);

    let code = crate::postprocess::postprocess_code(code, target, format, linter);
    write_output_file(&output_path, code.as_bytes());

    println!("{}", output_path.display());
}

/// Emit the generated crate with its runtime plan and delegate binary
/// production to Cargo, returning the produced binary path.
fn emit_crate_and_build(
    layout: &BuildLayout,
    code: &str,
    meta: Option<&FaberManifest>,
    plan: &super::RustRuntimePlan,
    release: bool,
) -> PathBuf {
    if let Err(d) = emit_generated_crate_with_runtime_plan(layout, code, meta, plan) {
        fail_with_diagnostic(&d);
    }
    match invoke_cargo_build(layout, release) {
        Ok(path) => path,
        Err(d) => fail_with_diagnostic(&d),
    }
}

/// Create the output path's parent and write `bytes`, exiting with a
/// structured error message on failure.
fn write_output_file(output_path: &Path, bytes: &[u8]) {
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).unwrap_or_else(|err| {
            eprintln!(
                "error: failed to create '{}': {}",
                parent.display(),
                err
            );
            std::process::exit(1);
        });
    }
    fs::write(output_path, bytes).unwrap_or_else(|err| {
        eprintln!(
            "error: failed to write '{}': {}",
            output_path.display(),
            err
        );
        std::process::exit(1);
    });
}

/// Structured runtime plan for a single `.fab` file (no package manifest).
#[cfg(feature = "hir-rust")]
fn single_file_rust_runtime_plan(
    config: &radix::Config,
    input_path: &Path,
) -> Result<super::RustRuntimePlan, Vec<radix::Diagnostic>> {
    super::rust_target::single_file_rust_runtime_plan(config, input_path)
}

#[cfg(not(feature = "hir-rust"))]
fn single_file_rust_runtime_plan(
    _config: &radix::Config,
    input_path: &Path,
) -> Result<super::RustRuntimePlan, Vec<radix::Diagnostic>> {
    Err(vec![crate::package_diagnostic_error(
        "target `rust` is not available in this faber build; rebuild with feature `hir-rust`",
    )
    .with_file(input_path.display().to_string())
    .with_arg("issue", "package_target_unavailable")
    .with_arg("target", "rust")])
}

/// Resolve the build target: an explicit CLI target wins, otherwise the
/// package manifest's `build.target`. Unlike [`resolve_check_target`], a
/// manifest read failure is fatal here rather than silently falling back.
fn resolve_build_target(command: &radix::tool::BuildCommand, input_path: &Path) -> Target {
    if command.target_explicit {
        return command.target;
    }

    let Ok(layout) = discover_build_layout(input_path) else {
        return command.target;
    };
    if !layout.manifest_path.exists() {
        return command.target;
    }

    let manifest = read_manifest(&layout.manifest_path)
        .unwrap_or_else(|diag| fail_with_diagnostic(&diag));
    manifest_build_target(manifest.build.target.as_deref(), &layout.manifest_path).unwrap_or_else(
        |diag| fail_with_diagnostic(&diag),
    )
}

/// Resolve the target for `faber check` from the package manifest.
///
/// Falls back to `Target::HirRust` when the manifest cannot be read or the
/// input is not a package — same safe default as the pre-manifest behaviour.
fn resolve_check_target(input_path: &Path) -> Target {
    let Ok(layout) = discover_build_layout(input_path) else {
        return Target::HirRust;
    };
    if !layout.manifest_path.exists() {
        return Target::HirRust;
    }
    let Ok(manifest) = read_manifest(&layout.manifest_path) else {
        return Target::HirRust;
    };
    manifest_build_target(manifest.build.target.as_deref(), &layout.manifest_path)
        .unwrap_or(Target::HirRust)
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
/// Single `.fab` files use the package loader for Rust and package-image
/// targets so `norma:*` imports resolve through the package graph. MIR
/// probe targets and HIR inspection targets (`go`, `ts`) use the radix
/// single-file path, matching `radix emit`.
fn use_package_compiler(target: Target, path: &std::path::Path, force_package: bool) -> bool {
    if force_package {
        return true;
    }
    if path.is_dir() || path.file_name().and_then(|name| name.to_str()) == Some(MANIFEST_FILE) {
        return true;
    }
    if path.extension().is_some_and(|ext| ext == "fab") {
        return matches!(
            target,
            Target::HirRust
                | Target::MirScena
                | Target::MirFmir
                | Target::MirFmirBinary
                | Target::MirFmirBundle
                | Target::HirFhir
                | Target::MirLlvmHost
                | Target::MirWasmBinary
        );
    }
    false
}

/// CLI-argument variant of target-aware package routing.
///
/// Standard-input builds (`-`) cannot be package builds because package
/// discovery needs filesystem paths for imports, manifests, and generated
/// layouts.
pub fn use_package_compiler_from_args(
    target: Target,
    input: &[String],
    force_package: bool,
) -> bool {
    if input.is_empty() || input[0] == "-" {
        return false;
    }
    let path = std::path::Path::new(&input[0]);
    use_package_compiler(target, path, force_package)
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
    let path = std::path::Path::new(&input[0]);
    should_treat_as_package(path)
}

/// Execute the package-aware `faber check` command.
///
/// The permissive mode intentionally downgrades only unresolved/import-driven
/// semantic errors; manifest, I/O, parse, and package-policy errors remain
/// fatal because they prevent reliable package graph construction.
pub fn cmd_check_package(command: radix::tool::CheckCommand) {
    if command.input.is_empty() || command.input[0] == "-" {
        eprintln!("error: package checking requires a path input");
        std::process::exit(1);
    }

    let input_path = std::path::PathBuf::from(&command.input[0]);

    // Resolve target from faber.toml so AIR-lane exempla with MIR-backed
    // targets (e.g. fmir-text) do not trigger TARGETLANE001.
    let check_target = resolve_check_target(&input_path);

    let (config, locale_pack) = match config_with_locale(
        check_target,
        &input_path,
        command.locale.as_deref(),
        command.diagnostics_locale.as_deref(),
    ) {
        Ok((config, pack)) => (
            config.with_warn_policy(radix::driver::WarnPolicy {
                deny_all_warnings: command.deny_warnings,
                deny_codes: command.deny_codes,
            }),
            pack,
        ),
        Err(diag) => fail_with_diagnostic(&diag),
    };
    let mut diagnostics = check_package(&config, &input_path);
    radix::apply_warn_policy(&mut diagnostics, &config.warn_policy);

    let mut fatal_errors = 0usize;
    let mut downgraded = 0usize;
    if command.diagnostic_mode == DiagnosticMode::Diagnostics && !diagnostics.is_empty() {
        match locale_pack.as_ref() {
            Some(pack) => {
                match radix::diagnostics::render_expanded_diagnostics_with_pack(&diagnostics, pack)
                {
                    Ok(rendered) => eprintln!("{rendered}"),
                    Err(err) => {
                        eprintln!("error: failed to render reader-pack diagnostics: {err}");
                        std::process::exit(1);
                    }
                }
            }
            None => eprintln!(
                "{}",
                radix::diagnostics::render_expanded_diagnostics(&diagnostics)
            ),
        }
    }

    for diag in &diagnostics {
        let downgraded_error =
            command.permissive && diag.is_error() && is_permissive_check_code(diag.code);
        if command.diagnostic_mode == DiagnosticMode::Normal {
            let display = if downgraded_error {
                diag.clone()
                    .with_severity(radix::diagnostics::Severity::Warning)
            } else {
                diag.clone()
            };
            eprintln!("{}", radix::diagnostics::render_plain(&display));
        }
        if diag.is_error() {
            if downgraded_error {
                downgraded += 1;
            } else {
                fatal_errors += 1;
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

    if fatal_errors == 0 {
        eprintln!("ok: {}", input_path.display());
    } else {
        std::process::exit(1);
    }
}

/// Execute package emission and print generated code to stdout.
///
/// Unlike `cmd_build`, this command does not materialize the generated Cargo
/// crate. It is a compiler-inspection surface for the assembled backend output.
pub fn cmd_emit_package(command: radix::tool::EmitCommand, format: bool, linter: bool) {
    let (result, locale_pack) = compile_package_input(
        &command.input,
        command.package,
        command.target,
        command.locale.as_deref(),
        command.diagnostics_locale.as_deref(),
    );

    radix::tool::print_diagnostics(
        &result.diagnostics,
        command.diagnostic_mode,
        locale_pack.as_ref(),
    );

    let Some(output) = result.output else {
        fail("compilation failed");
    };

    if command.reflection {
        let reflection_json = match radix::tool::output_reflection_json(&output) {
            Ok(Some(json)) => json,
            Ok(None) => {
                eprintln!("error: target does not expose GPU reflection metadata");
                std::process::exit(1);
            }
            Err(err) => {
                eprintln!("error: failed to serialize GPU reflection metadata: {err}");
                std::process::exit(1);
            }
        };
        if let Some(path) = command.output {
            radix::tool::write_text_artifact(&path, &reflection_json);
        } else {
            println!("{}", reflection_json);
        }
        return;
    }

    if let Some(path) = command.output {
        crate::postprocess::write_output_artifact(&path, output, command.target, format, linter);
        return;
    }

    // Binary wasm output has no text form: emit the entry module bytes to
    // stdout (the same behavior the single-file emit command uses).
    match output {
        Output::Wasm(out) => {
            use std::io::Write;
            std::io::stdout()
                .write_all(&out.bytes)
                .unwrap_or_else(|err| {
                    eprintln!("error: failed to write wasm bytes to stdout: {err}");
                    std::process::exit(1);
                });
        }
        output => {
            let code = crate::postprocess::postprocess_code(
                output_code(output),
                command.target,
                format,
                linter,
            );
            print!("{code}");
        }
    }
}

fn compile_package_input(
    input: &[String],
    force_package: bool,
    target: Target,
    locale: Option<&str>,
    diagnostics_locale: Option<&str>,
) -> (CompileResult, Option<radix::locale::LocalePack>) {
    if input.is_empty() || input[0] == "-" {
        eprintln!("error: package compilation requires a path input");
        std::process::exit(1);
    }

    let path = std::path::PathBuf::from(&input[0]);
    let package = use_package_compiler_from_args(target, input, force_package);
    if !package {
        eprintln!("error: expected a package directory, manifest, or entry file");
        std::process::exit(1);
    }

    let (config, locale_pack) = match config_with_locale(target, &path, locale, diagnostics_locale) {
        Ok(selection) => selection,
        Err(diag) => fail_with_diagnostic(&diag),
    };
    (compile_package(&config, &path), locale_pack)
}

fn is_permissive_check_code(code: Option<&'static str>) -> bool {
    matches!(
        code,
        Some("SEM001" | "SEM002" | "SEM003" | "SEM004" | "SEM006")
    )
}

fn output_code(output: Output) -> String {
    match output {
        Output::Rust(out) => out.code,
        Output::Faber(out) => out.code,
        Output::TypeScript(out) => out.code,
        Output::Go(out) => out.code,
        Output::WasmText(out) => out.code,
        // Unreachable: `cmd_build` writes binary wasm module bytes to the
        // output path, package wasm builds route through the package-wasm
        // builder, and `cmd_emit_package` rejects binary wasm on the stdout
        // path with a recorded diagnostic — binary bytes never reach this
        // text-only extractor.
        Output::Wasm(_) => unreachable!(
            "binary Wasm output is routed before output_code: package builds use the \
             package-wasm path, emit uses `--output`, build writes module bytes"
        ),
        Output::LlvmText(out) => out.code,
        Output::MetalText(out) => out.code,
        Output::WgslText(out) => out.code,
        Output::Sexp(out) => out.code,
        Output::Swift(out) => out.code,
    }
}
