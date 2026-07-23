use radix::codegen::Target;
use radix::driver::Config;
use radix::tool::DiagnosticMode;
use radix::{CompileResult, Output};
use std::path::Path;

use crate::input_shape::reader_locale_without_package_error;

use super::cargo::{emit_generated_crate_with_runtime_plan, invoke_cargo_build};
use super::go_build::{emit_go_module, invoke_go_build, GoBuildLayout};
use super::manifest::manifest_build_target;
use super::{
    build_package_fmir_binary_bundle, build_package_fmir_image, build_package_fmir_text_image,
    build_package_mir_artifact, check_package, compile_package, config_with_reader_locale,
    discover_build_layout, package_host_selection_diagnostic, package_rust_runtime_plan,
    read_manifest, BuildLayout, MANIFEST_FILE,
};

/// Execute the user-facing `faber build` command.
///
/// Package Rust builds emit a generated Cargo crate and then delegate binary
/// production to Cargo. Direct-file builds and non-Rust targets keep the legacy
/// single-output behavior so package ergonomics do not change unrelated command
/// paths.
pub fn cmd_build(command: radix::tool::BuildCommand) {
    use std::fs;
    use std::path::PathBuf;

    if let Some(message) = reader_locale_without_package_error(
        command.reader_locale.as_deref(),
        std::slice::from_ref(&command.input),
        command.package,
    ) {
        eprintln!("error: {message}");
        std::process::exit(1);
    }

    let input_path = PathBuf::from(&command.input);
    let target = resolve_build_target(&command, &input_path);
    let is_package = use_package_compiler(target, &input_path, command.package);
    let (config, reader_pack) = if is_package {
        match config_with_reader_locale(target, &input_path, command.reader_locale.as_deref()) {
            Ok(selection) => selection,
            Err(diag) => {
                eprintln!("error: {}", diag.message);
                std::process::exit(1);
            }
        }
    } else {
        (Config::default().with_target(target), None)
    };

    if is_package && target == Target::Scena {
        let artifact = match build_package_mir_artifact(&config, &input_path, &[]) {
            Ok(artifact) => artifact,
            Err(diagnostics) => {
                radix::tool::print_diagnostics(
                    &diagnostics,
                    DiagnosticMode::Normal,
                    reader_pack.as_ref(),
                );
                eprintln!("scena artifact build failed");
                std::process::exit(1);
            }
        };
        println!("{}", artifact.manifest_path.display());
        return;
    }

    if is_package && target == Target::FmirText {
        let image = match build_package_fmir_text_image(&config, &input_path, &[]) {
            Ok(image) => image,
            Err(diagnostics) => {
                radix::tool::print_diagnostics(
                    &diagnostics,
                    DiagnosticMode::Normal,
                    reader_pack.as_ref(),
                );
                eprintln!("fmir-text image build failed");
                std::process::exit(1);
            }
        };
        println!("{}", image.image_path.display());
        return;
    }

    if is_package && target == Target::Fmir {
        let image = match build_package_fmir_image(&config, &input_path, &[]) {
            Ok(image) => image,
            Err(diagnostics) => {
                radix::tool::print_diagnostics(
                    &diagnostics,
                    DiagnosticMode::Normal,
                    reader_pack.as_ref(),
                );
                eprintln!("fmir image build failed");
                std::process::exit(1);
            }
        };
        println!("{}", image.image_path.display());
        return;
    }

    if is_package && target == Target::FmirBin {
        let bundle =
            match build_package_fmir_binary_bundle(&config, &input_path, &[], command.release) {
                Ok(bundle) => bundle,
                Err(diagnostics) => {
                    radix::tool::print_diagnostics(
                        &diagnostics,
                        DiagnosticMode::Normal,
                        reader_pack.as_ref(),
                    );
                    eprintln!("fmir-bin bundle build failed");
                    std::process::exit(1);
                }
            };
        println!("{}", bundle.entrypoint_path.display());
        return;
    }

    if is_package && target == Target::TypeScript {
        let layout = match discover_build_layout(&input_path) {
            Ok(l) => l,
            Err(d) => {
                eprintln!("error: {}", d.message);
                std::process::exit(1);
            }
        };
        if layout.manifest_path.exists() {
            let manifest = read_manifest(&layout.manifest_path).unwrap_or_else(|diag| {
                eprintln!("error: {}", diag.message);
                std::process::exit(1);
            });
            if let Some(product) = manifest.product.as_ref() {
                match super::build_browser_product(&config, &input_path, product) {
                    Ok(build) => {
                        println!("{}", build.esm_entry.display());
                        return;
                    }
                    Err(diag) => {
                        eprintln!("error: {}", diag.message);
                        std::process::exit(1);
                    }
                }
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
        reader_pack.as_ref(),
    );

    let Some(output) = result.output else {
        eprintln!("compilation failed");
        std::process::exit(1);
    };

    // G6 GO3/GO4: package Go builds write target/faber/go and invoke `go build`.
    if is_package && target == radix::codegen::Target::Go {
        let layout = match discover_build_layout(&input_path) {
            Ok(l) => l,
            Err(d) => {
                eprintln!("error: {}", d.message);
                std::process::exit(1);
            }
        };
        let go_layout = GoBuildLayout::from_package(&layout);
        let code = output_code(output);
        let modules = super::compile::take_go_package_modules();
        if let Err(d) = emit_go_module(&go_layout, &code, &modules) {
            eprintln!("error: {}", d.message);
            std::process::exit(1);
        }
        match invoke_go_build(&go_layout) {
            Ok(binary_path) => {
                println!("{}", binary_path.display());
                return;
            }
            Err(d) => {
                eprintln!("error: {}", d.message);
                std::process::exit(1);
            }
        }
    }

    // Package Rust builds own a generated crate under target/faber/ and let
    // Cargo place artifacts in sibling debug/release directories.
    if is_package && target == radix::codegen::Target::Rust {
        let layout = match discover_build_layout(&input_path) {
            Ok(l) => l,
            Err(d) => {
                eprintln!("error: {}", d.message);
                std::process::exit(1);
            }
        };
        let meta = if layout.manifest_path.exists() {
            read_manifest(&layout.manifest_path).ok()
        } else {
            None
        };
        let mut runtime_plan = match package_rust_runtime_plan(&config, &input_path) {
            Ok(plan) => plan,
            Err(diagnostics) => {
                radix::tool::print_diagnostics(
                    &diagnostics,
                    DiagnosticMode::Normal,
                    reader_pack.as_ref(),
                );
                eprintln!("runtime plan failed");
                std::process::exit(1);
            }
        };
        if let Some(diagnostic) =
            package_host_selection_diagnostic(&runtime_plan, &layout.manifest_path)
        {
            radix::tool::print_diagnostics(
                &[diagnostic],
                DiagnosticMode::Normal,
                reader_pack.as_ref(),
            );
            eprintln!("runtime plan failed");
            std::process::exit(1);
        }
        // G4: emit native-binding library crates before the application crate links them.
        match super::library_link::emit_linked_library_crates(&layout.package_root, &layout) {
            Ok(linked) => {
                runtime_plan.library_path_deps = linked
                    .into_iter()
                    .map(|lib| (lib.crate_name, lib.crate_root))
                    .collect();
            }
            Err(diagnostics) => {
                radix::tool::print_diagnostics(
                    &diagnostics,
                    DiagnosticMode::Normal,
                    reader_pack.as_ref(),
                );
                eprintln!("library dependency graph failed");
                std::process::exit(1);
            }
        }
        match emit_generated_crate_with_runtime_plan(
            &layout,
            &output_code(output),
            meta.as_ref(),
            &runtime_plan,
        ) {
            Ok(_crate_root) => {
                let binary_path = match invoke_cargo_build(&layout, command.release) {
                    Ok(p) => p,
                    Err(d) => {
                        eprintln!("error: {}", d.message);
                        std::process::exit(1);
                    }
                };
                println!("{}", binary_path.display());
                return;
            }
            Err(d) => {
                eprintln!("error: {}", d.message);
                std::process::exit(1);
            }
        }
    }

    let code = output_code(output);

    // Single-file Rust builds that need faber-runtime / tokio (from HIR facts,
    // not emitted-text sniffing) emit a generated Cargo crate under
    // `target/faber/`. Programs without those deps keep the bare `.rs` path.
    if target == radix::codegen::Target::Rust {
        match single_file_rust_runtime_plan(&config, &input_path) {
            Ok(plan) if plan.requires_generated_crate() => {
                let package_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                let stem = input_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .filter(|s| !s.is_empty())
                    .unwrap_or("faber_out");
                let layout = BuildLayout::from_package_root(&package_root, stem);
                match emit_generated_crate_with_runtime_plan(&layout, &code, None, &plan) {
                    Ok(_) => match invoke_cargo_build(&layout, command.release) {
                        Ok(binary_path) => {
                            println!("{}", binary_path.display());
                            return;
                        }
                        Err(d) => {
                            eprintln!("error: {}", d.message);
                            std::process::exit(1);
                        }
                    },
                    Err(d) => {
                        eprintln!("error: {}", d.message);
                        std::process::exit(1);
                    }
                }
            }
            Ok(_) => {}
            Err(diagnostics) => {
                radix::tool::print_diagnostics(
                    &diagnostics,
                    DiagnosticMode::Normal,
                    reader_pack.as_ref(),
                );
                eprintln!("runtime plan failed");
                std::process::exit(1);
            }
        }
    }

    // Legacy single-file path (direct .fab files, other targets, or --out-dir override cases)
    let output_path =
        radix::tool::build_output_path(&command.out_dir, &input_path, target, is_package);
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).unwrap_or_else(|err| {
            eprintln!("error: failed to create '{}': {}", parent.display(), err);
            std::process::exit(1);
        });
    }

    fs::write(&output_path, &code).unwrap_or_else(|err| {
        eprintln!(
            "error: failed to write '{}': {}",
            output_path.display(),
            err
        );
        std::process::exit(1);
    });

    println!("{}", output_path.display());
}

/// Structured runtime plan for a single `.fab` file (no package manifest).
///
/// Uses HIR/type facts via [`radix::codegen::collect_rust_needs`] — never scans
/// generated Rust text for `faber::` / `tokio::`.
fn single_file_rust_runtime_plan(
    config: &radix::Config,
    input_path: &Path,
) -> Result<super::RustRuntimePlan, Vec<radix::Diagnostic>> {
    use radix::driver::{analyze_source, Session};
    use std::collections::BTreeSet;
    use std::fs;

    let source = fs::read_to_string(input_path).map_err(|err| {
        vec![crate::package_diagnostic_error(format!(
            "failed to read '{}': {err}",
            input_path.display()
        ))]
    })?;
    let session = Session::new(config.clone());
    let name = input_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("input.fab");
    let analysis = analyze_source(&session, name, &source)?;
    let needs =
        radix::codegen::collect_rust_needs(&analysis.hir, &analysis.types, BTreeSet::new(), None);
    let needs_tokio =
        analysis.hir.entry_is_async
            || analysis.hir.items.iter().any(
                |item| matches!(&item.kind, radix::hir::HirItemKind::Function(f) if f.is_async),
            );
    Ok(super::RustRuntimePlan {
        needs_faber: needs.needs_faber_runtime,
        needs_tokio,
        host: None,
        non_runtime_routes: BTreeSet::new(),
        selected_providers: BTreeSet::new(),
        provider_manifests: Vec::new(),
        provider_error: None,
        library_path_deps: Vec::new(),
    })
}

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

    let manifest = read_manifest(&layout.manifest_path).unwrap_or_else(|diag| {
        eprintln!("error: {}", diag.message);
        std::process::exit(1);
    });
    manifest_build_target(&manifest.build.target, &layout.manifest_path).unwrap_or_else(|diag| {
        eprintln!("error: {}", diag.message);
        std::process::exit(1);
    })
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
pub fn use_package_compiler(target: Target, path: &std::path::Path, force_package: bool) -> bool {
    if force_package {
        return true;
    }
    if path.is_dir() || path.file_name().and_then(|name| name.to_str()) == Some(MANIFEST_FILE) {
        return true;
    }
    if path.extension().is_some_and(|ext| ext == "fab") {
        return matches!(
            target,
            Target::Rust | Target::Scena | Target::FmirText | Target::Fmir | Target::FmirBin
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
    let (config, reader_pack) = match config_with_reader_locale(
        Target::Rust,
        &input_path,
        command.reader_locale.as_deref(),
    ) {
        Ok(selection) => selection,
        Err(diag) => {
            eprintln!("error: {}", diag.message);
            std::process::exit(1);
        }
    };
    let diagnostics = check_package(&config, &input_path);

    let mut fatal_errors = 0usize;
    let mut downgraded = 0usize;
    if command.diagnostic_mode == DiagnosticMode::Diagnostics && !diagnostics.is_empty() {
        match reader_pack.as_ref() {
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
pub fn cmd_emit_package(command: radix::tool::EmitCommand) {
    let (result, reader_pack) = compile_package_input(
        &command.input,
        command.package,
        command.target,
        command.reader_locale.as_deref(),
    );

    radix::tool::print_diagnostics(
        &result.diagnostics,
        command.diagnostic_mode,
        reader_pack.as_ref(),
    );

    let Some(output) = result.output else {
        eprintln!("compilation failed");
        std::process::exit(1);
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
        radix::tool::write_output_artifact(
            &path,
            output,
            command.target,
            command.format,
            command.linter,
        );
        return;
    }

    let code = radix::tool::postprocess_code(
        output_code(output),
        command.target,
        command.format,
        command.linter,
    );
    print!("{code}");
}

fn compile_package_input(
    input: &[String],
    force_package: bool,
    target: Target,
    reader_locale: Option<&str>,
) -> (
    CompileResult,
    Option<radix::reader_locale::ReaderLocalePack>,
) {
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

    let (config, reader_pack) = match config_with_reader_locale(target, &path, reader_locale) {
        Ok(selection) => selection,
        Err(diag) => {
            eprintln!("error: {}", diag.message);
            std::process::exit(1);
        }
    };
    (compile_package(&config, &path), reader_pack)
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
        Output::Wasm(_) => panic!("binary Wasm output is not supported in faber package builds"),
        Output::LlvmText(out) => out.code,
        Output::MetalText(out) => out.code,
        Output::WgslText(out) => out.code,
        Output::Sexp(out) => out.code,
        Output::Swift(out) => out.code,
    }
}
