//! `emit` and `build` command handlers plus emitted-artifact path helpers.
//!
//! This file is the process-facing source-emission boundary: compile a
//! single-file input, print diagnostics, and write source or Wasm bytes.
//! Compilation is delegated to [`super::compile`]. Target-specific formatting,
//! linting, binary compilation, and execution belong to Faber or external
//! tooling, not Radix.

use super::super::cli::{BuildCommand, DiagnosticMode, EmitCommand};
use super::compile::{
    compile_cli_input_with_locale_pack, compile_cli_input_with_locale_pack_explicit, compile_cli_path,
    resolve_package_mode,
};
use super::diagnostics_locale_pack::load_diagnostics_locale_pack_or_exit;
#[cfg(feature = "mir-core")]
use super::reflection::output_reflection_json;
use super::targets::target_extension;
#[cfg(feature = "hir-fhir")]
use handroll::Sha256;
use std::fs;
use std::path::{Path, PathBuf};

/// Compile single-file input to a target language and print generated source.
///
/// Package inputs are rejected before compilation so users do not accidentally
/// get a single-file interpretation of a package directory or manifest.
pub fn cmd_emit(command: EmitCommand) {
    let diagnostics_locale_pack = load_diagnostics_locale_pack_or_exit(command.diagnostics_locale_pack.as_ref());
    // An explicit `--locale` wins over source frontmatter (then the Latin
    // default). `--diagnostics-locale` is a TOML pack path and drives message
    // rendering only.
    let explicit_locale = command.locale.as_deref();
    let code_pack = super::compile::explicit_locale_pack(explicit_locale);
    let code_pack_explicit = explicit_locale.is_some();
    cmd_emit_with_locale_packs_inner(
        command,
        code_pack.as_ref(),
        diagnostics_locale_pack.as_ref(),
        code_pack_explicit,
    );
}

/// Emit given a pre-resolved reader pack (used for both code surface and diagnostics).
///
/// [`cmd_emit`] only loads a diagnostics pack from `--diagnostics-locale`.
/// Callers that already resolved a pack (install layout, package manifest)
/// can pass it here instead of asking this file to rediscover it.
pub fn cmd_emit_with_locale_pack(command: EmitCommand, code_pack: Option<&crate::locale::LocalePack>) {
    cmd_emit_with_locale_packs(command, code_pack, code_pack);
}

/// Emit with separate code and diagnostic locale packs.
///
/// `code_pack` drives keyword/type lexing and Faber re-emit surface.
/// `diagnostic_pack` drives message rendering only.
pub fn cmd_emit_with_locale_packs(
    command: EmitCommand,
    code_pack: Option<&crate::locale::LocalePack>,
    diagnostic_pack: Option<&crate::locale::LocalePack>,
) {
    cmd_emit_with_locale_packs_inner(command, code_pack, diagnostic_pack, false);
}

fn cmd_emit_with_locale_packs_inner(
    command: EmitCommand,
    code_pack: Option<&crate::locale::LocalePack>,
    diagnostic_pack: Option<&crate::locale::LocalePack>,
    code_pack_explicit: bool,
) {
    // `--cuda-descriptor` is valid for llvm-text output only. Fail closed on
    // any other target before codegen runs.
    #[cfg(feature = "mir-llvm")]
    if command.cuda_descriptor.is_some() && command.target != crate::codegen::Target::MirLlvm {
        eprintln!("error: --cuda-descriptor requires --target llvm-text");
        std::process::exit(1);
    }

    // Target::HirFhir is not a codegen backend — it serializes the analyzed unit
    // via HirArtifact + postcard and never calls generate_from_analyzed.
    if command.target == crate::codegen::Target::HirFhir {
        emit_fhir(&command);
        return;
    }

    let compile = if code_pack_explicit {
        compile_cli_input_with_locale_pack_explicit
    } else {
        compile_cli_input_with_locale_pack
    };
    let result = compile(
        &command.input,
        command.package,
        command.target,
        code_pack,
        command.output_mode,
        command.module_name.as_deref(),
    );

    print_diagnostics(&result.diagnostics, command.diagnostic_mode, diagnostic_pack);

    let Some(output) = result.output else {
        eprintln!("compilation failed");
        std::process::exit(1);
    };

    #[cfg(feature = "mir-llvm")]
    if let Some(descriptor_path) = command.cuda_descriptor.as_deref() {
        write_cuda_descriptor_sidecar(&command, code_pack, code_pack_explicit, descriptor_path);
    }

    #[cfg(feature = "mir-core")]
    if command.reflection {
        emit_reflection_output(&output, command.output);
        return;
    }
    #[cfg(not(feature = "mir-core"))]
    if command.reflection {
        eprintln!("error: GPU reflection requires the mir-core lane; rebuild with --features mir-core");
        std::process::exit(1);
    }

    if let Some(path) = command.output {
        write_output_artifact(&path, output);
        return;
    }

    write_stdout_output(output);
}

/// Write the NVVM kernel descriptor JSON sidecar for a successful `llvm-text` emit.
///
/// The compiled [`crate::Output`] does not carry MIR, so the descriptor is
/// re-derived from the same source through the driver's public front-half
/// pipeline ([`crate::driver::cuda_kernel_descriptor_json`]); analysis is
/// deterministic, so the sidecar matches the emitted text. Fail closed: an
/// unreadable input, a failed analysis, or a module without kernel-role
/// functions exits non-zero with a rendered diagnostic.
#[cfg(feature = "mir-llvm")]
fn write_cuda_descriptor_sidecar(
    command: &EmitCommand,
    code_pack: Option<&crate::locale::LocalePack>,
    code_pack_explicit: bool,
    descriptor_path: &Path,
) {
    // Stdin cannot be re-read: the codegen pass already consumed the stream.
    if command.input.is_empty() || command.input[0] == "-" {
        eprintln!("error: --cuda-descriptor requires a file input (stdin cannot be re-read)");
        std::process::exit(1);
    }

    let (name, source) = super::source::read_source(&command.input);
    let session = cuda_descriptor_session(command, code_pack, code_pack_explicit);
    match crate::driver::cuda_kernel_descriptor_json(&session, &name, &source) {
        Ok(json) => write_text_artifact(descriptor_path, &format!("{json}\n")),
        Err(diagnostics) => {
            for diag in &diagnostics {
                if diag.is_error() {
                    eprintln!("{}", crate::diagnostics::render_plain(diag));
                }
            }
            eprintln!(
                "error: failed to write CUDA kernel descriptor to '{}'",
                descriptor_path.display()
            );
            std::process::exit(1);
        }
    }
}

/// Build the session the descriptor re-derivation shares with the codegen pass
/// (target, reader pack, output mode, module name, dev stdlib).
#[cfg(feature = "mir-llvm")]
fn cuda_descriptor_session(
    command: &EmitCommand,
    code_pack: Option<&crate::locale::LocalePack>,
    code_pack_explicit: bool,
) -> crate::driver::Session {
    let mut config = crate::driver::Config::default()
        .with_target(command.target)
        .with_output_mode(command.output_mode)
        .with_dev_stdlib();
    if let Some(name) = command.module_name.as_deref() {
        config = config.with_module_name(Some(name.to_owned()));
    }
    if let Some(pack) = code_pack {
        config = config.with_locale_pack(pack.clone());
    }
    if code_pack_explicit {
        config = config.with_locale_pack_explicit();
    }
    crate::driver::Session::new(config)
}

fn write_stdout_output(output: crate::Output) {
    print!("{}", output_code(output));
}

#[cfg(feature = "mir-core")]
fn emit_reflection_output(output: &crate::Output, output_path: Option<PathBuf>) {
    let reflection_json = match output_reflection_json(output) {
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

    if let Some(path) = output_path {
        write_text_artifact(&path, &reflection_json);
    } else {
        println!("{reflection_json}");
    }
}

/// Compile single-file input and write generated source to disk.
///
/// This is intentionally source-emission only inside `radix`; executable
/// package builds are routed through `faber`, where generated Cargo layout
/// policy is available.
pub fn cmd_build(command: BuildCommand) {
    let input_path = PathBuf::from(&command.input);
    let is_package = resolve_package_mode(&input_path, command.package);
    let result = compile_cli_path(&input_path, is_package, command.target);

    print_diagnostics(&result.diagnostics, DiagnosticMode::Normal, None);

    let Some(output) = result.output else {
        eprintln!("compilation failed");
        std::process::exit(1);
    };

    let output_path = build_output_path(&command.out_dir, &input_path, command.target, is_package);
    write_output_artifact(&output_path, output);
    println!("{}", output_path.display());
}

pub fn print_diagnostics(
    diagnostics: &[crate::diagnostics::Diagnostic],
    mode: DiagnosticMode,
    diagnostics_locale_pack: Option<&crate::locale::LocalePack>,
) {
    if mode != DiagnosticMode::Diagnostics {
        for diag in diagnostics {
            eprintln!("{}", crate::diagnostics::render_plain(diag));
        }
        return;
    }

    if diagnostics.is_empty() {
        return;
    }

    match diagnostics_locale_pack {
        Some(pack) => match crate::diagnostics::render_expanded_diagnostics_with_pack(diagnostics, pack) {
            Ok(rendered) => eprintln!("{rendered}"),
            Err(err) => {
                eprintln!("error: failed to render reader-pack diagnostics: {err}");
                std::process::exit(1);
            }
        },
        None => eprintln!("{}", crate::diagnostics::render_expanded_diagnostics(diagnostics)),
    }
}

/// Derive the output path for source-emission builds.
///
/// Package mode currently uses a stable `main.<ext>` placeholder here because
/// real package build layout is outside `radix` and owned by `faber`.
#[must_use]
pub fn build_output_path(
    out_dir: &Path,
    input_path: &Path,
    target: crate::codegen::Target,
    is_package: bool,
) -> PathBuf {
    let base_name = if is_package {
        "main".to_owned()
    } else {
        input_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .filter(|stem| !stem.is_empty())
            .unwrap_or("out")
            .to_owned()
    };
    out_dir.join(format!("{}.{}", base_name, target_extension(target)))
}

/// Extract generated source from a target-specific compiler output.
#[must_use]
pub fn output_code(output: crate::Output) -> String {
    match output {
        crate::Output::Rust(out) => out.code,
        crate::Output::TypeScript(out) => out.code,
        crate::Output::Go(out) => out.code,
        crate::Output::Swift(out) => out.code,
        crate::Output::Python(out) => out.code,
        crate::Output::Haskell(out) => out.code,
        crate::Output::Faber(out) => out.code,
        #[cfg(feature = "mir-wasm")]
        crate::Output::WasmText(out) => out.code,
        #[cfg(feature = "mir-llvm")]
        crate::Output::LlvmText(out) => out.code,
        #[cfg(feature = "mir-metal")]
        crate::Output::MetalText(out) => out.code,
        #[cfg(feature = "mir-wgsl")]
        crate::Output::WgslText(out) => out.code,
        #[cfg(feature = "mir-sexp")]
        crate::Output::Sexp(out) => out.code,
    }
}

pub fn write_output_artifact(path: &Path, output: crate::Output) {
    ensure_artifact_parent(path);
    write_artifact_contents(path, output_code(output));
}

pub fn write_text_artifact(path: &Path, text: &str) {
    ensure_artifact_parent(path);
    write_artifact_contents(path, text);
}

fn ensure_artifact_parent(path: &Path) {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).unwrap_or_else(|err| {
                eprintln!("error: failed to create '{}': {}", parent.display(), err);
                std::process::exit(1);
            });
        }
    }
}

fn write_artifact_contents(path: &Path, contents: impl AsRef<[u8]>) {
    fs::write(path, contents).unwrap_or_else(|err| {
        eprintln!("error: failed to write '{}': {}", path.display(), err);
        std::process::exit(1);
    });
}

// FHIR ARTIFACT

/// Serialize an analyzed unit as a `.fhir` artifact without codegen.
///
/// Reads the source file, runs the full frontend analysis (lex, parse, resolve,
/// typecheck), constructs a [`radix_hir_fhir::HirArtifact`], and writes
/// the postcard-serialized bytes to the output path. This is not a codegen
/// backend — no lowering, no `generate_from_analyzed` call.
#[cfg(feature = "hir-fhir")]
fn emit_fhir(command: &EmitCommand) {
    let input_path = if command.input.is_empty() || command.input[0] == "-" {
        eprintln!("error: FHIR emission requires a file path input (stdin not supported)");
        std::process::exit(1);
    } else {
        PathBuf::from(&command.input[0])
    };

    // .fhir is an output format only; input must be .fab.
    if input_path.extension().is_none_or(|ext| ext != "fab") {
        eprintln!(
            "error: FHIR emission requires a .fab input file; got '{}'",
            input_path.display()
        );
        std::process::exit(1);
    }

    let source = fs::read_to_string(&input_path).unwrap_or_else(|err| {
        eprintln!("error: failed to read '{}': {}", input_path.display(), err);
        std::process::exit(1);
    });

    let content_hash = {
        let mut hasher = Sha256::new();
        hasher.update(source.as_bytes());
        hasher.finalize_hex()
    };
    let relative_path = input_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown.fab")
        .to_string();

    let output_path = command
        .output
        .clone()
        .unwrap_or_else(|| input_path.with_extension("fhir"));

    let session = crate::driver::Session::new(
        crate::driver::Config::default()
            .with_target(crate::codegen::Target::HirFhir)
            .with_dev_stdlib(),
    );
    let name = input_path.display().to_string();
    let prepared = match crate::tool::commands::compile::prepare_program_module(&input_path, &source, &session) {
        Ok(prepared) => prepared,
        Err(diagnostic) => {
            eprintln!("{}", crate::diagnostics::render_plain(&diagnostic));
            eprintln!("error: analysis failed for '{}'", input_path.display());
            std::process::exit(1);
        }
    };
    let analyzed = match crate::driver::analyze_source_with_module_import_contract(
        &prepared.session,
        &name,
        &source,
        None,
        prepared.contract,
    ) {
        Ok(unit) => unit,
        Err(diagnostics) => {
            for diag in &diagnostics {
                if diag.is_error() {
                    eprintln!("{}", crate::diagnostics::render_plain(diag));
                }
            }
            eprintln!("error: analysis failed for '{}'", input_path.display());
            std::process::exit(1);
        }
    };

    let artifact = crate::hir::serialize::from_analyzed(&analyzed, &content_hash, &relative_path);
    let bytes = radix_hir_fhir::encode(&artifact).unwrap_or_else(|err| {
        eprintln!("error: postcard serialization failed: {err}");
        std::process::exit(1);
    });

    ensure_artifact_parent(&output_path);
    write_artifact_contents(&output_path, &bytes);
    eprintln!("wrote {} bytes to {}", bytes.len(), output_path.display());
}

/// Fail closed when a build excludes the FHIR artifact backend.
#[cfg(not(feature = "hir-fhir"))]
fn emit_fhir(_command: &EmitCommand) {
    eprintln!("error[CODEGEN001:target_unavailable]: target `fhir` is not available in this radix build; rebuild with feature `hir-fhir`");
    eprintln!("target=fhir");
    eprintln!("feature=hir-fhir");
    std::process::exit(1);
}
